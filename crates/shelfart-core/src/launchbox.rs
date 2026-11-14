//! The LaunchBox Games Database: a free, community-maintained database that
//! LaunchBox publishes as a single daily download (no account, no API key).
//!
//! It gives ShelfArt game information (descriptions, genre, developer,
//! publisher, release date, players, rating) and extra image types the
//! libretro collection doesn't have: 3D boxes, back covers, cartridges,
//! clear logos and fan art.
//!
//! The full download is large, so it's fetched at most once a week, streamed
//! to disk, and parsed once per platform into a small local index.

use crate::matcher::{display_title, MatchPrefs, NameIndex};
use crate::net::SafeClient;
use crate::paths::safe_component;
use crate::source::ArtKind;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use url::Url;

pub const DB_URL: &str = "https://gamesdb.launchbox-app.com/Metadata.zip";
pub const IMAGES_URL: &str = "https://images.launchbox-app.com/";
/// The database is a few hundred MB; refuse anything absurd.
const MAX_DB_BYTES: usize = 1024 * 1024 * 1024;
const DB_TTL_SECS: u64 = 7 * 24 * 3600;
/// Bump when the on-disk index format changes.
const INDEX_VERSION: u32 = 1;

/// Information about a game, written into launcher files.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GameInfo {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// YYYY-MM-DD (or YYYY).
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub developer: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub players: Option<String>,
    /// 0.0 – 1.0
    #[serde(default)]
    pub rating: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LbGame {
    id: u64,
    info: GameInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LbImage {
    file: String,
    kind: String,
    #[serde(default)]
    region: Option<String>,
}

/// Everything ShelfArt needs from the database for one platform.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlatformDb {
    version: u32,
    /// Modification time of the Metadata.zip this was built from.
    source_mtime: u64,
    games: Vec<LbGame>,
    /// Alternate names → game id.
    alternates: Vec<(String, u64)>,
    /// Lower-case known ROM file names (without extension) → game title.
    files: HashMap<String, String>,
    images: HashMap<u64, Vec<LbImage>>,
    #[serde(skip)]
    index: Option<(NameIndex, HashMap<String, u64>)>,
}

/// LaunchBox image types for each art kind, best first.
fn image_types(kind: ArtKind) -> &'static [&'static str] {
    match kind {
        ArtKind::Boxart => &["Box - Front", "Box - Front - Reconstructed"],
        ArtKind::Box3d => &["Box - 3D"],
        ArtKind::Boxback => &["Box - Back", "Box - Back - Reconstructed"],
        ArtKind::Cartridge => &["Cart - Front", "Disc", "Cart - 3D"],
        ArtKind::Snap => &["Screenshot - Gameplay"],
        ArtKind::Title => &["Screenshot - Game Title"],
        ArtKind::Logo => &["Clear Logo"],
        ArtKind::Fanart => &["Fanart - Background"],
        ArtKind::Mix | ArtKind::Video | ArtKind::Manual => &[],
    }
}

fn wanted_image_type(t: &str) -> bool {
    ArtKind::ALL.iter().any(|k| image_types(*k).contains(&t))
}

/// LaunchBox region names for our region preference words.
fn lb_region(pref: &str) -> &'static [&'static str] {
    match pref {
        "usa" => &["North America", "United States"],
        "europe" => &["Europe"],
        "world" => &["World"],
        "japan" => &["Japan"],
        "australia" => &["Australia", "Oceania"],
        "uk" => &["United Kingdom"],
        "france" => &["France"],
        "germany" => &["Germany"],
        "spain" => &["Spain"],
        "italy" => &["Italy"],
        "korea" => &["Korea"],
        "brazil" => &["Brazil"],
        _ => &[],
    }
}

impl PlatformDb {
    /// Build the lookup index (call once after loading, before `find`).
    pub fn prepare(&mut self) {
        self.build_index();
    }

    fn build_index(&mut self) {
        if self.index.is_some() {
            return;
        }
        let mut by_name: HashMap<String, u64> = HashMap::new();
        let mut names = Vec::new();
        for g in &self.games {
            by_name.entry(g.info.title.clone()).or_insert(g.id);
            names.push(g.info.title.clone());
        }
        for (n, id) in &self.alternates {
            if !by_name.contains_key(n) {
                by_name.insert(n.clone(), *id);
                names.push(n.clone());
            }
        }
        self.index = Some((NameIndex::new(names), by_name));
    }

    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    fn game(&self, id: u64) -> Option<&LbGame> {
        self.games.iter().find(|g| g.id == id)
    }

    /// Find a game from any of the names we know for it (official database
    /// name, file name, header title), most reliable first.
    pub fn find(&self, names: &[&str], prefs: &MatchPrefs) -> Option<u64> {
        let (index, by_name) = self.index.as_ref()?;
        // 1. Exact known file name (LaunchBox maps No-Intro/Redump names to games).
        for n in names {
            if let Some(title) = self.files.get(&n.to_lowercase()) {
                if let Some(id) = by_name.get(title) {
                    return Some(*id);
                }
            }
        }
        // 2. Title match (punctuation, articles and region tags ignored).
        let strict = MatchPrefs {
            fuzzy: false,
            ..prefs.clone()
        };
        for n in names {
            if let Some(m) = index.best_match(&display_title(n), &strict) {
                return by_name.get(&m.name).copied();
            }
        }
        // 3. Careful fuzzy match.
        for n in names {
            if let Some(m) = index.best_match(&display_title(n), prefs) {
                return by_name.get(&m.name).copied();
            }
        }
        None
    }

    pub fn info(&self, id: u64) -> Option<GameInfo> {
        self.game(id).map(|g| g.info.clone())
    }

    /// Best image URL for an art kind, honouring region preference.
    pub fn image_url(&self, id: u64, kind: ArtKind, regions: &[String]) -> Option<Url> {
        let imgs = self.images.get(&id)?;
        for t in image_types(kind) {
            let of_type: Vec<&LbImage> = imgs.iter().filter(|i| i.kind == *t).collect();
            if of_type.is_empty() {
                continue;
            }
            let pick = regions
                .iter()
                .find_map(|r| {
                    of_type.iter().find(|i| {
                        i.region
                            .as_deref()
                            .map(|ir| lb_region(r).contains(&ir))
                            .unwrap_or(false)
                    })
                })
                .or_else(|| of_type.iter().find(|i| i.region.is_none()))
                .or_else(|| of_type.first())?;
            // File names are GUIDs; refuse anything that could change the path.
            if pick.file.contains(['/', '\\']) || pick.file.contains("..") {
                return None;
            }
            let mut url = Url::parse(IMAGES_URL).ok()?;
            url.path_segments_mut()
                .ok()?
                .pop_if_empty()
                .push(&pick.file);
            return Some(url);
        }
        None
    }
}

pub struct LaunchBox {
    client: SafeClient,
    dir: PathBuf,
}

/// The parts of [`LaunchBox`] needed for parsing on a worker thread.
pub struct LaunchBoxFiles {
    dir: PathBuf,
}

impl LaunchBox {
    pub fn dir_for_blocking(&self) -> LaunchBoxFiles {
        LaunchBoxFiles {
            dir: self.dir.clone(),
        }
    }

    pub fn new(client: SafeClient, cache_dir: &Path) -> Self {
        Self {
            client,
            dir: cache_dir.join("launchbox"),
        }
    }

    fn zip_path(&self) -> PathBuf {
        self.dir.join("Metadata.zip")
    }

    /// Make sure a recent copy of the database is on disk. Returns its path.
    /// `progress` gets (downloaded, total) bytes.
    pub async fn ensure_database(
        &self,
        refresh: bool,
        progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    ) -> anyhow::Result<PathBuf> {
        let path = self.zip_path();
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| {
                t.elapsed()
                    .map(|e| e.as_secs() < DB_TTL_SECS)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if fresh && !refresh {
            return Ok(path);
        }
        std::fs::create_dir_all(&self.dir)?;
        let url = Url::parse(DB_URL)?;
        match self
            .client
            .download_to_file(&url, &path, MAX_DB_BYTES, progress)
            .await
        {
            Ok(()) => {
                // Index files built from the old download are now stale.
                let _ = std::fs::remove_dir_all(self.dir.join("platforms"));
                Ok(path)
            }
            Err(e) if path.exists() => {
                let _ = e;
                Ok(path) // offline: keep using the older copy
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Load (or build) the index for some LaunchBox platform names.
    pub fn platforms(&self, names: &[&str]) -> anyhow::Result<HashMap<String, PlatformDb>> {
        self.dir_for_blocking().platforms(names)
    }
    /// Download an image (cached), returning raw bytes for validation.
    pub async fn fetch_image(&self, url: &Url) -> anyhow::Result<Vec<u8>> {
        let name = url
            .path_segments()
            .and_then(|mut s| s.next_back())
            .unwrap_or("image");
        let cache = self.dir.join("images").join(safe_component(name));
        if let Ok(b) = std::fs::read(&cache) {
            if crate::imaging::looks_like_image(&b) {
                return Ok(b);
            }
        }
        let b = self.client.get(url, crate::net::MAX_IMAGE_BYTES).await?;
        anyhow::ensure!(crate::imaging::looks_like_image(&b), "not an image");
        if let Some(d) = cache.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&cache, &b);
        Ok(b)
    }

    /// Path of the cached image for a URL (used by tests to pre-seed the cache).
    pub fn image_cache_path(&self, file: &str) -> PathBuf {
        self.dir.join("images").join(safe_component(file))
    }
}

impl LaunchBoxFiles {
    fn zip_path(&self) -> PathBuf {
        self.dir.join("Metadata.zip")
    }

    fn platform_path(&self, platform: &str) -> PathBuf {
        self.dir
            .join("platforms")
            .join(format!("{}.json", safe_component(platform)))
    }

    /// Load (or build) the index for some LaunchBox platform names.
    /// Building parses the big download once for all requested platforms.
    pub fn platforms(&self, names: &[&str]) -> anyhow::Result<HashMap<String, PlatformDb>> {
        let zip = self.zip_path();
        let mtime = std::fs::metadata(&zip)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut out = HashMap::new();
        let mut missing = Vec::new();
        for n in names {
            let p = self.platform_path(n);
            let cached: Option<PlatformDb> = std::fs::read(&p)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            match cached {
                Some(db) if db.version == INDEX_VERSION && db.source_mtime == mtime => {
                    out.insert(n.to_string(), db);
                }
                _ => missing.push(n.to_string()),
            }
        }
        if !missing.is_empty() {
            let built = parse_database(&zip, &missing)?;
            std::fs::create_dir_all(self.dir.join("platforms"))?;
            for (name, mut db) in built {
                db.version = INDEX_VERSION;
                db.source_mtime = mtime;
                let _ = std::fs::write(self.platform_path(&name), serde_json::to_vec(&db)?);
                out.insert(name, db);
            }
        }
        Ok(out)
    }
}

/// Stream-parse Metadata.xml and Files.xml from the zip, keeping only the
/// requested platforms. Memory stays small even though the files are huge.
pub fn parse_database(
    zip_path: &Path,
    platforms: &[String],
) -> anyhow::Result<HashMap<String, PlatformDb>> {
    let wanted: HashSet<&str> = platforms.iter().map(|s| s.as_str()).collect();
    let mut dbs: HashMap<String, PlatformDb> = platforms
        .iter()
        .map(|p| (p.clone(), PlatformDb::default()))
        .collect();
    let mut id_platform: HashMap<u64, String> = HashMap::new();
    // Images and alternate names may refer to games we haven't seen yet.
    let mut pending_images: Vec<(u64, LbImage)> = Vec::new();
    let mut pending_alts: Vec<(u64, String)> = Vec::new();

    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?)?;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        let file_name = name.rsplit('/').next().unwrap_or(&name).to_string();
        if file_name != "Metadata.xml" && file_name != "Files.xml" {
            continue;
        }
        let mut reader = quick_xml::Reader::from_reader(BufReader::with_capacity(1 << 20, entry));
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut depth = 0usize;
        let mut record: Option<String> = None;
        let mut field: Option<String> = None;
        let mut values: HashMap<String, String> = HashMap::new();
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(e) => {
                    depth += 1;
                    let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                    if depth == 2 {
                        record = Some(tag);
                        values.clear();
                    } else if depth == 3 {
                        field = Some(tag);
                    }
                }
                Event::Text(t) => {
                    if let (Some(f), true) = (&field, depth == 3) {
                        let v = t.unescape().map(|c| c.to_string()).unwrap_or_default();
                        values.entry(f.clone()).or_default().push_str(&v);
                    }
                }
                Event::CData(t) => {
                    if let (Some(f), true) = (&field, depth == 3) {
                        values
                            .entry(f.clone())
                            .or_default()
                            .push_str(&String::from_utf8_lossy(&t));
                    }
                }
                Event::End(_) => {
                    if depth == 3 {
                        field = None;
                    } else if depth == 2 {
                        if let Some(r) = record.take() {
                            handle_record(
                                &r,
                                &values,
                                &wanted,
                                &mut dbs,
                                &mut id_platform,
                                &mut pending_images,
                                &mut pending_alts,
                            );
                        }
                    }
                    depth = depth.saturating_sub(1);
                }
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }
    }
    for (id, img) in pending_images {
        if let Some(p) = id_platform.get(&id) {
            dbs.get_mut(p)
                .unwrap()
                .images
                .entry(id)
                .or_default()
                .push(img);
        }
    }
    for (id, alt) in pending_alts {
        if let Some(p) = id_platform.get(&id) {
            dbs.get_mut(p).unwrap().alternates.push((alt, id));
        }
    }
    Ok(dbs)
}

fn clean(v: Option<&String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[allow(clippy::too_many_arguments)]
fn handle_record(
    record: &str,
    v: &HashMap<String, String>,
    wanted: &HashSet<&str>,
    dbs: &mut HashMap<String, PlatformDb>,
    id_platform: &mut HashMap<u64, String>,
    pending_images: &mut Vec<(u64, LbImage)>,
    pending_alts: &mut Vec<(u64, String)>,
) {
    let id = || {
        v.get("DatabaseID")
            .and_then(|s| s.trim().parse::<u64>().ok())
    };
    match record {
        "Game" => {
            let Some(platform) = clean(v.get("Platform")) else {
                return;
            };
            if !wanted.contains(platform.as_str()) {
                return;
            }
            let (Some(id), Some(title)) = (id(), clean(v.get("Name"))) else {
                return;
            };
            let rating = v
                .get("CommunityRating")
                .and_then(|r| r.trim().parse::<f32>().ok())
                .map(|r| (r / 5.0).clamp(0.0, 1.0));
            let release_date = clean(v.get("ReleaseDate"))
                .map(|d| d.chars().take(10).collect())
                .or_else(|| clean(v.get("ReleaseYear")));
            let genre = clean(v.get("Genres")).map(|g| {
                g.split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            let info = GameInfo {
                title,
                description: clean(v.get("Overview")),
                release_date,
                developer: clean(v.get("Developer")),
                publisher: clean(v.get("Publisher")),
                genre,
                players: clean(v.get("MaxPlayers")).filter(|p| p != "0"),
                rating,
            };
            id_platform.insert(id, platform.clone());
            dbs.get_mut(&platform)
                .unwrap()
                .games
                .push(LbGame { id, info });
        }
        "GameAlternateName" => {
            if let (Some(id), Some(alt)) = (id(), clean(v.get("AlternateName"))) {
                match id_platform.get(&id) {
                    Some(p) => dbs.get_mut(p).unwrap().alternates.push((alt, id)),
                    None => pending_alts.push((id, alt)),
                }
            }
        }
        "GameImage" => {
            let (Some(id), Some(file), Some(kind)) =
                (id(), clean(v.get("FileName")), clean(v.get("Type")))
            else {
                return;
            };
            if !wanted_image_type(&kind) {
                return;
            }
            let img = LbImage {
                file,
                kind,
                region: clean(v.get("Region")),
            };
            match id_platform.get(&id) {
                Some(p) => dbs
                    .get_mut(p)
                    .unwrap()
                    .images
                    .entry(id)
                    .or_default()
                    .push(img),
                None => pending_images.push((id, img)),
            }
        }
        "File" => {
            let Some(platform) = clean(v.get("Platform")) else {
                return;
            };
            if let Some(db) = dbs.get_mut(&platform) {
                if let (Some(f), Some(g)) = (clean(v.get("FileName")), clean(v.get("GameName"))) {
                    db.files.insert(f.to_lowercase(), g);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    pub const METADATA: &str = r#"<?xml version="1.0" standalone="yes"?>
<LaunchBox>
  <GameImage><DatabaseID>141</DatabaseID><FileName>early-box.png</FileName><Type>Box - Front</Type><Region>Japan</Region></GameImage>
  <Game>
    <Name>Paper Mario</Name>
    <ReleaseDate>2000-08-11T00:00:00-07:00</ReleaseDate>
    <Overview>Mario pals around &amp; saves the day.</Overview>
    <MaxPlayers>1</MaxPlayers>
    <DatabaseID>141</DatabaseID>
    <CommunityRating>4.5</CommunityRating>
    <Platform>Nintendo 64</Platform>
    <Genres>Role-Playing; Adventure</Genres>
    <Developer>Intelligent Systems</Developer>
    <Publisher>Nintendo</Publisher>
  </Game>
  <Game><Name>Super Mario World</Name><DatabaseID>7</DatabaseID><Platform>Super Nintendo Entertainment System</Platform></Game>
  <Game><Name>The Legend of Zelda: A Link to the Past</Name><DatabaseID>8</DatabaseID><Platform>Super Nintendo Entertainment System</Platform></Game>
  <GameAlternateName><AlternateName>Mario Story</AlternateName><DatabaseID>141</DatabaseID><Region>Japan</Region></GameAlternateName>
  <GameImage><DatabaseID>141</DatabaseID><FileName>us-box.png</FileName><Type>Box - Front</Type><Region>North America</Region></GameImage>
  <GameImage><DatabaseID>141</DatabaseID><FileName>box3d.png</FileName><Type>Box - 3D</Type></GameImage>
  <GameImage><DatabaseID>141</DatabaseID><FileName>logo.png</FileName><Type>Clear Logo</Type></GameImage>
  <GameImage><DatabaseID>141</DatabaseID><FileName>flyer.png</FileName><Type>Advertisement Flyer - Front</Type></GameImage>
  <GameImage><DatabaseID>141</DatabaseID><FileName>../evil.png</FileName><Type>Fanart - Background</Type></GameImage>
  <GameImage><DatabaseID>8</DatabaseID><FileName>zelda.png</FileName><Type>Box - 3D</Type></GameImage>
</LaunchBox>"#;

    pub const FILES: &str = r#"<?xml version="1.0" standalone="yes"?>
<LaunchBox>
  <File><Platform>Nintendo 64</Platform><FileName>Paper Mario (USA)</FileName><GameName>Paper Mario</GameName></File>
</LaunchBox>"#;

    pub fn make_zip(path: &Path) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("Metadata.xml", o).unwrap();
        z.write_all(METADATA.as_bytes()).unwrap();
        z.start_file("Files.xml", o).unwrap();
        z.write_all(FILES.as_bytes()).unwrap();
        z.finish().unwrap();
    }

    #[test]
    fn hostile_database_files_are_handled() {
        let t = tempfile::tempdir().unwrap();
        // Not a zip.
        let bad = t.path().join("bad.zip");
        std::fs::write(&bad, b"garbage").unwrap();
        assert!(parse_database(&bad, &["Nintendo 64".into()]).is_err());
        // Broken XML and an entity bomb: an error or an empty result, never a hang or crash.
        for xml in [
            "<LaunchBox><Game><Name>x</Name><Platform>Nintendo 64",
            "<!DOCTYPE x [<!ENTITY a \"aaaaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\">]><LaunchBox><Game><Name>&b;</Name><DatabaseID>1</DatabaseID><Platform>Nintendo 64</Platform></Game></LaunchBox>",
        ] {
            let zp = t.path().join("x.zip");
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
            z.start_file("Metadata.xml", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(xml.as_bytes()).unwrap();
            z.finish().unwrap();
            if let Ok(dbs) = parse_database(&zp, &["Nintendo 64".into()]) {
                assert!(dbs["Nintendo 64"].games.iter().all(|g| g.info.title.len() < 1000));
            }
        }
    }

    #[test]
    fn parses_and_matches() {
        let t = tempfile::tempdir().unwrap();
        let zp = t.path().join("Metadata.zip");
        make_zip(&zp);
        let mut dbs = parse_database(
            &zp,
            &[
                "Nintendo 64".into(),
                "Super Nintendo Entertainment System".into(),
            ],
        )
        .unwrap();
        dbs.values_mut().for_each(|d| d.prepare());
        let n64 = dbs.get_mut("Nintendo 64").unwrap();
        assert_eq!(n64.len(), 1);
        let p = MatchPrefs::default();
        // Known file name, title with region tag, alternate name.
        assert_eq!(n64.find(&["Paper Mario (USA)"], &p), Some(141));
        assert_eq!(n64.find(&["paper_mario (Europe)"], &p), Some(141));
        assert_eq!(n64.find(&["Mario Story (Japan)"], &p), Some(141));
        assert_eq!(n64.find(&["Mario Kart 64"], &p), None);
        let info = n64.info(141).unwrap();
        assert_eq!(
            info.description.as_deref(),
            Some("Mario pals around & saves the day.")
        );
        assert_eq!(info.release_date.as_deref(), Some("2000-08-11"));
        assert_eq!(info.genre.as_deref(), Some("Role-Playing, Adventure"));
        assert_eq!(info.rating, Some(0.9));
        // Region preference, image seen before its game, unknown types ignored.
        assert!(n64
            .image_url(141, ArtKind::Boxart, &p.regions)
            .unwrap()
            .as_str()
            .ends_with("/us-box.png"));
        let jp = vec!["japan".to_string()];
        assert!(n64
            .image_url(141, ArtKind::Boxart, &jp)
            .unwrap()
            .as_str()
            .ends_with("/early-box.png"));
        assert!(n64.image_url(141, ArtKind::Box3d, &p.regions).is_some());
        assert!(n64.image_url(141, ArtKind::Logo, &p.regions).is_some());
        // Path tricks in file names are refused.
        assert!(n64.image_url(141, ArtKind::Fanart, &p.regions).is_none());
        // SNES: "The Legend of Zelda: A Link to the Past" ↔ No-Intro style name.
        let snes = dbs.get_mut("Super Nintendo Entertainment System").unwrap();
        assert_eq!(
            snes.find(&["Legend of Zelda, The - A Link to the Past (USA)"], &p),
            Some(8)
        );
    }
}
