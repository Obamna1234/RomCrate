//! The artwork source: the public libretro-thumbnails collection.
//!
//! No account, no API key. Artwork lists come from the libretro thumbnail
//! server (falling back to GitHub's public API), images come from GitHub's
//! raw CDN (falling back to the libretro server). Everything is cached locally
//! so re-runs are fast and work offline for art you already have.

use crate::matcher::NameIndex;
use crate::net::{NetError, SafeClient, MAX_IMAGE_BYTES, MAX_INDEX_BYTES};
use crate::paths::safe_component;
use crate::systems::System;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use url::Url;

/// Kinds of artwork RomCrate can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtKind {
    /// Front box / cover art (2D).
    Boxart,
    /// 3D box render.
    Box3d,
    /// Back cover.
    Boxback,
    /// Cartridge / disc / physical media.
    Cartridge,
    /// In-game screenshot.
    Snap,
    /// Title screen.
    Title,
    /// Clear logo / wheel / marquee.
    Logo,
    /// Fan art / background.
    Fanart,
    /// Composite built by RomCrate: screenshot + box + logo.
    Mix,
    /// Short gameplay video (mp4).
    Video,
    /// Game manual (pdf).
    Manual,
}

impl ArtKind {
    pub const ALL: [ArtKind; 11] = [
        ArtKind::Boxart,
        ArtKind::Box3d,
        ArtKind::Boxback,
        ArtKind::Cartridge,
        ArtKind::Snap,
        ArtKind::Title,
        ArtKind::Logo,
        ArtKind::Fanart,
        ArtKind::Mix,
        ArtKind::Video,
        ArtKind::Manual,
    ];
    /// Kinds available from the free libretro collection.
    pub const DOWNLOADABLE: [ArtKind; 4] = [
        ArtKind::Boxart,
        ArtKind::Snap,
        ArtKind::Title,
        ArtKind::Logo,
    ];

    pub fn libretro_dir(self) -> Option<&'static str> {
        match self {
            ArtKind::Boxart => Some("Named_Boxarts"),
            ArtKind::Snap => Some("Named_Snaps"),
            ArtKind::Title => Some("Named_Titles"),
            ArtKind::Logo => Some("Named_Logos"),
            _ => None,
        }
    }

    /// ScreenScraper media types, best first.
    pub fn screenscraper_types(self) -> &'static [&'static str] {
        match self {
            ArtKind::Boxart => &["box-2D"],
            ArtKind::Box3d => &["box-3D"],
            ArtKind::Boxback => &["box-2D-back"],
            ArtKind::Cartridge => &["support-2D"],
            ArtKind::Snap => &["ss"],
            ArtKind::Title => &["sstitle"],
            ArtKind::Logo => &["wheel-hd", "wheel"],
            ArtKind::Fanart => &["fanart"],
            ArtKind::Video => &["video-normalized", "video"],
            ArtKind::Manual => &["manuel"],
            ArtKind::Mix => &[],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArtKind::Boxart => "Box art (2D)",
            ArtKind::Box3d => "3D box",
            ArtKind::Boxback => "Back cover",
            ArtKind::Cartridge => "Cartridge / disc",
            ArtKind::Snap => "Screenshot",
            ArtKind::Title => "Title screen",
            ArtKind::Logo => "Wheel / logo",
            ArtKind::Fanart => "Fan art",
            ArtKind::Mix => "Mix image",
            ArtKind::Video => "Video snap",
            ArtKind::Manual => "Manual",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            ArtKind::Boxart => "boxart",
            ArtKind::Box3d => "box3d",
            ArtKind::Boxback => "boxback",
            ArtKind::Cartridge => "cartridge",
            ArtKind::Snap => "snap",
            ArtKind::Title => "title",
            ArtKind::Logo => "logo",
            ArtKind::Fanart => "fanart",
            ArtKind::Mix => "mix",
            ArtKind::Video => "video",
            ArtKind::Manual => "manual",
        }
    }

    /// File extension of the saved file.
    pub fn ext(self) -> &'static str {
        match self {
            ArtKind::Video => "mp4",
            ArtKind::Manual => "pdf",
            _ => "png",
        }
    }

    pub fn is_image(self) -> bool {
        !matches!(self, ArtKind::Video | ArtKind::Manual)
    }

    /// Screenshots and title screens are pixel art and need different upscaling.
    pub fn is_pixel_art(self) -> bool {
        matches!(self, ArtKind::Snap | ArtKind::Title)
    }
}

/// How long a downloaded artwork list stays fresh.
const INDEX_TTL_SECS: i64 = 7 * 24 * 3600;

#[derive(Debug, Serialize, Deserialize, Default)]
struct IndexCache {
    fetched_at: i64,
    kinds: BTreeMap<ArtKind, Vec<String>>,
}

/// All artwork names available for one system.
pub struct SystemArt {
    pub all: NameIndex,
    pub per_kind: HashMap<ArtKind, NameIndex>,
}

impl SystemArt {
    pub fn from_names(kinds: BTreeMap<ArtKind, Vec<String>>) -> Self {
        let all = NameIndex::new(kinds.values().flatten().cloned());
        let per_kind = kinds
            .into_iter()
            .map(|(k, v)| (k, NameIndex::new(v)))
            .collect();
        Self { all, per_kind }
    }

    pub fn total(&self) -> usize {
        self.all.len()
    }
}

pub struct Libretro {
    client: SafeClient,
    cache_dir: PathBuf,
}

pub fn default_cache_dir() -> PathBuf {
    let dir = crate::datadir::cache_dir();
    // Carry over downloads from when the app was called ShelfArt (saves re-downloading).
    if dir == crate::datadir::default_cache_dir() {
        let old = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("ShelfArt");
        if !dir.exists() && old.is_dir() {
            let _ = std::fs::rename(&old, &dir);
        }
    }
    dir
}

impl Libretro {
    pub fn new(client: SafeClient, cache_dir: PathBuf) -> Self {
        Self { client, cache_dir }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    fn index_path(&self, sys: &System) -> PathBuf {
        self.cache_dir
            .join("index")
            .join(format!("{}.json", sys.repo))
    }

    /// Load the artwork list for a system, from cache when fresh.
    pub async fn system_art(&self, sys: &System, force_refresh: bool) -> anyhow::Result<SystemArt> {
        let path = self.index_path(sys);
        let now = chrono::Utc::now().timestamp();
        let cached: Option<IndexCache> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        if let Some(c) = &cached {
            if !force_refresh && now - c.fetched_at < INDEX_TTL_SECS && !c.kinds.is_empty() {
                return Ok(SystemArt::from_names(c.kinds.clone()));
            }
        }
        match self.fetch_index(sys).await {
            Ok(kinds) => {
                let cache = IndexCache {
                    fetched_at: now,
                    kinds,
                };
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&path, serde_json::to_vec(&cache)?);
                Ok(SystemArt::from_names(cache.kinds))
            }
            // Offline or rate-limited: fall back to a stale cache if we have one.
            Err(e) => match cached {
                Some(c) if !c.kinds.is_empty() => Ok(SystemArt::from_names(c.kinds)),
                _ => Err(e),
            },
        }
    }

    async fn fetch_index(&self, sys: &System) -> anyhow::Result<BTreeMap<ArtKind, Vec<String>>> {
        match self.fetch_index_libretro(sys).await {
            Ok(k) if k.values().any(|v| !v.is_empty()) => Ok(k),
            _ => self.fetch_index_github(sys).await,
        }
    }

    /// Directory listings from thumbnails.libretro.com.
    async fn fetch_index_libretro(
        &self,
        sys: &System,
    ) -> anyhow::Result<BTreeMap<ArtKind, Vec<String>>> {
        let mut out = BTreeMap::new();
        for kind in ArtKind::DOWNLOADABLE {
            let dir = kind.libretro_dir().unwrap();
            let mut url = Url::parse("https://thumbnails.libretro.com/")?;
            {
                let mut seg = url
                    .path_segments_mut()
                    .map_err(|_| anyhow::anyhow!("bad url"))?;
                seg.pop_if_empty().push(sys.libretro).push(dir).push("");
            }
            match self.client.get(&url, MAX_INDEX_BYTES).await {
                Ok(body) => {
                    out.insert(kind, parse_listing(&String::from_utf8_lossy(&body)));
                }
                Err(NetError::NotFound) => {
                    out.insert(kind, Vec::new());
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(out)
    }

    /// Git tree listing from the public GitHub API (60 requests/hour without a token).
    async fn fetch_index_github(
        &self,
        sys: &System,
    ) -> anyhow::Result<BTreeMap<ArtKind, Vec<String>>> {
        #[derive(Deserialize)]
        struct Tree {
            tree: Vec<Entry>,
            #[serde(default)]
            truncated: bool,
        }
        #[derive(Deserialize)]
        struct Entry {
            path: String,
            #[serde(rename = "type")]
            kind: String,
            sha: String,
        }
        let api = |path: &str| {
            Url::parse(&format!(
                "https://api.github.com/repos/libretro-thumbnails/{}/git/trees/{}",
                sys.repo, path
            ))
        };
        let body = self
            .client
            .get(&api("HEAD?recursive=1")?, MAX_INDEX_BYTES)
            .await?;
        let tree: Tree = serde_json::from_slice(&body)?;
        let mut out: BTreeMap<ArtKind, Vec<String>> = BTreeMap::new();
        if !tree.truncated {
            for e in &tree.tree {
                if e.kind == "tree" {
                    continue;
                }
                if let Some((dir, file)) = e.path.split_once('/') {
                    if let Some(kind) = ArtKind::DOWNLOADABLE
                        .into_iter()
                        .find(|k| k.libretro_dir() == Some(dir))
                    {
                        if let Some(name) = file.strip_suffix(".png") {
                            if !name.contains('/') {
                                out.entry(kind).or_default().push(name.to_string());
                            }
                        }
                    }
                }
            }
            return Ok(out);
        }
        // Huge repo: list each art folder separately.
        let top = self.client.get(&api("HEAD")?, MAX_INDEX_BYTES).await?;
        let top: Tree = serde_json::from_slice(&top)?;
        for kind in ArtKind::DOWNLOADABLE {
            let dir = kind.libretro_dir().unwrap();
            if let Some(e) = top.tree.iter().find(|e| e.path == dir && e.kind == "tree") {
                let body = self.client.get(&api(&e.sha)?, MAX_INDEX_BYTES).await?;
                let sub: Tree = serde_json::from_slice(&body)?;
                out.insert(
                    kind,
                    sub.tree
                        .into_iter()
                        .filter_map(|e| e.path.strip_suffix(".png").map(String::from))
                        .collect(),
                );
            }
        }
        Ok(out)
    }

    fn art_cache_path(&self, sys: &System, kind: ArtKind, name: &str) -> PathBuf {
        self.cache_dir
            .join("art")
            .join(sys.repo)
            .join(kind.libretro_dir().unwrap_or("Other"))
            .join(format!("{}.png", safe_component(name)))
    }

    /// Download (or load from cache) one piece of art. Returns raw image bytes
    /// which the caller must still validate by decoding.
    pub async fn fetch_art(
        &self,
        sys: &System,
        kind: ArtKind,
        name: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let dir = kind
            .libretro_dir()
            .ok_or_else(|| anyhow::anyhow!("{kind:?} is generated, not downloaded"))?;
        let cache = self.art_cache_path(sys, kind, name);
        if let Ok(b) = std::fs::read(&cache) {
            if crate::imaging::looks_like_image(&b) {
                return Ok(b);
            }
        }
        let bytes = match self.fetch_from_github(sys, dir, name).await {
            Ok(b) => b,
            Err(_) => self.fetch_from_libretro(sys, dir, name).await?,
        };
        if let Some(p) = cache.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(&cache, &bytes);
        Ok(bytes)
    }

    async fn fetch_from_libretro(
        &self,
        sys: &System,
        dir: &str,
        name: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let mut url = Url::parse("https://thumbnails.libretro.com/")?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("bad url"))?
            .pop_if_empty()
            .push(sys.libretro)
            .push(dir)
            .push(&format!("{name}.png"));
        let b = self.client.get(&url, MAX_IMAGE_BYTES).await?;
        anyhow::ensure!(
            crate::imaging::looks_like_image(&b),
            "server returned something that is not an image"
        );
        Ok(b)
    }

    /// GitHub stores duplicate art as git symlinks; the raw CDN returns the link
    /// target as text, so we follow it (staying inside the repository).
    async fn fetch_from_github(
        &self,
        sys: &System,
        dir: &str,
        name: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let mut rel: Vec<String> = vec![dir.to_string(), format!("{name}.png")];
        for _hop in 0..4 {
            let mut url = Url::parse(&format!(
                "https://raw.githubusercontent.com/libretro-thumbnails/{}/HEAD/",
                sys.repo
            ))?;
            {
                let mut seg = url
                    .path_segments_mut()
                    .map_err(|_| anyhow::anyhow!("bad url"))?;
                seg.pop_if_empty();
                for r in &rel {
                    seg.push(r);
                }
            }
            let b = self.client.get(&url, MAX_IMAGE_BYTES).await?;
            if crate::imaging::looks_like_image(&b) {
                return Ok(b);
            }
            // Symlink?
            let target = std::str::from_utf8(&b)
                .ok()
                .filter(|t| b.len() < 1024 && t.ends_with(".png"));
            let Some(target) = target else {
                anyhow::bail!("not an image")
            };
            let mut next: Vec<String> = rel[..rel.len() - 1].to_vec();
            for part in target.trim().split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        anyhow::ensure!(next.pop().is_some(), "symlink escapes repository");
                    }
                    p => next.push(p.to_string()),
                }
            }
            anyhow::ensure!(!next.is_empty(), "bad symlink");
            rel = next;
        }
        anyhow::bail!("too many symlinks")
    }
}

/// Extract `.png` file names from an HTML directory listing.
pub fn parse_listing(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=\"") {
        rest = &rest[i + 6..];
        let Some(end) = rest.find('"') else { break };
        let href = &rest[..end];
        rest = &rest[end..];
        let decoded = percent_encoding::percent_decode_str(href).decode_utf8_lossy();
        let decoded = decoded.replace("&amp;", "&");
        if decoded.contains('/') || decoded.starts_with('?') {
            continue;
        }
        if let Some(name) = decoded.strip_suffix(".png") {
            out.push(name.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_listing() {
        let html = r#"<a href="../">../</a><a href="Super%20Mario%20World%20%28USA%29.png">Super…</a>
            <a href="Tom%20_%20Jerry.png">x</a><a href="?C=M">sort</a><a href="sub/">sub/</a>"#;
        assert_eq!(
            parse_listing(html),
            vec!["Super Mario World (USA)", "Tom _ Jerry"]
        );
    }
}
