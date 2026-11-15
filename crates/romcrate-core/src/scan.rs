//! Finds systems and games inside the user's ROM folders.

use crate::systems::{self, System};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Folders that hold artwork / metadata rather than games.
const SKIP_DIRS: &[&str] = &[
    "media",
    "images",
    "image",
    "videos",
    "video",
    "manuals",
    "downloaded_media",
    "snap",
    "snaps",
    "boxart",
    "covers",
    "screenshots",
    "titlescreens",
    "marquees",
    "wheel",
    "miximages",
    "thumbnails",
    "bios",
    "system",
    "saves",
    "states",
    "savestates",
    "shaders",
    "cheats",
    ".git",
];

/// Files that are never games.
const SKIP_FILES: &[&str] = &[
    "gamelist.xml",
    "metadata.pegasus.txt",
    "metadata.txt",
    "readme.txt",
    "desktop.ini",
    "thumbs.db",
    ".ds_store",
    "systeminfo.txt",
    "_info.txt",
];

/// Extensions that are track/companion files when a sheet (.cue/.gdi/.ccd) is present.
const TRACK_EXTS: &[&str] = &["bin", "raw", "img", "sub", "iso", "wav", "ape", "ecm"];
const SHEET_EXTS: &[&str] = &["cue", "gdi", "ccd", "toc", "mds"];

/// Extensions accepted for any system: directories-as-games use these.
const GENERIC_DIR_GAME_EXTS: &[&str] =
    &["ps3", "ps3dir", "wiiu", "xbox", "pc", "psvita", "scummvm"];

#[derive(Debug, Clone, Serialize)]
pub struct Game {
    /// Absolute path to the ROM (file or game directory).
    pub path: PathBuf,
    /// File name including extension.
    pub file_name: String,
    /// File name without extension; what launchers use to find media.
    pub stem: String,
    /// Path relative to the system folder, using forward slashes.
    pub rel_path: String,
    /// Track files a .cue/.gdi sheet points to that aren't there (incomplete download).
    #[serde(default)]
    pub missing_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScannedSystem {
    pub system_id: String,
    pub system_name: String,
    pub maker: String,
    /// The folder the games were found in.
    pub path: PathBuf,
    /// Name of that folder (used by launchers like ES-DE for media paths).
    pub folder_name: String,
    pub games: Vec<Game>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Library {
    pub systems: Vec<ScannedSystem>,
}

impl Library {
    pub fn game_count(&self) -> usize {
        self.systems.iter().map(|s| s.games.len()).sum()
    }
}

/// Scan one or more ROM roots. Each root can be either a folder that contains
/// per-system folders (`roms/snes`, `roms/psx` …) or a single system folder.
pub fn scan_roots(roots: &[PathBuf]) -> Library {
    let mut lib = Library::default();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for root in roots {
        let root = match root.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
        };
        // Root itself is a system folder?
        if let Some(sys) = root
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(systems::detect_from_folder)
        {
            if seen.insert(root.clone()) {
                push_system(&mut lib, sys, &root);
            }
            continue;
        }
        // Otherwise look one and two levels down (e.g. `roms/` or `Emulation/roms/`).
        for depth_dir in child_dirs(&root).into_iter().flat_map(|d| {
            let mut v = vec![d.clone()];
            if systems::detect_from_folder(&dir_name(&d)).is_none() {
                v.extend(child_dirs(&d));
            }
            v
        }) {
            if let Some(sys) = systems::detect_from_folder(&dir_name(&depth_dir)) {
                if seen.insert(depth_dir.clone()) {
                    push_system(&mut lib, sys, &depth_dir);
                }
            }
        }
    }
    // Folders with nothing to scrape (e.g. an artwork output folder that
    // happens to be called "psx") are not shown.
    lib.systems.retain(|s| !s.games.is_empty());
    lib.systems.sort_by(|a, b| {
        a.maker
            .cmp(&b.maker)
            .then(a.system_name.cmp(&b.system_name))
    });
    lib
}

/// Scan a folder that the user has explicitly assigned to a system.
pub fn scan_system_folder(sys: &System, dir: &Path) -> ScannedSystem {
    let mut games = Vec::new();
    collect_games(sys, dir, dir, 0, &mut games);
    games.sort_by_key(|g| g.stem.to_lowercase());
    ScannedSystem {
        system_id: sys.id.to_string(),
        system_name: sys.name.to_string(),
        maker: sys.maker.to_string(),
        path: dir.to_path_buf(),
        folder_name: dir_name(dir),
        games,
    }
}

fn push_system(lib: &mut Library, sys: &System, dir: &Path) {
    let s = scan_system_folder(sys, dir);
    lib.systems.push(s);
}

fn dir_name(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

fn child_dirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| {
            let n = dir_name(p);
            !n.starts_with('.') && !SKIP_DIRS.contains(&n.to_lowercase().as_str())
        })
        .collect();
    v.sort();
    v
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase()
}

fn collect_games(sys: &System, base: &Path, dir: &Path, depth: usize, out: &mut Vec<Game>) {
    if depth > 3 {
        return;
    }
    let entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.flatten().collect(),
        Err(_) => return,
    };

    // Stems of cue sheets / playlists in this folder, used to hide track & disc files.
    let mut sheet_stems = HashSet::new();
    let mut m3u_stems = Vec::new();
    for e in &entries {
        let p = e.path();
        let ext = ext_of(&p);
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if SHEET_EXTS.contains(&ext.as_str()) {
            sheet_stems.insert(stem.clone());
        }
        if ext == "m3u" {
            m3u_stems.push(stem);
        }
    }
    let has_sheet = !sheet_stems.is_empty();

    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let lname = name.to_lowercase();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let ext = ext_of(&p);

        if is_dir {
            if SKIP_DIRS.contains(&lname.as_str()) {
                continue;
            }
            // Directory-as-game (e.g. "Game.ps3", "Game.wiiu").
            if !ext.is_empty()
                && (sys.extensions.contains(&ext.as_str())
                    || GENERIC_DIR_GAME_EXTS.contains(&ext.as_str()))
            {
                out.push(make_game(base, &p));
                continue;
            }
            collect_games(sys, base, &p, depth + 1, out);
            continue;
        }

        if SKIP_FILES.contains(&lname.as_str()) || !sys.extensions.contains(&ext.as_str()) {
            continue;
        }
        // Empty files (failed copies/downloads) are not games.
        if e.metadata().map(|m| m.len() == 0).unwrap_or(false) {
            continue;
        }
        let stem_l = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_lowercase();
        // Hide raw tracks belonging to a cue/gdi sheet.
        if has_sheet && TRACK_EXTS.contains(&ext.as_str()) && !SHEET_EXTS.contains(&ext.as_str()) {
            continue;
        }
        if stem_l.contains("(track ") {
            continue;
        }
        // Hide individual discs when an .m3u playlist for the game exists.
        if ext != "m3u"
            && m3u_stems
                .iter()
                .any(|m| stem_l.starts_with(m.as_str()) && stem_l.contains("(disc"))
        {
            continue;
        }
        out.push(make_game(base, &p));
    }
}

impl Game {
    /// A game record for a single file outside any library (used by import).
    pub fn for_file(p: &Path) -> Game {
        make_game(p.parent().unwrap_or(Path::new(".")), p)
    }
}

/// Folders that hold media/saves/BIOS rather than games.
pub fn is_skip_dir(lower_name: &str) -> bool {
    SKIP_DIRS.contains(&lower_name)
}

/// Files that are never games (gamelists, Thumbs.db…).
pub fn is_skip_file(lower_name: &str) -> bool {
    SKIP_FILES.contains(&lower_name)
}

fn make_game(base: &Path, p: &Path) -> Game {
    let file_name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let stem = p
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let rel_path = p
        .strip_prefix(base)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| file_name.clone());
    let missing_files = missing_tracks(p);
    Game {
        path: p.to_path_buf(),
        file_name,
        stem,
        rel_path,
        missing_files,
    }
}

/// Files referenced by a cue/gdi sheet that don't exist next to it.
fn missing_tracks(sheet: &Path) -> Vec<String> {
    let ext = ext_of(sheet);
    if ext != "cue" && ext != "gdi" {
        return Vec::new();
    }
    let Ok(meta) = std::fs::metadata(sheet) else {
        return Vec::new();
    };
    if meta.len() > 256 * 1024 {
        return Vec::new();
    }
    let text = std::fs::read_to_string(sheet).unwrap_or_default();
    let mut names = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        let name = if ext == "cue" {
            if !l.to_uppercase().starts_with("FILE") {
                continue;
            }
            match l.split('"').nth(1) {
                Some(n) => n.to_string(),
                None => l.split_whitespace().nth(1).unwrap_or_default().to_string(),
            }
        } else {
            // gdi: "1 0 4 2352 track01.bin 0" (name may be quoted)
            match l.split('"').nth(1) {
                Some(n) => n.to_string(),
                None => l.split_whitespace().nth(4).unwrap_or_default().to_string(),
            }
        };
        // Only look in the same folder; never follow paths elsewhere.
        if let Some(file) = Path::new(&name).file_name() {
            if !sheet.with_file_name(file).exists() {
                names.push(file.to_string_lossy().to_string());
            }
        }
    }
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_nested_library() {
        let t = tempfile::tempdir().unwrap();
        let roms = t.path().join("roms");
        fs::create_dir_all(roms.join("snes/media")).unwrap();
        fs::create_dir_all(roms.join("psx")).unwrap();
        fs::create_dir_all(roms.join("notasystem")).unwrap();
        fs::write(roms.join("snes/Super Mario World (USA).sfc"), b"x").unwrap();
        fs::write(roms.join("snes/gamelist.xml"), b"x").unwrap();
        fs::write(roms.join("snes/readme.txt"), b"x").unwrap();
        fs::write(roms.join("psx/Crash Bandicoot (USA).cue"), b"x").unwrap();
        fs::write(roms.join("psx/Crash Bandicoot (USA).bin"), b"x").unwrap();
        fs::write(roms.join("psx/Final Fantasy VII (USA).m3u"), b"x").unwrap();
        fs::write(roms.join("psx/Broken Copy (USA).chd"), b"").unwrap();
        fs::write(roms.join("psx/Final Fantasy VII (USA) (Disc 1).chd"), b"x").unwrap();

        let lib = scan_roots(&[t.path().to_path_buf()]);
        assert_eq!(lib.systems.len(), 2);
        let snes = lib.systems.iter().find(|s| s.system_id == "snes").unwrap();
        assert_eq!(snes.games.len(), 1);
        let psx = lib.systems.iter().find(|s| s.system_id == "psx").unwrap();
        let names: Vec<_> = psx.games.iter().map(|g| g.file_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Crash Bandicoot (USA).cue", "Final Fantasy VII (USA).m3u"]
        );
    }

    #[test]
    fn output_folders_inside_library_are_ignored() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("roms/snes")).unwrap();
        fs::write(t.path().join("roms/snes/Game (USA).sfc"), b"x").unwrap();
        // Artwork output that lives next to the games.
        fs::create_dir_all(t.path().join("downloaded_media/snes/covers")).unwrap();
        fs::write(
            t.path().join("downloaded_media/snes/covers/Game (USA).png"),
            b"x",
        )
        .unwrap();
        fs::create_dir_all(t.path().join("art/psx/boxart")).unwrap();
        fs::write(t.path().join("art/psx/boxart/Game.png"), b"x").unwrap();
        let lib = scan_roots(&[t.path().to_path_buf()]);
        assert_eq!(lib.systems.len(), 1);
        assert!(lib.systems[0].path.ends_with("roms/snes"));
    }

    #[test]
    fn flags_cue_sheets_with_missing_tracks() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("psx");
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("A.cue"),
            "FILE \"A.bin\" BINARY\n  TRACK 01 MODE2/2352\n",
        )
        .unwrap();
        fs::write(d.join("A.bin"), b"x").unwrap();
        fs::write(
            d.join("B.cue"),
            "FILE \"B (Track 1).bin\" BINARY\nFILE \"B (Track 2).bin\" BINARY\n",
        )
        .unwrap();
        fs::write(d.join("B (Track 1).bin"), b"x").unwrap();
        let ss = scan_system_folder(crate::systems::by_id("psx").unwrap(), &d);
        let a = ss.games.iter().find(|g| g.stem == "A").unwrap();
        let b = ss.games.iter().find(|g| g.stem == "B").unwrap();
        assert!(a.missing_files.is_empty());
        assert_eq!(b.missing_files, vec!["B (Track 2).bin"]);
    }

    #[test]
    fn root_can_be_system_folder() {
        let t = tempfile::tempdir().unwrap();
        let gba = t.path().join("gba");
        fs::create_dir_all(&gba).unwrap();
        fs::write(gba.join("Metroid Fusion (USA).gba"), b"x").unwrap();
        let lib = scan_roots(&[gba]);
        assert_eq!(lib.systems.len(), 1);
        assert_eq!(lib.game_count(), 1);
    }
}
