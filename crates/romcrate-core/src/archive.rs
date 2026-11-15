//! Zipped game folders: if the user picks a `.zip` instead of a folder,
//! RomCrate unpacks it next to the zip and then carries on as normal.
//!
//! Safety rules for untrusted archives:
//! * every entry path is checked — nothing can be written outside the target
//!   folder ("zip slip"), absolute paths and `..` are refused;
//! * symlink entries are skipped;
//! * the unpacked size is checked against free disk space *before* writing,
//!   and against a hard cap, and entries that expand suspiciously (zip bombs)
//!   are refused while streaming;
//! * the original zip is never modified or deleted, and existing files are
//!   never overwritten.

use serde::Serialize;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Refuse archives that claim to unpack to more than this.
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024 * 1024; // 2 TB
const MAX_ENTRIES: usize = 200_000;
/// Entries that expand more than this ratio are treated as zip bombs
/// (real ROMs rarely exceed ~20:1; blank padding can reach a few hundred).
const MAX_RATIO: u64 = 1_000;
/// Keep this much space free on the target drive.
const SPACE_MARGIN: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipPlan {
    pub zip: PathBuf,
    pub target: PathBuf,
    pub files: usize,
    pub unpacked_bytes: u64,
    pub free_bytes: Option<u64>,
    /// Already unpacked earlier (target exists and holds every file).
    pub already_unpacked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnzipProgress {
    pub zip: PathBuf,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub current: String,
}

pub fn is_zip(p: &Path) -> bool {
    p.is_file()
        && p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("zip"))
            .unwrap_or(false)
}

/// Folder a zip unpacks into: next to the zip, named after it.
pub fn target_for(zip: &Path) -> PathBuf {
    let stem = zip
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unzipped");
    zip.with_file_name(crate::paths::safe_component(stem))
}

/// Inspect a zip without writing anything.
pub fn plan(zip: &Path) -> anyhow::Result<ZipPlan> {
    let mut ar = zip::ZipArchive::new(File::open(zip)?)?;
    anyhow::ensure!(
        ar.len() <= MAX_ENTRIES,
        "this zip has too many files ({})",
        ar.len()
    );
    let target = target_for(zip);
    let mut total = 0u64;
    let mut files = 0usize;
    let mut all_present = target.is_dir();
    for i in 0..ar.len() {
        let e = ar.by_index_raw(i)?;
        if e.is_dir() || is_symlink(&e) {
            continue;
        }
        let Some(rel) = e.enclosed_name() else {
            continue;
        };
        total = total.saturating_add(e.size());
        files += 1;
        if all_present {
            let out = target.join(&rel);
            all_present = std::fs::metadata(&out)
                .map(|m| m.len() == e.size())
                .unwrap_or(false);
        }
    }
    anyhow::ensure!(
        total <= MAX_TOTAL_BYTES,
        "this zip claims to be far too large to unpack safely"
    );
    let free = fs4::available_space(zip.parent().unwrap_or(Path::new("."))).ok();
    Ok(ZipPlan {
        zip: zip.to_path_buf(),
        target,
        files,
        unpacked_bytes: total,
        free_bytes: free,
        already_unpacked: all_present && files > 0,
    })
}

fn is_symlink(e: &zip::read::ZipFile<'_, File>) -> bool {
    e.unix_mode()
        .map(|m| m & 0o170000 == 0o120000)
        .unwrap_or(false)
}

/// Unpack a zip next to itself and return the folder. Safe to call again:
/// files that already exist with the right size are skipped.
pub fn unpack(zip: &Path, progress: &dyn Fn(UnzipProgress)) -> anyhow::Result<PathBuf> {
    let p = plan(zip)?;
    if p.already_unpacked {
        return Ok(p.target);
    }
    let needed = p.unpacked_bytes.saturating_add(SPACE_MARGIN);
    if let Some(free) = p.free_bytes {
        anyhow::ensure!(
            free >= needed,
            "not enough disk space to unzip: needs {}, only {} free",
            human(needed),
            human(free)
        );
    }
    std::fs::create_dir_all(&p.target)?;
    let mut ar = zip::ZipArchive::new(File::open(zip)?)?;
    let mut done = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    for i in 0..ar.len() {
        let mut e = ar.by_index(i)?;
        if is_symlink(&e) {
            continue;
        }
        // enclosed_name() rejects absolute paths and anything with `..`.
        let Some(rel) = e.enclosed_name() else {
            continue;
        };
        let out = p.target.join(&rel);
        anyhow::ensure!(
            crate::paths::is_within(&p.target, &out),
            "unsafe path in zip"
        );
        if e.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Ok(m) = std::fs::metadata(&out) {
            if m.len() == e.size() {
                done += e.size();
                continue; // already unpacked
            }
            anyhow::bail!("{} already exists — not overwriting it", out.display());
        }
        if let Some(d) = out.parent() {
            std::fs::create_dir_all(d)?;
        }
        let declared = e.size();
        let compressed = e.compressed_size().max(1);
        let limit = declared
            .min(compressed.saturating_mul(MAX_RATIO))
            .max(declared.min(1 << 20));
        anyhow::ensure!(
            declared <= compressed.saturating_mul(MAX_RATIO) || declared <= 1 << 20,
            "{} looks like a zip bomb",
            rel.display()
        );
        let part = out.with_extension(format!(
            "{}.part",
            out.extension().and_then(|x| x.to_str()).unwrap_or("")
        ));
        let mut w = std::io::BufWriter::new(File::create(&part)?);
        let mut written = 0u64;
        let name = rel.to_string_lossy().to_string();
        loop {
            let n = e.read(&mut buf)?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > limit {
                drop(w);
                let _ = std::fs::remove_file(&part);
                anyhow::bail!(
                    "{} unpacks to more than it says — refusing (possible zip bomb)",
                    name
                );
            }
            w.write_all(&buf[..n])?;
            done += n as u64;
            progress(UnzipProgress {
                zip: zip.to_path_buf(),
                done_bytes: done,
                total_bytes: p.unpacked_bytes,
                current: name.clone(),
            });
        }
        w.flush()?;
        drop(w);
        std::fs::rename(&part, &out)?;
    }
    Ok(p.target)
}

/// Replace any `.zip` roots by their unpacked folders.
pub fn prepare_roots(
    roots: &[PathBuf],
    progress: &dyn Fn(UnzipProgress),
) -> (Vec<PathBuf>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for r in roots {
        if is_zip(r) {
            match unpack(r, progress) {
                Ok(dir) => out.push(dir),
                Err(e) => errors.push(format!(
                    "{}: {e}",
                    r.file_name().and_then(|n| n.to_str()).unwrap_or("zip")
                )),
            }
        } else {
            out.push(r.clone());
        }
    }
    (out, errors)
}

pub fn human(b: u64) -> String {
    let gb = b as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else if b >= 1_048_576 {
        format!("{:.0} MB", b as f64 / 1_048_576.0)
    } else {
        format!("{} KB", b.div_ceil(1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in entries {
            z.start_file(
                *name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn unpacks_next_to_zip_and_is_repeatable() {
        let t = tempfile::tempdir().unwrap();
        let zp = t.path().join("My Games.zip");
        make_zip(
            &zp,
            &[
                ("My Games/snes/Super Mario World (USA).sfc", b"rom"),
                ("My Games/psx/readme.txt", b"hi"),
            ],
        );
        let dir = unpack(&zp, &|_| {}).unwrap();
        assert_eq!(dir, t.path().join("My Games"));
        assert_eq!(
            std::fs::read(dir.join("My Games/snes/Super Mario World (USA).sfc")).unwrap(),
            b"rom"
        );
        assert!(zp.exists(), "original zip kept");
        // Second time: nothing to do.
        assert!(plan(&zp).unwrap().already_unpacked);
        unpack(&zp, &|_| {}).unwrap();
        // And the scanner finds the games.
        let (roots, errs) = prepare_roots(&[zp], &|_| {});
        assert!(errs.is_empty());
        let lib = crate::scan::scan_roots(&roots);
        assert_eq!(lib.game_count(), 1);
    }

    #[test]
    fn refuses_zip_slip_and_keeps_existing_files() {
        let t = tempfile::tempdir().unwrap();
        let zp = t.path().join("evil.zip");
        make_zip(
            &zp,
            &[
                ("../escaped.txt", b"x"),
                ("/abs.txt", b"x"),
                ("ok/game.gba", b"x"),
            ],
        );
        let dir = unpack(&zp, &|_| {}).unwrap();
        assert!(!t.path().join("escaped.txt").exists());
        assert!(!Path::new("/abs.txt").exists() || std::fs::read("/abs.txt").unwrap() != b"x");
        assert!(dir.join("ok/game.gba").exists());

        let zp2 = t.path().join("clash.zip");
        std::fs::create_dir_all(t.path().join("clash")).unwrap();
        std::fs::write(t.path().join("clash/a.sfc"), b"mine!").unwrap();
        make_zip(&zp2, &[("a.sfc", b"theirs")]);
        assert!(unpack(&zp2, &|_| {}).is_err());
        assert_eq!(
            std::fs::read(t.path().join("clash/a.sfc")).unwrap(),
            b"mine!"
        );
    }

    #[test]
    fn refuses_zip_bombs() {
        let t = tempfile::tempdir().unwrap();
        let zp = t.path().join("bomb.zip");
        // 64 MB of zeros compresses to ~64 KB: ratio ~1000+.
        let zeros = vec![0u8; 64 * 1024 * 1024];
        make_zip(&zp, &[("big.bin", &zeros)]);
        let err = unpack(&zp, &|_| {}).unwrap_err().to_string();
        assert!(err.contains("zip bomb"), "{err}");
        assert!(!t.path().join("bomb/big.bin").exists());
    }

    #[test]
    fn rejects_non_zip() {
        let t = tempfile::tempdir().unwrap();
        let zp = t.path().join("fake.zip");
        std::fs::write(&zp, b"not a zip at all").unwrap();
        let (roots, errs) = prepare_roots(&[zp], &|_| {});
        assert!(roots.is_empty());
        assert_eq!(errs.len(), 1);
    }
}
