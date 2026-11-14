//! Optional ScreenScraper source for extra media (3D boxes, videos, manuals…).
//!
//! Runs in **guest mode** by default: the app identifies itself with its own
//! developer registration and the user does not need an account. A user who
//! already has an account can supply it for more speed; it is kept in memory
//! for the session only and never written to disk.

use crate::identify::Fingerprint;
use crate::net::{NetError, SafeClient};
use crate::scan::Game;
use crate::source::ArtKind;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use url::Url;

mod creds {
    include!(concat!(env!("OUT_DIR"), "/ss_creds.rs"));
}

fn unscramble(b: &[u8]) -> String {
    b.iter()
        .enumerate()
        .map(|(i, x)| (x ^ creds::SS_KEY[i % creds::SS_KEY.len()]) as char)
        .collect()
}

/// Whether this build of ShelfArt has ScreenScraper developer access.
pub fn available() -> bool {
    !creds::SS_DEVID.is_empty() && !creds::SS_DEVPASSWORD.is_empty()
}

/// ScreenScraper system ids (same values ES-DE uses).
pub fn system_id(id: &str) -> Option<u32> {
    Some(match id {
        "nes" => 3,
        "fds" => 106,
        "snes" => 4,
        "n64" => 14,
        "n64dd" => 14,
        "gb" => 9,
        "gbc" => 10,
        "gba" => 12,
        "nds" => 15,
        "3ds" => 17,
        "gc" => 13,
        "wii" => 16,
        "wiiu" => 18,
        "vb" => 11,
        "pokemini" => 211,
        "satellaview" => 107,
        "sufami" => 108,
        "sg1000" => 109,
        "mastersystem" => 2,
        "megadrive" => 1,
        "gamegear" => 21,
        "segacd" => 20,
        "sega32x" => 19,
        "saturn" => 22,
        "dreamcast" => 23,
        "naomi" => 56,
        "naomi2" => 56,
        "atomiswave" => 53,
        "pico" => 250,
        "psx" => 57,
        "ps2" => 58,
        "ps3" => 59,
        "psp" => 61,
        "psvita" => 62,
        "pcengine" => 31,
        "pcenginecd" => 114,
        "supergrafx" => 105,
        "pcfx" => 72,
        "pc98" => 208,
        "neogeo" => 142,
        "neogeocd" => 70,
        "ngp" => 25,
        "ngpc" => 82,
        "atari2600" => 26,
        "atari5200" => 40,
        "atari7800" => 41,
        "atari800" => 43,
        "lynx" => 28,
        "jaguar" => 27,
        "atarist" => 42,
        "c64" => 66,
        "vic20" => 73,
        "amiga" => 64,
        "amigacd32" => 130,
        "msx" => 113,
        "msx2" => 116,
        "zxspectrum" => 76,
        "amstradcpc" => 65,
        "x68000" => 79,
        "dos" => 135,
        "scummvm" => 123,
        "colecovision" => 48,
        "intellivision" => 115,
        "vectrex" => 102,
        "odyssey2" => 104,
        "channelf" => 80,
        "wonderswan" => 45,
        "wonderswancolor" => 46,
        "3do" => 29,
        "cdi" => 133,
        "supervision" => 207,
        "gamecom" => 121,
        "arduboy" => 263,
        "xbox" => 32,
        "fbneo" => 75,
        "mame" => 75,
        _ => return None,
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct Media {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SsGame {
    pub name: Option<String>,
    pub medias: Vec<Media>,
}

impl SsGame {
    /// Best media URL for a kind, honouring region preference.
    pub fn media_for(&self, kind: ArtKind, regions: &[String]) -> Option<&Media> {
        let mut order: Vec<String> = regions
            .iter()
            .map(|r| match r.as_str() {
                "usa" => "us".to_string(),
                "europe" => "eu".to_string(),
                "japan" => "jp".to_string(),
                "world" => "wor".to_string(),
                "australia" => "au".to_string(),
                other => other.to_string(),
            })
            .collect();
        order.extend(
            ["wor", "us", "eu", "ss", "jp", "cus"]
                .iter()
                .map(|s| s.to_string()),
        );
        for t in kind.screenscraper_types() {
            let of_type: Vec<&Media> = self.medias.iter().filter(|m| m.kind == *t).collect();
            if of_type.is_empty() {
                continue;
            }
            for r in &order {
                if let Some(m) = of_type
                    .iter()
                    .find(|m| m.region.as_deref() == Some(r.as_str()))
                {
                    return Some(m);
                }
            }
            return of_type.first().copied();
        }
        None
    }
}

#[derive(Clone)]
pub struct ScreenScraper {
    client: SafeClient,
    user: Option<(String, String)>,
    /// Guest mode allows a single request at a time.
    slots: Arc<Semaphore>,
    /// Set when the service tells us to stop (closed for guests, quota reached…).
    stopped: Arc<AtomicBool>,
    stop_reason: Arc<std::sync::Mutex<Option<String>>>,
}

#[derive(Deserialize)]
struct Resp {
    response: RespInner,
}
#[derive(Deserialize)]
struct RespInner {
    jeu: Option<Jeu>,
}
#[derive(Deserialize)]
struct Jeu {
    #[serde(default)]
    noms: Vec<Nom>,
    #[serde(default)]
    medias: Vec<Media>,
}
#[derive(Deserialize)]
struct Nom {
    #[serde(default)]
    text: String,
}

impl ScreenScraper {
    pub fn new(client: SafeClient, user: Option<(String, String)>) -> Option<Self> {
        if !available() {
            return None;
        }
        let threads = if user.is_some() { 2 } else { 1 };
        Some(Self {
            client,
            user,
            slots: Arc::new(Semaphore::new(threads)),
            stopped: Arc::new(AtomicBool::new(false)),
            stop_reason: Arc::new(std::sync::Mutex::new(None)),
        })
    }

    pub fn stopped_reason(&self) -> Option<String> {
        self.stop_reason.lock().ok().and_then(|g| g.clone())
    }

    fn stop(&self, why: &str) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Ok(mut g) = self.stop_reason.lock() {
            if g.is_none() {
                *g = Some(why.to_string());
            }
        }
    }

    fn base_url(&self, endpoint: &str) -> anyhow::Result<Url> {
        let mut url = Url::parse(&format!("https://api.screenscraper.fr/api2/{endpoint}"))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("devid", &unscramble(creds::SS_DEVID));
            q.append_pair("devpassword", &unscramble(creds::SS_DEVPASSWORD));
            q.append_pair("softname", concat!("ShelfArt-", env!("CARGO_PKG_VERSION")));
            q.append_pair("output", "json");
            if let Some((u, p)) = &self.user {
                q.append_pair("ssid", u);
                q.append_pair("sspassword", p);
            }
        }
        Ok(url)
    }

    /// Look a game up. Returns Ok(None) when ScreenScraper doesn't know it.
    pub async fn game(
        &self,
        sys_id: &str,
        game: &Game,
        fp: &Fingerprint,
        canonical: Option<&str>,
    ) -> anyhow::Result<Option<SsGame>> {
        if self.stopped.load(Ordering::Relaxed) {
            anyhow::bail!(self
                .stopped_reason()
                .unwrap_or_else(|| "ScreenScraper unavailable".into()));
        }
        let Some(system) = system_id(sys_id) else {
            return Ok(None);
        };
        let mut url = self.base_url("jeuInfos.php")?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("systemeid", &system.to_string());
            q.append_pair("romtype", "rom");
            let romname = match canonical {
                Some(c) => format!(
                    "{c}.{}",
                    game.path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("zip")
                ),
                None => game.file_name.clone(),
            };
            q.append_pair("romnom", &romname);
            if let Some(s) = fp.size {
                q.append_pair("romtaille", &s.to_string());
            }
            if let Some(c) = fp.crc_headerless.or(fp.crc) {
                q.append_pair("crc", &format!("{c:08X}"));
            }
            if let Some(s) = &fp.serial {
                q.append_pair("serialnum", s);
            }
        }
        let _permit = self.slots.acquire().await?;
        let body = match self.client.get(&url, 8 * 1024 * 1024).await {
            Ok(b) => b,
            Err(NetError::NotFound) => return Ok(None),
            Err(NetError::RateLimited) => {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                return Err(anyhow::anyhow!("ScreenScraper is busy, skipped"));
            }
            Err(NetError::Status(code)) => {
                let why = match code {
                    401 => "ScreenScraper is currently closed to guests (busy hours). Try later, or add your own ScreenScraper login in Settings.",
                    403 => "ScreenScraper rejected this app's registration.",
                    423 => "ScreenScraper's API is closed right now.",
                    426 => "This ShelfArt version is too old for ScreenScraper. Please update.",
                    430 => "Daily ScreenScraper quota reached. Extras will resume tomorrow.",
                    431 => "Too many unrecognised games for ScreenScraper today.",
                    _ => "ScreenScraper error.",
                };
                if matches!(code, 401 | 403 | 423 | 426 | 430 | 431) {
                    self.stop(why);
                }
                anyhow::bail!(why)
            }
            Err(e) => return Err(e.into()),
        };
        // Errors come back as plain text.
        let parsed: Resp = match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(_) => {
                let text = String::from_utf8_lossy(&body);
                if text.contains("non trouv") {
                    return Ok(None);
                }
                anyhow::bail!(
                    "ScreenScraper: {}",
                    text.chars().take(120).collect::<String>()
                )
            }
        };
        Ok(parsed.response.jeu.map(|j| SsGame {
            name: j.noms.into_iter().map(|n| n.text).find(|t| !t.is_empty()),
            medias: j
                .medias
                .into_iter()
                .filter(|m| m.url.starts_with("https://"))
                .collect(),
        }))
    }

    /// Download a media file (URL must be on screenscraper.fr; enforced by the client).
    pub async fn download(&self, media: &Media, max: usize) -> anyhow::Result<Vec<u8>> {
        let url = Url::parse(&media.url)?;
        let _permit = self.slots.acquire().await?;
        Ok(self.client.get(&url, max).await?)
    }
}

/// Basic content checks for non-image media.
pub fn validate_media(kind: ArtKind, bytes: &[u8]) -> anyhow::Result<()> {
    match kind {
        ArtKind::Video => anyhow::ensure!(
            bytes.len() > 12 && &bytes[4..8] == b"ftyp",
            "not an MP4 video"
        ),
        ArtKind::Manual => anyhow::ensure!(bytes.starts_with(b"%PDF-"), "not a PDF"),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_choice() {
        let g = SsGame {
            name: None,
            medias: vec![
                Media {
                    kind: "box-2D".into(),
                    url: "https://x.screenscraper.fr/jp".into(),
                    region: Some("jp".into()),
                    format: None,
                },
                Media {
                    kind: "box-2D".into(),
                    url: "https://x.screenscraper.fr/eu".into(),
                    region: Some("eu".into()),
                    format: None,
                },
                Media {
                    kind: "wheel".into(),
                    url: "https://x.screenscraper.fr/w".into(),
                    region: Some("wor".into()),
                    format: None,
                },
            ],
        };
        let r = vec!["usa".to_string(), "europe".to_string()];
        assert!(g
            .media_for(ArtKind::Boxart, &r)
            .unwrap()
            .url
            .ends_with("/eu"));
        assert!(g.media_for(ArtKind::Logo, &r).unwrap().url.ends_with("/w"));
        assert!(g.media_for(ArtKind::Video, &r).is_none());
    }

    #[test]
    fn media_validation() {
        assert!(validate_media(ArtKind::Video, b"\0\0\0\x18ftypmp42xxxx").is_ok());
        assert!(validate_media(ArtKind::Video, b"<html>").is_err());
        assert!(validate_media(ArtKind::Manual, b"%PDF-1.4").is_ok());
    }
}
