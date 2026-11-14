//! Image validation, resizing/upscaling and mix-image composition.
//!
//! Every downloaded file is fully decoded with strict limits and re-encoded
//! as a fresh PNG. That throws away anything that isn't pixels (metadata,
//! trailing data, malformed chunks), so only clean images reach your drive.

use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, ImageReader, Limits, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::{Path, PathBuf};

const MAX_DIM: u32 = 8192;

/// Quick magic-number check for PNG / JPEG / WebP.
pub fn looks_like_image(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG\r\n\x1a\n")
        || b.starts_with(&[0xFF, 0xD8, 0xFF])
        || (b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP")
}

/// Decode untrusted bytes into an image, with size and memory limits.
pub fn decode_validated(bytes: &[u8]) -> anyhow::Result<DynamicImage> {
    anyhow::ensure!(looks_like_image(bytes), "not a PNG/JPEG/WebP image");
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIM);
    limits.max_image_height = Some(MAX_DIM);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode()?;
    anyhow::ensure!(
        img.width() >= 8 && img.height() >= 8,
        "image is too small to be useful"
    );
    Ok(img)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Upscale {
    /// Never enlarge; only shrink big images.
    Off,
    /// High-quality smooth (Lanczos) resampling.
    Smooth,
    /// Pixel-art aware: crisp integer scaling for screenshots, smooth for box art.
    Smart,
    /// Use Real-ESRGAN (ncnn-vulkan) if the user has installed it; falls back to Smart.
    Ai,
}

/// Target box an image must fit in (aspect ratio is always kept).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Size {
    pub w: u32,
    pub h: u32,
}

impl Size {
    pub fn scaled(self, f: f32) -> Size {
        Size {
            w: ((self.w as f32) * f).round().max(16.0) as u32,
            h: ((self.h as f32) * f).round().max(16.0) as u32,
        }
    }
}

/// Dimensions that fit `(w,h)` inside `target` keeping aspect ratio.
fn fit_dims(w: u32, h: u32, target: Size) -> (u32, u32) {
    let s = (target.w as f64 / w as f64).min(target.h as f64 / h as f64);
    (
        ((w as f64) * s).round().max(1.0) as u32,
        ((h as f64) * s).round().max(1.0) as u32,
    )
}

/// Resize an image to fit `target`.
pub fn fit(img: &DynamicImage, target: Size, mode: Upscale, pixel_art: bool) -> RgbaImage {
    let (w, h) = img.dimensions();
    let (tw, th) = fit_dims(w, h, target);
    let rgba = img.to_rgba8();
    if tw <= w && th <= h {
        // Downscale (or same size): Lanczos is the sharpest high-quality filter.
        if tw == w && th == h {
            return rgba;
        }
        return imageops::resize(&rgba, tw, th, FilterType::Lanczos3);
    }
    match mode {
        Upscale::Off => rgba,
        Upscale::Smooth => imageops::resize(&rgba, tw, th, FilterType::Lanczos3),
        Upscale::Smart | Upscale::Ai => {
            if pixel_art {
                // Crisp: integer nearest-neighbour up to just below the target,
                // then a gentle filter for the remaining fraction.
                let factor = (tw / w).max(1).min(th / h).max(1);
                let int = if factor > 1 {
                    imageops::resize(&rgba, w * factor, h * factor, FilterType::Nearest)
                } else {
                    rgba
                };
                if int.width() == tw && int.height() == th {
                    int
                } else {
                    imageops::resize(&int, tw, th, FilterType::Triangle)
                }
            } else {
                let up = imageops::resize(&rgba, tw, th, FilterType::Lanczos3);
                unsharp(&up)
            }
        }
    }
}

/// Light sharpening to counter softness after enlarging photos/box art.
fn unsharp(img: &RgbaImage) -> RgbaImage {
    imageops::unsharpen(img, 0.8, 2)
}

/// Upscale using an external Real-ESRGAN ncnn-vulkan executable chosen by the
/// user. Runs it directly (no shell) on a temp copy and validates the output.
pub async fn ai_upscale(binary: &Path, img: &DynamicImage) -> anyhow::Result<DynamicImage> {
    anyhow::ensure!(binary.is_file(), "Real-ESRGAN executable not found");
    let tmp =
        std::env::temp_dir().join(format!("shelfart-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&tmp)?;
    let input = tmp.join("in.png");
    let output = tmp.join("out.png");
    img.save(&input)?;
    let model = "realesrgan-x4plus";
    let mut cmd = tokio::process::Command::new(binary);
    cmd.arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("-n")
        .arg(model)
        .arg("-s")
        .arg("4");
    if let Some(dir) = binary.parent() {
        cmd.current_dir(dir);
    }
    cmd.kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let status = tokio::time::timeout(std::time::Duration::from_secs(180), cmd.status()).await??;
    let result = (|| -> anyhow::Result<DynamicImage> {
        anyhow::ensure!(status.success(), "Real-ESRGAN failed");
        let bytes = std::fs::read(&output)?;
        decode_validated(&bytes)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Compose a "mix" image: screenshot as the backdrop, box art bottom-left,
/// logo along the bottom — the classic EmulationStation look.
pub fn compose_mix(
    snap: Option<&DynamicImage>,
    boxart: Option<&DynamicImage>,
    logo: Option<&DynamicImage>,
    size: Size,
) -> Option<RgbaImage> {
    let snap = snap?;
    let (cw, ch) = (size.w, size.h);
    let mut canvas = RgbaImage::from_pixel(cw, ch, Rgba([0, 0, 0, 0]));

    // Screenshot: top-right area, ~88% x 82%.
    let sw = (cw as f32 * 0.88) as u32;
    let shh = (ch as f32 * 0.82) as u32;
    let s = fit(snap, Size { w: sw, h: shh }, Upscale::Smart, true);
    let sx = cw - s.width();
    imageops::overlay(&mut canvas, &s, sx as i64, 0);

    // Box art: bottom-left, ~42% height, with a soft shadow.
    if let Some(b) = boxart {
        let bimg = fit(
            b,
            Size {
                w: (cw as f32 * 0.38) as u32,
                h: (ch as f32 * 0.60) as u32,
            },
            Upscale::Smart,
            false,
        );
        let bx = (cw as f32 * 0.02) as i64;
        let by = ch as i64 - bimg.height() as i64 - (ch as f32 * 0.02) as i64;
        draw_shadow(&mut canvas, &bimg, bx + 8, by + 8);
        imageops::overlay(&mut canvas, &bimg, bx, by);
    }

    // Logo: bottom-right band.
    if let Some(l) = logo {
        let limg = fit(
            l,
            Size {
                w: (cw as f32 * 0.52) as u32,
                h: (ch as f32 * 0.20) as u32,
            },
            Upscale::Smart,
            false,
        );
        let lx = cw as i64 - limg.width() as i64 - (cw as f32 * 0.03) as i64;
        let ly = ch as i64 - limg.height() as i64 - (ch as f32 * 0.02) as i64;
        draw_shadow(&mut canvas, &limg, lx + 4, ly + 4);
        imageops::overlay(&mut canvas, &limg, lx, ly);
    }
    Some(canvas)
}

/// Render a 3D-looking box from a flat front cover: the cover turned slightly
/// to the right with a spine on the left, perspective, shading and a highlight.
pub fn render_box3d(front: &DynamicImage, size: Size) -> RgbaImage {
    // Work at a decent resolution, then fit to the requested size.
    let src = {
        let f = front.to_rgba8();
        if f.height() < 900 {
            let s = 900.0 / f.height() as f32;
            imageops::resize(&f, (f.width() as f32 * s) as u32, 900, FilterType::Lanczos3)
        } else {
            f
        }
    };
    let (w, h) = (src.width() as f32, src.height() as f32);
    let angle: f32 = 24f32.to_radians();
    let thickness = w * 0.14;
    let front_w = w * angle.cos();
    let spine_w = thickness * angle.sin() * 2.2;
    let far_front = 0.90; // far edge of the front appears shorter
    let far_spine = 0.95;
    let pad = (h * 0.04) as u32;
    let out_w = (front_w + spine_w) as u32 + pad * 2;
    let out_h = h as u32 + pad * 2;
    let mut out = RgbaImage::from_pixel(out_w, out_h, Rgba([0, 0, 0, 0]));

    // Spine texture: the cover's left edge, stretched and darkened.
    let strip_w = ((w * 0.05) as u32).max(2);
    let strip = imageops::crop_imm(&src, 0, 0, strip_w, src.height()).to_image();

    let sample = |img: &RgbaImage, x: f32, y: f32| -> Rgba<u8> {
        let x = x.clamp(0.0, img.width() as f32 - 1.001);
        let y = y.clamp(0.0, img.height() as f32 - 1.001);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p = |xx: u32, yy: u32| {
            img.get_pixel(xx.min(img.width() - 1), yy.min(img.height() - 1))
                .0
        };
        let (a, b, c, d) = (p(x0, y0), p(x0 + 1, y0), p(x0, y0 + 1), p(x0 + 1, y0 + 1));
        let mut o = [0u8; 4];
        for i in 0..4 {
            let top = a[i] as f32 * (1.0 - fx) + b[i] as f32 * fx;
            let bot = c[i] as f32 * (1.0 - fx) + d[i] as f32 * fx;
            o[i] = (top * (1.0 - fy) + bot * fy).round() as u8;
        }
        Rgba(o)
    };

    let edge_x = pad as f32 + spine_w;
    // Spine: near edge (x = edge_x) full height, far edge shorter.
    for ox in 0..spine_w.ceil() as u32 {
        let t = 1.0 - ox as f32 / spine_w; // 1 at far, 0 at near
        let col_h = h * (1.0 - (1.0 - far_spine) * t);
        let top = pad as f32 + (h - col_h) / 2.0;
        let sx = (1.0 - t) * (strip.width() as f32 - 1.0);
        let shade = 0.55 + 0.15 * (1.0 - t);
        for oy in 0..col_h as u32 {
            let sy = oy as f32 / col_h * h;
            let mut px = sample(&strip, sx, sy);
            for c in 0..3 {
                px[c] = (px[c] as f32 * shade) as u8;
            }
            out.put_pixel(pad + ox, (top as u32 + oy).min(out_h - 1), px);
        }
    }
    // Front: perspective-correct horizontal mapping.
    for ox in 0..front_w as u32 {
        let t_lin = ox as f32 / front_w;
        // Simple perspective correction: farther parts are compressed.
        let z0 = 1.0;
        let z1 = 1.0 / far_front;
        let t = (t_lin / z1) / ((1.0 - t_lin) / z0 + t_lin / z1);
        let col_h = h * (1.0 - (1.0 - far_front) * t_lin);
        let top = pad as f32 + (h - col_h) / 2.0;
        let sx = t * (w - 1.0);
        let light = 1.04 - 0.10 * t_lin;
        for oy in 0..col_h as u32 {
            let sy = oy as f32 / col_h * h;
            let mut px = sample(&src, sx, sy);
            for c in 0..3 {
                px[c] = ((px[c] as f32) * light).min(255.0) as u8;
            }
            out.put_pixel(
                (edge_x as u32 + ox).min(out_w - 1),
                (top as u32 + oy).min(out_h - 1),
                px,
            );
        }
    }
    // Edge highlight where spine meets front.
    for oy in pad..(pad + h as u32).min(out_h) {
        for dx in 0..2 {
            let x = (edge_x as u32 + dx).min(out_w - 1);
            let p = out.get_pixel_mut(x, oy);
            for c in 0..3 {
                p[c] = p[c].saturating_add(40);
            }
        }
    }
    fit(&DynamicImage::ImageRgba8(out), size, Upscale::Smart, false)
}

fn draw_shadow(canvas: &mut RgbaImage, img: &RgbaImage, x: i64, y: i64) {
    let pad = 12u32;
    let mut mask = RgbaImage::from_pixel(
        img.width() + pad * 2,
        img.height() + pad * 2,
        Rgba([0, 0, 0, 0]),
    );
    for (px, py, p) in img.enumerate_pixels() {
        let a = (p[3] as f32 * 0.55) as u8;
        mask.put_pixel(px + pad, py + pad, Rgba([0, 0, 0, a]));
    }
    let blurred = imageops::blur(&mask, 6.0);
    imageops::overlay(canvas, &blurred, x - pad as i64, y - pad as i64);
}

/// Write a PNG atomically (temp file + rename) so a crash never leaves a
/// half-written image that a launcher would choke on.
pub fn save_png(img: &RgbaImage, path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp: PathBuf = path.with_extension("png.part");
    {
        let f = std::fs::File::create(&tmp)?;
        let mut w = std::io::BufWriter::new(f);
        let enc = image::codecs::png::PngEncoder::new_with_quality(
            &mut w,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        );
        img.write_with_encoder(enc)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Write raw bytes (videos, manuals) atomically.
pub fn save_bytes(bytes: &[u8], path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, Rgba([200, 30, 30, 255]));
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn rejects_non_images() {
        assert!(decode_validated(b"<html>nope</html>").is_err());
        let mut evil = b"\x89PNG\r\n\x1a\n".to_vec();
        evil.extend_from_slice(b"garbage");
        assert!(decode_validated(&evil).is_err());
    }

    #[test]
    fn fits_and_upscales() {
        let img = decode_validated(&png_bytes(256, 224)).unwrap();
        let out = fit(&img, Size { w: 1280, h: 960 }, Upscale::Smart, true);
        assert_eq!((out.width(), out.height()), (1097, 960));
        let out = fit(&img, Size { w: 1280, h: 960 }, Upscale::Off, true);
        assert_eq!((out.width(), out.height()), (256, 224));
        let big = decode_validated(&png_bytes(2000, 1000)).unwrap();
        let out = fit(&big, Size { w: 1000, h: 1000 }, Upscale::Off, false);
        assert_eq!((out.width(), out.height()), (1000, 500));
    }

    #[test]
    fn box3d_renders() {
        let img = decode_validated(&png_bytes(512, 720)).unwrap();
        let b = render_box3d(&img, Size { w: 900, h: 1200 });
        assert!(b.width() > 100 && b.height() <= 1200);
        // Corners are transparent, centre is opaque.
        assert_eq!(b.get_pixel(0, 0)[3], 0);
        assert_eq!(b.get_pixel(b.width() / 2, b.height() / 2)[3], 255);
    }

    #[test]
    fn mix_needs_snap() {
        let img = decode_validated(&png_bytes(320, 240)).unwrap();
        assert!(compose_mix(None, Some(&img), None, Size { w: 640, h: 480 }).is_none());
        let m = compose_mix(Some(&img), Some(&img), Some(&img), Size { w: 640, h: 480 }).unwrap();
        assert_eq!((m.width(), m.height()), (640, 480));
    }
}
