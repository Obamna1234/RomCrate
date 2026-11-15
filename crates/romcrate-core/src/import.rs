//! Import & sort: take any messy folder of games and put each game into
//! `<ROMs>/<console>/`, working out the console from the file's *contents*
//! first (cartridge headers, disc signatures), then its file type, then the
//! folder it was in, then the official game lists.
//!
//! Two phases, so nothing is touched until the user has seen the preview:
//! * [`plan`] only reads files and returns an [`ImportPlan`].
//! * [`execute`] copies (default) or moves the files, verifying every copy
//!   with a CRC32 read-back before it counts as done. Existing files are never
//!   overwritten.

use crate::identify::{self, Identifier};
use crate::paths::{join_safe, safe_component};
use crate::systems::{self, System};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Folder used for PC game discs (not a console; kept as-is for GameNative etc.).
pub const PC_ID: &str = "pc";
const PC_NAME: &str = "PC (Windows) discs";
/// Extra room kept free after importing.
const SPACE_MARGIN: u64 = 256 * 1024 * 1024;
const MAX_ITEMS: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Copy into the ROMs folder; the originals stay where they are.
    #[default]
    Copy,
    /// Move into the ROMs folder (a verified copy is made first when the
    /// destination is on another drive; the original is removed only after).
    Move,
}

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub mode: Mode,
    /// Rename single-file games to their official name when identified by contents.
    pub rename: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    /// Read from inside the file (header, disc signature, serial).
    Sure,
    /// File type, folder name or an exact name in the official list.
    Likely,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
pub enum ItemState {
    Ready,
    /// The same file is already in the ROMs folder.
    AlreadyThere,
    /// A different file with the same name is already there; left alone.
    Conflict(String),
    /// Couldn't tell which console; needs the user to choose.
    NeedsSystem,
    /// Not imported (e.g. a BIOS file or an incomplete disc set).
    Skipped(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub id: usize,
    /// Name shown to the user (main file).
    pub name: String,
    /// Main file followed by its companion files (tracks, discs).
    pub files: Vec<PathBuf>,
    pub size: u64,
    pub system: Option<String>,
    pub system_name: Option<String>,
    pub confidence: Confidence,
    /// Why we think it's that console, in plain words.
    pub reason: String,
    /// File name the main file will get in the ROMs folder.
    pub dest_name: String,
    /// Folder it will go into (inside the ROMs folder).
    pub dest_folder: Option<String>,
    pub state: ItemState,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemTotal {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub games: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub dest: PathBuf,
    pub mode: Mode,
    pub items: Vec<ImportItem>,
    pub systems: Vec<SystemTotal>,
    /// Bytes that will be written to the destination drive.
    pub needed_bytes: u64,
    pub free_bytes: Option<u64>,
    pub enough_space: bool,
    /// Files that were looked at but aren't games (text, images, installers…).
    pub ignored_files: usize,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------- detection

#[derive(Debug, Clone)]
struct Guess {
    system: &'static str,
    confidence: Confidence,
    reason: String,
}

fn sure(system: &'static str, reason: impl Into<String>) -> Option<Guess> {
    Some(Guess {
        system,
        confidence: Confidence::Sure,
        reason: reason.into(),
    })
}

fn read_head(path: &Path, n: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Ok(f) = File::open(path) {
        let _ = f.take(n as u64).read_to_end(&mut buf);
    }
    buf
}

fn read_at(path: &Path, off: u64, n: usize) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut b = vec![0u8; n];
    f.read_exact(&mut b).ok()?;
    Some(b)
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// Cartridge headers that identify a console with certainty. `rom` is the
/// first part of the file (at least 0x10200 bytes when available).
fn detect_cart(rom: &[u8], len: u64) -> Option<Guess> {
    let at = |o: usize, s: &[u8]| rom.get(o..o + s.len()) == Some(s);
    if at(0, b"NES\x1a") {
        return sure("nes", "NES cartridge header");
    }
    if at(0, b"FDS\x1a") {
        return sure("fds", "Famicom Disk System header");
    }
    if let Some(m) = be32(rom, 0) {
        if matches!(m, 0x8037_1240 | 0x3780_4012 | 0x4012_3780) {
            return sure("n64", "Nintendo 64 cartridge header");
        }
    }
    if at(0x100, b"NCSD") {
        return sure("3ds", "Nintendo 3DS cartridge header");
    }
    // Nintendo DS: header CRC16 at 0x15E over the first 0x15E bytes.
    if rom.len() >= 0x160 {
        let crc = crc16_modbus(&rom[..0x15E]);
        let stored = u16::from_le_bytes([rom[0x15E], rom[0x15F]]);
        if crc == stored
            && rom[0..12]
                .iter()
                .all(|c| c.is_ascii_graphic() || *c == b' ' || *c == 0)
        {
            return sure("nds", "Nintendo DS cartridge header");
        }
    }
    // Game Boy Advance: fixed byte + header complement check.
    if rom.len() >= 0xC0 && rom[0xB2] == 0x96 {
        let sum: u8 = rom[0xA0..=0xBC]
            .iter()
            .fold(0u8, |a, &b| a.wrapping_sub(b))
            .wrapping_sub(0x19);
        if sum == rom[0xBD] {
            return sure("gba", "Game Boy Advance cartridge header");
        }
    }
    // Game Boy / Color: header checksum over 0x134..=0x14C.
    if rom.len() >= 0x150 {
        let x = rom[0x134..=0x14C]
            .iter()
            .fold(0u8, |x, &b| x.wrapping_sub(b).wrapping_sub(1));
        if x == rom[0x14D] && rom[0x134..0x13F].iter().any(|c| c.is_ascii_uppercase()) {
            return if rom[0x143] & 0x80 != 0 {
                sure("gbc", "Game Boy Color cartridge header")
            } else {
                sure("gb", "Game Boy cartridge header")
            };
        }
    }
    // Mega Drive / 32X: "SEGA" at 0x100 (plain .bin/.md dumps).
    if at(0x100, b"SEGA 32X") {
        return sure("sega32x", "Sega 32X cartridge header");
    }
    if at(0x100, b"SEGA") {
        return sure("megadrive", "Mega Drive / Genesis cartridge header");
    }
    // Master System / Game Gear: "TMR SEGA".
    for o in [0x7FF0usize, 0x3FF0, 0x1FF0] {
        if at(o, b"TMR SEGA") {
            let region = rom.get(o + 0xF).map(|b| b >> 4).unwrap_or(0);
            return if (5..=7).contains(&region) {
                sure("gamegear", "Game Gear cartridge header")
            } else {
                sure("mastersystem", "Master System cartridge header")
            };
        }
    }
    if at(0, b"LYNX") {
        return sure("lynx", "Atari Lynx header");
    }
    if at(1, b"ATARI7800") {
        return sure("atari7800", "Atari 7800 header");
    }
    // SNES: internal header checksum + complement (LoROM / HiROM, with or without copier header).
    let skip = if len % 1024 == 512 { 512 } else { 0 };
    for base in [0x7FC0usize, 0xFFC0] {
        let o = base + skip;
        if let Some(h) = rom.get(o..o + 0x20) {
            let comp = u16::from_le_bytes([h[0x1C], h[0x1D]]);
            let sum = u16::from_le_bytes([h[0x1E], h[0x1F]]);
            let title_ok = h[..21].iter().all(|c| (0x20..0x7F).contains(c));
            if comp ^ sum == 0xFFFF && sum != 0 && sum != 0xFFFF && title_ok {
                return sure("snes", "Super Nintendo cartridge header");
            }
        }
    }
    None
}

fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// What kind of CD/DVD image a CHD holds (it's compressed, so the contents can't be read directly).
#[derive(Debug, PartialEq)]
enum ChdKind {
    Cd,
    Dvd,
    GdRom,
}

fn chd_kind(path: &Path) -> Option<ChdKind> {
    let head = read_head(path, 124);
    if head.get(0..8) != Some(b"MComprHD") {
        return None;
    }
    let version = be32(&head, 12)?;
    // Metadata offset: v5 at 0x30, v3/v4 at 0x24.
    let meta_off = match version {
        5 => u64::from_be_bytes(head.get(0x30..0x38)?.try_into().ok()?),
        3 | 4 => u64::from_be_bytes(head.get(0x24..0x2C)?.try_into().ok()?),
        _ => return None,
    };
    let mut off = meta_off;
    for _ in 0..64 {
        if off == 0 {
            break;
        }
        let e = read_at(path, off, 16)?;
        let tag = &e[0..4];
        match tag {
            b"CHGD" => return Some(ChdKind::GdRom),
            b"DVD " => return Some(ChdKind::Dvd),
            b"CHT2" | b"CHTR" | b"CHCD" => return Some(ChdKind::Cd),
            _ => {}
        }
        off = u64::from_be_bytes(e[8..16].try_into().ok()?);
    }
    None
}

/// Disc images: GameCube/Wii headers, Sega boot sectors, PlayStation SYSTEM.CNF, PSP, PC.
fn detect_disc(path: &Path, ext: &str) -> Option<Guess> {
    // GameCube / Wii magic words (plain, WBFS, RVZ/WIA, CISO containers).
    let hdr_off = match ext {
        "wbfs" => 0x200,
        "rvz" | "wia" => 0x58,
        "ciso" => 0x8000,
        _ => 0,
    };
    if let Some(h) = read_at(path, hdr_off, 0x20) {
        if be32(&h, 0x18) == Some(0x5D1C_9EA3) {
            return sure(
                "wii",
                format!("Wii disc ({})", String::from_utf8_lossy(&h[..6])),
            );
        }
        if be32(&h, 0x1C) == Some(0xC233_9F3D) {
            return sure(
                "gc",
                format!("GameCube disc ({})", String::from_utf8_lossy(&h[..6])),
            );
        }
    }
    // Sega boot sector text, cooked (offset 0) or raw (offset 16 after sync).
    let head = read_head(path, 0x200);
    let has = |s: &[u8]| head.windows(s.len()).take(0x40).any(|w| w == s);
    if has(b"SEGA SEGASATURN") {
        return sure("saturn", "Sega Saturn disc");
    }
    if has(b"SEGADISCSYSTEM") || has(b"SEGABOOTDISC") {
        return sure("segacd", "Sega CD / Mega-CD disc");
    }
    if has(b"SEGA SEGAKATANA") {
        return sure("dreamcast", "Dreamcast disc");
    }
    // ISO-9660 contents.
    let probe = identify::iso_probe(path)?;
    if let Some(cnf) = &probe.system_cnf {
        let serial = identify::parse_system_cnf(cnf).unwrap_or_default();
        let up = cnf.to_uppercase();
        if up.contains("BOOT2") {
            return sure(
                "ps2",
                format!("PlayStation 2 disc {serial}")
                    .trim_end()
                    .to_string(),
            );
        }
        if up.contains("BOOT") {
            return sure(
                "psx",
                format!("PlayStation disc {serial}").trim_end().to_string(),
            );
        }
    }
    if probe
        .root
        .iter()
        .any(|n| n == "UMD_DATA.BIN" || n == "PSP_GAME")
    {
        return sure("psp", "PSP disc");
    }
    let pc_markers = probe.root.iter().any(|n| {
        n == "AUTORUN.INF"
            || n == "SETUP.EXE"
            || n.ends_with(".EXE")
            || n.ends_with(".CAB")
            || n.ends_with(".MSI")
    });
    if pc_markers {
        return Some(Guess {
            system: PC_ID,
            confidence: Confidence::Sure,
            reason: "PC disc (autorun/setup files, no console boot file)".into(),
        });
    }
    None
}

/// Extensions that belong to exactly one console and are safe to trust.
#[rustfmt::skip]
const CONFIDENT_EXTS: &[(&str, &str)] = &[
    ("nes", "nes"), ("unf", "nes"), ("unif", "nes"), ("fds", "fds"), ("sfc", "snes"),
    ("smc", "snes"), ("fig", "snes"), ("swc", "snes"), ("z64", "n64"), ("n64", "n64"),
    ("v64", "n64"), ("ndd", "n64dd"), ("gb", "gb"), ("gbc", "gbc"), ("gba", "gba"),
    ("nds", "nds"), ("dsi", "nds"), ("3ds", "3ds"), ("cia", "3ds"), ("cci", "3ds"),
    ("gcm", "gc"), ("wad", "wii"), ("wud", "wiiu"), ("wux", "wiiu"), ("vb", "vb"),
    ("vboy", "vb"), ("min", "pokemini"), ("sg", "sg1000"), ("sms", "mastersystem"), ("gg", "gamegear"),
    ("gen", "megadrive"), ("smd", "megadrive"), ("md", "megadrive"), ("32x", "sega32x"), ("gdi", "dreamcast"),
    ("cdi", "dreamcast"), ("zso", "ps2"), ("pce", "pcengine"), ("sgx", "supergrafx"), ("ngp", "ngp"),
    ("ngc", "ngpc"), ("ngpc", "ngpc"), ("a26", "atari2600"), ("a52", "atari5200"), ("a78", "atari7800"),
    ("lnx", "lynx"), ("j64", "jaguar"), ("jag", "jaguar"), ("ws", "wonderswan"), ("wsc", "wonderswancolor"),
    ("col", "colecovision"), ("vec", "vectrex"), ("adf", "amiga"), ("d64", "c64"), ("t64", "c64"),
    ("tzx", "zxspectrum"), ("cdt", "amstradcpc"), ("vpk", "psvita"), ("xex", "atari800"), ("atr", "atari800"),
];

/// Disc-ish extensions whose console can only be told from contents or context.
const DISC_EXTS: &[&str] = &[
    "iso", "bin", "img", "chd", "cue", "gdi", "ccd", "mds", "cso", "pbp", "rvz", "wbfs", "gcz",
    "ciso", "wia", "m3u", "ecm",
];

const ARCHIVE_EXTS: &[&str] = &["zip", "7z"];

/// Not games: never imported, just counted.
const NOT_GAME_EXTS: &[&str] = &[
    "txt", "nfo", "diz", "url", "html", "htm", "pdf", "jpg", "jpeg", "png", "gif", "bmp", "webp",
    "mp4", "mkv", "avi", "mp3", "flac", "ogg", "wav", "xml", "json", "ini", "cfg", "log", "md5",
    "sha1", "sfv", "torrent", "lnk", "db", "ds_store", "sub", "sbi", "srm", "sav", "state", "exe",
    "dll", "bat", "sh", "py", "rar", "part",
];

/// Words that don't help name a console in a folder ("PS2 Games" → "PS2").
const FOLDER_NOISE: &[&str] = &[
    "games",
    "game",
    "roms",
    "rom",
    "isos",
    "iso",
    "collection",
    "set",
    "full",
    "complete",
    "usa",
    "eur",
    "europe",
    "pal",
    "ntsc",
    "my",
    "new",
    "backup",
    "backups",
    "dumps",
    "files",
];

fn folder_hint(path: &Path, source_root: &Path) -> Option<(&'static System, String)> {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if !d.starts_with(source_root) {
            break;
        }
        if let Some(name) = d.file_name().and_then(|n| n.to_str()) {
            if let Some(s) = systems::detect_from_folder(name) {
                return Some((s, name.to_string()));
            }
            let cleaned: Vec<&str> = name
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty() && !FOLDER_NOISE.contains(&w.to_lowercase().as_str()))
                .collect();
            if !cleaned.is_empty() {
                if let Some(s) = systems::detect_from_folder(&cleaned.join(" ")) {
                    return Some((s, name.to_string()));
                }
            }
        }
        if d == source_root {
            break;
        }
        dir = d.parent();
    }
    None
}

/// Peek at the first ROM inside a zip: its extension and its first bytes.
fn zip_peek(path: &Path) -> Option<(String, Vec<u8>, u64, usize)> {
    let f = File::open(path).ok()?;
    let mut ar = zip::ZipArchive::new(f).ok()?;
    let count = ar.len();
    let mut best: Option<usize> = None;
    for i in 0..count.min(64) {
        let e = ar.by_index_raw(i).ok()?;
        if e.is_dir() {
            continue;
        }
        let name = e.enclosed_name()?;
        let ext = name
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if !NOT_GAME_EXTS.contains(&ext.as_str()) {
            best = Some(i);
            break;
        }
    }
    let i = best?;
    let mut e = ar.by_index(i).ok()?;
    let ext = e
        .enclosed_name()
        .and_then(|n| n.extension().map(|x| x.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    let size = e.size();
    let mut head = Vec::new();
    // Bounded read: a hostile zip can't make us inflate more than this.
    let _ = (&mut e).take(0x10200).read_to_end(&mut head);
    Some((ext, head, size, count))
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase()
}

fn by_ext(ext: &str) -> Option<&'static str> {
    CONFIDENT_EXTS
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, s)| *s)
}

fn sys_name(id: &str) -> String {
    if id == PC_ID {
        return PC_NAME.into();
    }
    systems::by_id(id)
        .map(|s| s.name.to_string())
        .unwrap_or_else(|| id.to_string())
}

// ---------------------------------------------------------------- grouping

/// Files referenced by a .cue (same folder only).
fn cue_members(cue: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    let Ok(text) = fs::read_to_string(cue) else {
        return (found, missing);
    };
    for line in text.lines() {
        let l = line.trim_start();
        if !l.to_uppercase().starts_with("FILE") {
            continue;
        }
        let name = l
            .split('"')
            .nth(1)
            .map(str::to_string)
            .or_else(|| l.split_whitespace().nth(1).map(str::to_string));
        let Some(name) = name else { continue };
        let Some(base) = Path::new(&name).file_name() else {
            continue;
        };
        let p = cue.with_file_name(base);
        if p.is_file() {
            if !found.contains(&p) {
                found.push(p);
            }
        } else {
            missing.push(base.to_string_lossy().into_owned());
        }
    }
    (found, missing)
}

/// Track files listed in a .gdi (same folder only).
fn gdi_members(gdi: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    let Ok(text) = fs::read_to_string(gdi) else {
        return (found, missing);
    };
    for line in text.lines().skip(1) {
        let name = if let Some(q) = line.split('"').nth(1) {
            q.to_string()
        } else if let Some(n) = line.split_whitespace().nth(4) {
            n.to_string()
        } else {
            continue;
        };
        let Some(base) = Path::new(&name).file_name() else {
            continue;
        };
        let p = gdi.with_file_name(base);
        if p.is_file() {
            found.push(p);
        } else {
            missing.push(base.to_string_lossy().into_owned());
        }
    }
    (found, missing)
}

/// Discs listed in an .m3u (relative paths kept, but never outside its folder).
fn m3u_members(m3u: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    let Ok(text) = fs::read_to_string(m3u) else {
        return (found, missing);
    };
    let base = m3u.parent().unwrap_or(Path::new("."));
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let rel = Path::new(l);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            missing.push(l.to_string());
            continue;
        }
        let p = base.join(rel);
        if p.is_file() {
            found.push(p);
        } else {
            missing.push(l.to_string());
        }
    }
    (found, missing)
}

// ---------------------------------------------------------------- walking

struct Found {
    root: PathBuf,
    files: Vec<PathBuf>,
}

fn walk(root: &Path, skip: &Path, out: &mut Vec<PathBuf>, ignored: &mut usize, depth: usize) {
    if depth > 12 || out.len() > MAX_ITEMS {
        return;
    }
    let Ok(rd) = fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_lowercase();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() || name.starts_with('.') || name.starts_with("._") {
            continue;
        }
        if ft.is_dir() {
            // Never descend into the destination, or into media/save folders.
            if p == skip || crate::scan::is_skip_dir(&name) {
                continue;
            }
            walk(&p, skip, out, ignored, depth + 1);
        } else if ft.is_file() {
            let ext = ext_of(&p);
            if crate::scan::is_skip_file(&name) || NOT_GAME_EXTS.contains(&ext.as_str()) {
                *ignored += 1;
                continue;
            }
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------- plan

/// Look at everything under `sources` and decide where each game goes.
/// Reads files but writes nothing. `identifier` (optional) lets RomCrate use
/// the official game lists for files whose console can't be read from inside
/// (e.g. `.chd`) and to look up official names for renaming.
pub async fn plan(
    sources: &[PathBuf],
    dest: &Path,
    opts: &ImportOptions,
    identifier: Option<&Identifier>,
) -> anyhow::Result<ImportPlan> {
    anyhow::ensure!(
        !sources.is_empty(),
        "choose at least one folder to import from"
    );
    for s in sources {
        anyhow::ensure!(s.is_dir(), "{} is not a folder", s.display());
        anyhow::ensure!(
            !s.starts_with(dest),
            "the folder to import from is inside your ROMs folder — choose a different one"
        );
    }
    let mut found: Vec<Found> = Vec::new();
    let mut ignored = 0usize;
    for s in sources {
        let mut files = Vec::new();
        walk(s, dest, &mut files, &mut ignored, 0);
        found.push(Found {
            root: s.clone(),
            files,
        });
    }

    let mut items: Vec<ImportItem> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut empty = 0usize;
    let mut db_cache: HashMap<&'static str, identify::GameDb> = HashMap::new();

    for fnd in &found {
        // Group sheets/playlists with the files they reference.
        let mut claimed: HashSet<PathBuf> = HashSet::new();
        let mut groups: Vec<(PathBuf, Vec<PathBuf>, Vec<String>)> = Vec::new();
        let mut order: Vec<&PathBuf> = fnd.files.iter().collect();
        // Playlists first so their discs (and those discs' tracks) join them.
        order.sort_by_key(|p| match ext_of(p).as_str() {
            "m3u" => 0,
            "cue" | "gdi" | "ccd" => 1,
            _ => 2,
        });
        for p in order {
            if claimed.contains(p) {
                continue;
            }
            let ext = ext_of(p);
            let (mut members, missing) = match ext.as_str() {
                "m3u" => m3u_members(p),
                "cue" => cue_members(p),
                "gdi" => gdi_members(p),
                "ccd" => {
                    let mut v = Vec::new();
                    for e in ["img", "sub"] {
                        let c = p.with_extension(e);
                        if c.is_file() {
                            v.push(c);
                        }
                    }
                    (v, vec![])
                }
                _ => (vec![], vec![]),
            };
            // A playlist's discs bring their own tracks along.
            if ext == "m3u" {
                let mut extra = Vec::new();
                for m in &members {
                    match ext_of(m).as_str() {
                        "cue" => extra.extend(cue_members(m).0),
                        "gdi" => extra.extend(gdi_members(m).0),
                        _ => {}
                    }
                }
                members.extend(extra);
            }
            members.retain(|m| m != p);
            let mut all = vec![p.clone()];
            for m in members {
                if !all.contains(&m) {
                    all.push(m);
                }
            }
            if all.iter().skip(1).any(|m| claimed.contains(m)) {
                // Already part of another set (e.g. a cue inside a playlist).
                continue;
            }
            for m in &all {
                claimed.insert(m.clone());
            }
            groups.push((p.clone(), all, missing));
        }

        for (main, files, missing) in groups {
            let size: u64 = files
                .iter()
                .filter_map(|f| f.metadata().ok())
                .map(|m| m.len())
                .sum();
            let file_name = main
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if size == 0 {
                empty += 1;
                continue;
            }
            let ext = ext_of(&main);
            let lname = file_name.to_lowercase();
            let mut warnings = Vec::new();
            if !missing.is_empty() {
                warnings.push(format!(
                    "Incomplete: {} is missing ({}).",
                    if missing.len() == 1 {
                        "a file it needs"
                    } else {
                        "files it needs"
                    },
                    missing.join(", ")
                ));
            }

            // BIOS files are not games: leave them alone and say so.
            if lname.starts_with("scph") || lname.contains("bios") {
                items.push(item(
                    items.len(),
                    &main,
                    files,
                    size,
                    None,
                    Confidence::Unknown,
                    "Looks like a BIOS file",
                    ItemState::Skipped(
                        "BIOS files go in your emulator's BIOS folder, not with games".into(),
                    ),
                    warnings,
                ));
                continue;
            }

            // Which file do we look inside? (A sheet's first track, a playlist's first disc.)
            let probe_file = match ext.as_str() {
                "m3u" => files.get(1).cloned(),
                "cue" | "gdi" | "ccd" => files
                    .iter()
                    .skip(1)
                    .find(|f| !matches!(ext_of(f).as_str(), "cue" | "sub" | "sbi"))
                    .cloned(),
                _ => Some(main.clone()),
            };
            let probe_file = probe_file.and_then(|f| {
                // For a playlist whose first entry is itself a sheet, look at that sheet's track.
                if matches!(ext_of(&f).as_str(), "cue") {
                    cue_members(&f).0.into_iter().next()
                } else {
                    Some(f)
                }
            });
            let pext = probe_file.as_deref().map(ext_of).unwrap_or_default();

            let mut guess: Option<Guess> = None;
            if let Some(pf) = &probe_file {
                if ARCHIVE_EXTS.contains(&pext.as_str()) {
                    if pext == "zip" {
                        if let Some((inner_ext, head, inner_size, count)) = zip_peek(pf) {
                            guess = detect_cart(&head, inner_size).or_else(|| {
                                by_ext(&inner_ext).map(|s| Guess {
                                    system: s,
                                    confidence: Confidence::Likely,
                                    reason: format!("zip contains a .{inner_ext} file"),
                                })
                            });
                            if count > 1 && guess.is_some() {
                                if let Some(g) = &mut guess {
                                    g.reason.push_str(&format!(" ({count} files inside)"));
                                }
                            }
                        }
                    }
                } else if DISC_EXTS.contains(&pext.as_str()) {
                    guess = detect_disc(pf, &pext);
                    if guess.is_none() && pext == "bin" {
                        // A .bin can also be a cartridge dump (Mega Drive, Atari…).
                        guess = detect_cart(
                            &read_head(pf, 0x10200),
                            pf.metadata().map(|m| m.len()).unwrap_or(0),
                        );
                    }
                    if guess.is_none() && pext == "chd" {
                        match chd_kind(pf) {
                            Some(ChdKind::GdRom) => {
                                guess = Some(Guess {
                                    system: "dreamcast",
                                    confidence: Confidence::Sure,
                                    reason: "Dreamcast GD-ROM (CHD)".into(),
                                })
                            }
                            Some(ChdKind::Dvd) if size > 1_900_000_000 => {
                                guess = Some(Guess {
                                    system: "ps2",
                                    confidence: Confidence::Likely,
                                    reason: "DVD-sized disc image (CHD) — PlayStation 2".into(),
                                })
                            }
                            _ => {}
                        }
                    }
                } else {
                    let head = read_head(pf, 0x10200);
                    guess = detect_cart(&head, pf.metadata().map(|m| m.len()).unwrap_or(0));
                }
            }
            // File type next (cartridges).
            if guess.is_none() {
                let e = if ext == "zip" {
                    String::new()
                } else {
                    pext.clone()
                };
                if let Some(s) = by_ext(&e) {
                    guess = Some(Guess {
                        system: s,
                        confidence: Confidence::Likely,
                        reason: format!("file type .{e}"),
                    });
                }
            }
            // The folder it came from.
            let hint = folder_hint(&main, &fnd.root);
            if guess.is_none() {
                if let Some((s, folder)) = &hint {
                    let plausible = s.extensions.contains(&ext.as_str())
                        || ext == "m3u"
                        || ARCHIVE_EXTS.contains(&ext.as_str());
                    if plausible {
                        guess = Some(Guess {
                            system: s.id,
                            confidence: Confidence::Likely,
                            reason: format!("was in a folder called \"{folder}\""),
                        });
                    }
                }
            }
            // Finally the official lists: an exact name match for disc formats we can't look inside.
            if guess.is_none() && DISC_EXTS.contains(&ext.as_str()) {
                if let Some(idf) = identifier {
                    let stem = main
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let mut hits = Vec::new();
                    for cand in [
                        "psx",
                        "ps2",
                        "psp",
                        "saturn",
                        "segacd",
                        "dreamcast",
                        "pcenginecd",
                        "3do",
                        "neogeocd",
                    ] {
                        let Some(sys) = systems::by_id(cand) else {
                            continue;
                        };
                        if !sys.extensions.contains(&ext.as_str()) {
                            continue;
                        }
                        if !db_cache.contains_key(cand) {
                            let db = idf.load(sys).await.unwrap_or_default();
                            db_cache.insert(sys.id, db);
                        }
                        if db_cache
                            .get(cand)
                            .map(|d| d.has_name(&stem))
                            .unwrap_or(false)
                        {
                            hits.push(sys.id);
                        }
                    }
                    if hits.len() == 1 {
                        guess = Some(Guess {
                            system: hits[0],
                            confidence: Confidence::Likely,
                            reason: format!(
                                "name found in the official {} list",
                                sys_name(hits[0])
                            ),
                        });
                    }
                }
            }

            let (system, confidence, reason, state) = match guess {
                Some(g) => {
                    let mut reason = g.reason;
                    if let Some((s, folder)) = &hint {
                        if s.id != g.system && g.confidence == Confidence::Sure {
                            reason.push_str(&format!(" — even though it was in \"{folder}\""));
                        }
                    }
                    (
                        Some(g.system.to_string()),
                        g.confidence,
                        reason,
                        ItemState::Ready,
                    )
                }
                None => (
                    None,
                    Confidence::Unknown,
                    "Couldn't tell which console this is for".to_string(),
                    ItemState::NeedsSystem,
                ),
            };
            // "Check my files": problems that make a game unusable stop it here.
            let (mut state, mut reason, mut warnings) = (state, reason, warnings);
            for f in &files {
                for finding in crate::preflight::check_file(f) {
                    match finding.severity {
                        crate::preflight::Severity::Error => {
                            state = ItemState::Skipped(format!(
                                "{} — {}",
                                finding.title, finding.advice
                            ));
                        }
                        crate::preflight::Severity::Warn => {
                            warnings.push(format!("{} — {}", finding.title, finding.advice));
                        }
                        crate::preflight::Severity::Info => {
                            if finding.check == "n64-checksum" {
                                reason.push_str(" · checksum OK");
                            }
                        }
                    }
                }
            }
            let mut it = item(
                items.len(),
                &main,
                files,
                size,
                system,
                confidence,
                &reason,
                state,
                warnings,
            );
            if !missing.is_empty() && !matches!(it.state, ItemState::Skipped(_)) {
                it.state = ItemState::Skipped("incomplete — fix or re-download it first".into());
            }
            items.push(it);
            if items.len() > MAX_ITEMS {
                anyhow::bail!("too many files to import in one go (over {MAX_ITEMS})");
            }
        }
    }
    if empty > 0 {
        notes.push(format!(
            "{empty} empty file{} skipped (failed copies or downloads).",
            if empty == 1 { "" } else { "s" }
        ));
    }

    // Official names for renaming (single-file games identified by contents).
    if opts.rename {
        if let Some(idf) = identifier {
            for it in items.iter_mut() {
                if it.files.len() != 1 || it.state != ItemState::Ready {
                    continue;
                }
                let Some(sys) = it.system.as_deref().and_then(systems::by_id) else {
                    continue;
                };
                if !Identifier::supports(sys) {
                    continue;
                }
                if !db_cache.contains_key(sys.id) {
                    let db = idf.load(sys).await.unwrap_or_default();
                    db_cache.insert(sys.id, db);
                }
                let db = &db_cache[sys.id];
                let path = it.files[0].clone();
                let game = crate::scan::Game::for_file(&path);
                let fp = identify::fingerprint(sys, &game);
                if let Some(canon) = identify::lookup(db, &fp) {
                    let ext = ext_of(&path);
                    let new = format!("{}.{ext}", safe_component(canon));
                    if new != it.dest_name {
                        it.reason.push_str(" · renamed to its official name");
                        it.dest_name = new;
                    }
                }
            }
        }
    }

    finish_plan(dest, opts.mode, items, ignored, notes)
}

#[allow(clippy::too_many_arguments)]
fn item(
    id: usize,
    main: &Path,
    files: Vec<PathBuf>,
    size: u64,
    system: Option<String>,
    confidence: Confidence,
    reason: &str,
    state: ItemState,
    warnings: Vec<String>,
) -> ImportItem {
    let name = main
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    ImportItem {
        id,
        dest_name: safe_component(&name),
        name,
        files,
        size,
        system_name: system.as_deref().map(sys_name),
        system,
        confidence,
        reason: reason.to_string(),
        dest_folder: None,
        state,
        warnings,
    }
}

/// Existing system folders in the destination ("PS2", "Nintendo 64"…) are reused.
fn existing_folders(dest: &Path) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Ok(rd) = fs::read_dir(dest) {
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.eq_ignore_ascii_case(PC_ID) {
                m.insert(PC_ID.to_string(), name);
            } else if let Some(s) = systems::detect_from_folder(&name) {
                m.entry(s.id.to_string()).or_insert(name);
            }
        }
    }
    m
}

fn finish_plan(
    dest: &Path,
    mode: Mode,
    mut items: Vec<ImportItem>,
    ignored: usize,
    notes: Vec<String>,
) -> anyhow::Result<ImportPlan> {
    let folders = existing_folders(dest);
    let mut totals: BTreeMap<String, SystemTotal> = BTreeMap::new();
    let mut needed = 0u64;
    for it in items.iter_mut() {
        let Some(sys) = it.system.clone() else {
            continue;
        };
        let folder = folders.get(&sys).cloned().unwrap_or_else(|| sys.clone());
        it.dest_folder = Some(folder.clone());
        if it.state == ItemState::Ready {
            check_destination(dest, it);
        }
        if it.state == ItemState::Ready {
            let same = mode == Mode::Move
                && it
                    .files
                    .first()
                    .map(|f| same_volume(f, dest))
                    .unwrap_or(false);
            if !same {
                needed += it.size;
            }
            let t = totals.entry(sys.clone()).or_insert_with(|| SystemTotal {
                id: sys.clone(),
                name: sys_name(&sys),
                folder,
                games: 0,
                bytes: 0,
            });
            t.games += 1;
            t.bytes += it.size;
        }
    }
    // Preview order: by console, then name; things needing attention last.
    items.sort_by_key(|i| {
        let rank = match i.state {
            ItemState::Ready => 0,
            ItemState::AlreadyThere => 1,
            ItemState::NeedsSystem => 2,
            ItemState::Conflict(_) => 3,
            ItemState::Skipped(_) => 4,
        };
        (
            rank,
            i.system_name.clone().unwrap_or_default(),
            i.name.to_lowercase(),
        )
    });
    for (n, i) in items.iter_mut().enumerate() {
        i.id = n;
    }
    let free = free_space(dest);
    let enough = free
        .map(|f| f >= needed.saturating_add(SPACE_MARGIN) || needed == 0)
        .unwrap_or(true);
    Ok(ImportPlan {
        dest: dest.to_path_buf(),
        mode,
        items,
        systems: totals.into_values().collect(),
        needed_bytes: needed,
        free_bytes: free,
        enough_space: enough,
        ignored_files: ignored,
        notes,
    })
}

/// Where each of an item's files will be written.
fn targets(dest: &Path, it: &ImportItem) -> Vec<PathBuf> {
    let folder = it.dest_folder.clone().unwrap_or_default();
    let main_dir = it.files[0]
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    it.files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if i == 0 {
                join_safe(dest, &[&folder, &it.dest_name])
            } else {
                // Companion files keep their place relative to the main file.
                let rel = f
                    .strip_prefix(&main_dir)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| {
                        f.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    });
                join_safe(dest, &[&folder, &rel])
            }
        })
        .collect()
}

fn check_destination(dest: &Path, it: &mut ImportItem) {
    let t = targets(dest, it);
    let mut all_same = true;
    for (src, dst) in it.files.iter().zip(&t) {
        if src == dst {
            continue;
        }
        match fs::metadata(dst) {
            Ok(m) => {
                let same = m.len() == src.metadata().map(|s| s.len()).unwrap_or(u64::MAX)
                    && file_crc(dst).ok() == file_crc(src).ok();
                if !same {
                    it.state = ItemState::Conflict(format!(
                        "a different file called \"{}\" is already there",
                        dst.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    ));
                    return;
                }
            }
            Err(_) => all_same = false,
        }
    }
    if all_same {
        it.state = ItemState::AlreadyThere;
    }
}

fn free_space(dest: &Path) -> Option<u64> {
    let mut p = Some(dest);
    while let Some(d) = p {
        if d.exists() {
            return fs4::available_space(d).ok();
        }
        p = d.parent();
    }
    None
}

#[cfg(unix)]
fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let existing = |p: &Path| {
        p.ancestors()
            .find(|x| x.exists())
            .and_then(|x| fs::metadata(x).ok())
    };
    match (existing(a), existing(b)) {
        (Some(x), Some(y)) => x.dev() == y.dev(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_volume(a: &Path, b: &Path) -> bool {
    let root = |p: &Path| {
        p.canonicalize()
            .ok()
            .or_else(|| {
                p.ancestors()
                    .find(|x| x.exists())
                    .and_then(|x| x.canonicalize().ok())
            })
            .and_then(|c| {
                c.components()
                    .next()
                    .map(|c| c.as_os_str().to_ascii_lowercase())
            })
    };
    matches!((root(a), root(b)), (Some(x), Some(y)) if x == y)
}

fn file_crc(p: &Path) -> std::io::Result<u32> {
    let mut f = File::open(p)?;
    let mut h = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize())
}

// ---------------------------------------------------------------- execute

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ImportEvent {
    Started {
        games: usize,
        bytes: u64,
    },
    Progress {
        done_bytes: u64,
        total_bytes: u64,
        current: String,
    },
    ItemDone {
        id: usize,
        name: String,
        ok: bool,
        message: String,
    },
    Finished {
        summary: ImportSummary,
    },
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub imported: usize,
    pub already_there: usize,
    pub failed: usize,
    pub skipped: usize,
    pub bytes: u64,
    pub cancelled: bool,
    /// System folders that received games (for scraping straight after).
    pub folders: Vec<PathBuf>,
    pub errors: Vec<String>,
}

/// Copy or move the planned games. `assign` sets the console for items the
/// user chose manually (id → system id); `exclude` lists items the user unticked.
pub fn execute(
    plan: &ImportPlan,
    assign: &HashMap<usize, String>,
    exclude: &HashSet<usize>,
    cancel: &AtomicBool,
    progress: &dyn Fn(ImportEvent),
) -> ImportSummary {
    let folders = existing_folders(&plan.dest);
    let mut work: Vec<ImportItem> = Vec::new();
    let mut sum = ImportSummary::default();
    for it in &plan.items {
        if exclude.contains(&it.id) {
            sum.skipped += 1;
            continue;
        }
        let mut it = it.clone();
        if let Some(s) = assign.get(&it.id) {
            if s == PC_ID || systems::by_id(s).is_some() {
                it.system = Some(s.clone());
                it.dest_folder = Some(folders.get(s).cloned().unwrap_or_else(|| s.clone()));
                if it.state == ItemState::NeedsSystem {
                    it.state = ItemState::Ready;
                    check_destination(&plan.dest, &mut it);
                }
            }
        }
        match it.state {
            ItemState::Ready => work.push(it),
            ItemState::AlreadyThere => sum.already_there += 1,
            _ => sum.skipped += 1,
        }
    }
    let total: u64 = work.iter().map(|i| i.size).sum();
    progress(ImportEvent::Started {
        games: work.len(),
        bytes: total,
    });

    // Re-check space right before writing (things may have changed since the preview).
    let needed: u64 = work
        .iter()
        .filter(|i| !(plan.mode == Mode::Move && same_volume(&i.files[0], &plan.dest)))
        .map(|i| i.size)
        .sum();
    if let Some(free) = free_space(&plan.dest) {
        if needed > 0 && free < needed.saturating_add(SPACE_MARGIN) {
            sum.errors.push(format!(
                "Not enough space: needs {}, only {} free. Nothing was changed.",
                crate::archive::human(needed + SPACE_MARGIN),
                crate::archive::human(free)
            ));
            sum.failed = work.len();
            progress(ImportEvent::Finished {
                summary: sum.clone(),
            });
            return sum;
        }
    }

    let mut done_bytes = 0u64;
    let mut folders_used: Vec<PathBuf> = Vec::new();
    for it in &work {
        if cancel.load(Ordering::Relaxed) {
            sum.cancelled = true;
            break;
        }
        let t = targets(&plan.dest, it);
        let res = transfer_item(it, &t, plan.mode, cancel, &mut |n| {
            done_bytes += n;
            progress(ImportEvent::Progress {
                done_bytes,
                total_bytes: total,
                current: it.name.clone(),
            });
        });
        match res {
            Ok(()) => {
                sum.imported += 1;
                sum.bytes += it.size;
                if let Some(dir) = t[0].parent() {
                    if !folders_used.iter().any(|f| f == dir) {
                        folders_used.push(dir.to_path_buf());
                    }
                }
                progress(ImportEvent::ItemDone {
                    id: it.id,
                    name: it.name.clone(),
                    ok: true,
                    message: format!(
                        "{} → {}/{}",
                        if plan.mode == Mode::Move {
                            "Moved"
                        } else {
                            "Copied"
                        },
                        it.dest_folder.clone().unwrap_or_default(),
                        it.dest_name
                    ),
                });
            }
            Err(e) => {
                sum.failed += 1;
                let msg = format!("{}: {e}", it.name);
                sum.errors.push(msg.clone());
                progress(ImportEvent::ItemDone {
                    id: it.id,
                    name: it.name.clone(),
                    ok: false,
                    message: msg,
                });
            }
        }
    }
    sum.folders = folders_used;
    progress(ImportEvent::Finished {
        summary: sum.clone(),
    });
    sum
}

/// Copy/move every file of one game. All-or-nothing: if any file fails, the
/// copies already made for this game are removed and the originals are kept.
fn transfer_item(
    it: &ImportItem,
    targets: &[PathBuf],
    mode: Mode,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> anyhow::Result<()> {
    let mut written: Vec<PathBuf> = Vec::new();
    let mut renamed: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> anyhow::Result<()> {
        for (src, dst) in it.files.iter().zip(targets) {
            if src == dst {
                continue;
            }
            if dst.exists() {
                // Identical file already there (checked in the plan) → nothing to do.
                if fs::metadata(dst)?.len() == fs::metadata(src)?.len()
                    && file_crc(dst)? == file_crc(src)?
                {
                    continue;
                }
                anyhow::bail!("{} already exists — not overwritten", dst.display());
            }
            if let Some(d) = dst.parent() {
                fs::create_dir_all(d)?;
            }
            if mode == Mode::Move && fs::rename(src, dst).is_ok() {
                // Same drive: an instant, safe rename.
                renamed.push((src.clone(), dst.clone()));
                on_bytes(fs::metadata(dst).map(|m| m.len()).unwrap_or(0));
                continue;
            }
            copy_verified(src, dst, cancel, on_bytes)?;
            written.push(dst.clone());
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            if mode == Mode::Move {
                // Originals are removed only once every copy is verified.
                for (src, dst) in it.files.iter().zip(targets) {
                    if written.contains(dst) {
                        let _ = fs::remove_file(src);
                    }
                }
            }
            Ok(())
        }
        Err(e) => {
            for w in &written {
                let _ = fs::remove_file(w);
            }
            for (src, dst) in renamed.iter().rev() {
                let _ = fs::rename(dst, src);
            }
            Err(e)
        }
    }
}

/// Stream-copy to `<dst>.part`, flush to disk, read it back and compare CRCs,
/// then rename into place. A half-written file never appears under the real name.
fn copy_verified(
    src: &Path,
    dst: &Path,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> anyhow::Result<()> {
    let part = {
        let mut s = dst.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s)
    };
    let mut input = File::open(src)?;
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)?;
    let mut h = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 4 << 20];
    let res = (|| -> anyhow::Result<u32> {
        loop {
            if cancel.load(Ordering::Relaxed) {
                anyhow::bail!("cancelled");
            }
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            on_bytes(n as u64);
        }
        out.sync_all()?;
        Ok(h.clone().finalize())
    })();
    drop(out);
    let src_crc = match res {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
    };
    let copy_crc = file_crc(&part)?;
    if copy_crc != src_crc {
        let _ = fs::remove_file(&part);
        anyhow::bail!("the copy didn't match the original (drive or cable problem?) — removed");
    }
    if dst.exists() {
        let _ = fs::remove_file(&part);
        anyhow::bail!("{} appeared while copying — not overwritten", dst.display());
    }
    fs::rename(&part, dst)?;
    Ok(())
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn gba_rom() -> Vec<u8> {
        let mut r = vec![0u8; 0x200];
        r[0xA0..0xAC].copy_from_slice(b"TESTGAME\0\0\0\0");
        r[0xAC..0xB0].copy_from_slice(b"ATST");
        r[0xB0..0xB2].copy_from_slice(b"01");
        r[0xB2] = 0x96;
        let c = r[0xA0..=0xBC]
            .iter()
            .fold(0u8, |a, &b| a.wrapping_sub(b))
            .wrapping_sub(0x19);
        r[0xBD] = c;
        r
    }

    fn gb_rom(color: bool) -> Vec<u8> {
        let mut r = vec![0u8; 0x8000];
        r[0x134..0x13C].copy_from_slice(b"TESTGAME");
        if color {
            r[0x143] = 0x80;
        }
        let x = r[0x134..=0x14C]
            .iter()
            .fold(0u8, |x, &b| x.wrapping_sub(b).wrapping_sub(1));
        r[0x14D] = x;
        r
    }

    fn snes_rom(copier: bool) -> Vec<u8> {
        let mut r = vec![0u8; 0x8000 * 4];
        let h = 0x7FC0;
        r[h..h + 21].copy_from_slice(b"TEST GAME            ");
        let sum: u16 = 0x1234;
        r[h + 0x1C..h + 0x1E].copy_from_slice(&(!sum).to_le_bytes());
        r[h + 0x1E..h + 0x20].copy_from_slice(&sum.to_le_bytes());
        if copier {
            let mut v = vec![0u8; 512];
            v.extend(r);
            v
        } else {
            r
        }
    }

    fn nds_rom() -> Vec<u8> {
        let mut r = vec![0u8; 0x200];
        r[0..9].copy_from_slice(b"TESTGAME\0");
        r[0x0C..0x10].copy_from_slice(b"ATSE");
        let crc = crc16_modbus(&r[..0x15E]);
        r[0x15E..0x160].copy_from_slice(&crc.to_le_bytes());
        r
    }

    #[test]
    fn events_use_the_names_the_app_expects() {
        let e = ImportEvent::Progress {
            done_bytes: 1,
            total_bytes: 2,
            current: "x".into(),
        };
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j["type"], "progress");
        assert_eq!(j["doneBytes"], 1);
        let st = serde_json::to_value(ItemState::NeedsSystem).unwrap();
        assert_eq!(st["kind"], "needs-system");
        let c = serde_json::to_value(ItemState::Conflict("a".into())).unwrap();
        assert_eq!(c["detail"], "a");
    }

    #[test]
    fn cartridge_headers() {
        let d = |r: &[u8]| detect_cart(r, r.len() as u64).map(|g| g.system);
        assert_eq!(d(b"NES\x1a\x02\x01rest-of-rom"), Some("nes"));
        assert_eq!(d(&[0x80, 0x37, 0x12, 0x40, 0, 0]), Some("n64"));
        assert_eq!(d(&[0x37, 0x80, 0x40, 0x12, 0, 0]), Some("n64"));
        assert_eq!(d(&gba_rom()), Some("gba"));
        assert_eq!(d(&gb_rom(false)), Some("gb"));
        assert_eq!(d(&gb_rom(true)), Some("gbc"));
        assert_eq!(d(&snes_rom(false)), Some("snes"));
        assert_eq!(d(&snes_rom(true)), Some("snes"));
        assert_eq!(d(&nds_rom()), Some("nds"));
        let mut md = vec![0u8; 0x200];
        md[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        assert_eq!(d(&md), Some("megadrive"));
        // Random bytes are not claimed by anything.
        let junk: Vec<u8> = (0..0x10200u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        assert_eq!(d(&junk), None);
    }

    fn write(p: &Path, b: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn plans_a_messy_folder() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("Downloads");
        let dest = t.path().join("ROMs");
        fs::create_dir_all(dest.join("Nintendo 64")).unwrap(); // existing folder gets reused
        write(&src.join("random/mystery.bin"), &{
            let mut v = vec![0u8; 0x400];
            v[0x100..0x104].copy_from_slice(b"SEGA");
            v
        });
        write(&src.join("weird name.dat2"), b"x"); // unknown type → needs a console
        write(&src.join("Some Game (USA).sfc"), &snes_rom(false));
        write(&src.join("renamed.gba"), &gb_rom(false)); // header beats extension
        write(
            &src.join("Mario 64.z64"),
            &[0x80, 0x37, 0x12, 0x40, 1, 2, 3, 4],
        );
        write(&src.join("PS2 Games/Some Disc.chd"), b"MComprHD-not-really");
        write(
            &src.join("Game (Disc 1).cue"),
            b"FILE \"Game (Disc 1).bin\" BINARY\n",
        );
        write(&src.join("Game (Disc 1).bin"), b"track data");
        write(&src.join("Broken.cue"), b"FILE \"Broken.bin\" BINARY\n");
        write(&src.join("scph1001.bin"), b"bios");
        write(&src.join("readme.txt"), b"hi");
        write(&src.join("empty.gba"), b"");
        let p = rt()
            .block_on(plan(
                std::slice::from_ref(&src),
                &dest,
                &ImportOptions::default(),
                None,
            ))
            .unwrap();
        let get = |n: &str| {
            p.items
                .iter()
                .find(|i| i.name == n)
                .unwrap_or_else(|| panic!("{n} missing: {:#?}", p.items))
        };
        assert_eq!(get("mystery.bin").system.as_deref(), Some("megadrive"));
        assert_eq!(get("mystery.bin").confidence, Confidence::Sure);
        assert_eq!(get("Some Game (USA).sfc").system.as_deref(), Some("snes"));
        assert_eq!(get("renamed.gba").system.as_deref(), Some("gb"));
        assert_eq!(
            get("Mario 64.z64").dest_folder.as_deref(),
            Some("Nintendo 64")
        );
        assert_eq!(get("Some Disc.chd").system.as_deref(), Some("ps2"));
        assert!(get("Some Disc.chd").reason.contains("PS2 Games"));
        assert_eq!(get("weird name.dat2").state, ItemState::NeedsSystem);
        assert_eq!(
            get("Game (Disc 1).cue").files.len(),
            2,
            "cue + bin travel together"
        );
        assert!(!p.items.iter().any(|i| i.name == "Game (Disc 1).bin"));
        assert!(matches!(get("Broken.cue").state, ItemState::Skipped(_)));
        assert!(matches!(get("scph1001.bin").state, ItemState::Skipped(_)));
        assert!(!p
            .items
            .iter()
            .any(|i| i.name == "readme.txt" || i.name == "empty.gba"));
        assert_eq!(p.ignored_files, 1);
        assert!(p.notes.iter().any(|n| n.contains("empty")));
    }

    #[test]
    fn copies_verifies_and_never_overwrites() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("in");
        let dest = t.path().join("ROMs");
        write(&src.join("A.sfc"), &snes_rom(false));
        write(&src.join("B.gba"), &gba_rom());
        write(&src.join("C.nes"), b"NES\x1a original");
        write(&dest.join("nes/C.nes"), b"NES\x1a something else");
        let p = rt()
            .block_on(plan(
                std::slice::from_ref(&src),
                &dest,
                &ImportOptions::default(),
                None,
            ))
            .unwrap();
        assert!(matches!(
            p.items.iter().find(|i| i.name == "C.nes").unwrap().state,
            ItemState::Conflict(_)
        ));
        let s = execute(
            &p,
            &HashMap::new(),
            &HashSet::new(),
            &AtomicBool::new(false),
            &|_| {},
        );
        assert_eq!(s.imported, 2, "{s:?}");
        assert_eq!(fs::read(dest.join("snes/A.sfc")).unwrap(), snes_rom(false));
        assert_eq!(fs::read(dest.join("gba/B.gba")).unwrap(), gba_rom());
        assert_eq!(
            fs::read(dest.join("nes/C.nes")).unwrap(),
            b"NES\x1a something else"
        );
        assert!(src.join("A.sfc").exists(), "copy mode keeps originals");
        // Running again: everything is already there.
        let p2 = rt()
            .block_on(plan(
                std::slice::from_ref(&src),
                &dest,
                &ImportOptions::default(),
                None,
            ))
            .unwrap();
        assert!(p2
            .items
            .iter()
            .filter(|i| i.name != "C.nes")
            .all(|i| i.state == ItemState::AlreadyThere));
        // No leftovers.
        let parts: Vec<_> = walkdir_all(&dest)
            .into_iter()
            .filter(|p| p.to_string_lossy().ends_with(".part"))
            .collect();
        assert!(parts.is_empty());
    }

    #[test]
    fn move_mode_and_manual_assignment() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("in");
        let dest = t.path().join("ROMs");
        write(&src.join("Game.cue"), b"FILE \"Game.bin\" BINARY\n");
        write(&src.join("Game.bin"), b"some disc data");
        write(&src.join("mystery.xyz"), b"???");
        let p = rt()
            .block_on(plan(
                std::slice::from_ref(&src),
                &dest,
                &ImportOptions {
                    mode: Mode::Move,
                    rename: false,
                },
                None,
            ))
            .unwrap();
        let cue = p.items.iter().find(|i| i.name == "Game.cue").unwrap();
        assert_eq!(cue.state, ItemState::NeedsSystem);
        let unk = p.items.iter().find(|i| i.name == "mystery.xyz").unwrap();
        let mut assign = HashMap::new();
        assign.insert(cue.id, "psx".to_string());
        let mut exclude = HashSet::new();
        exclude.insert(unk.id);
        let s = execute(&p, &assign, &exclude, &AtomicBool::new(false), &|_| {});
        assert_eq!(s.imported, 1, "{s:?}");
        assert!(dest.join("psx/Game.cue").exists() && dest.join("psx/Game.bin").exists());
        assert!(!src.join("Game.bin").exists(), "moved");
        assert!(src.join("mystery.xyz").exists(), "unticked item untouched");
        assert_eq!(s.folders, vec![dest.join("psx")]);
    }

    #[test]
    fn hostile_names_and_links_stay_inside() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("in");
        let dest = t.path().join("ROMs");
        write(
            &src.join("x.m3u"),
            b"../../../etc/passwd\n/abs/path.chd\nok.chd\n",
        );
        write(&src.join("ok.chd"), b"MComprHD");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", src.join("link")).unwrap();
        let p = rt()
            .block_on(plan(
                std::slice::from_ref(&src),
                &dest,
                &ImportOptions::default(),
                None,
            ))
            .unwrap();
        let m3u = p.items.iter().find(|i| i.name == "x.m3u").unwrap();
        assert!(m3u.files.iter().all(|f| f.starts_with(&src)));
        assert!(matches!(
            m3u.state,
            ItemState::Skipped(_) | ItemState::NeedsSystem
        ));
        assert!(p
            .items
            .iter()
            .all(|i| i.files.iter().all(|f| f.starts_with(&src))));
    }

    #[test]
    fn destination_inside_source_is_not_rescanned() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("stuff");
        let dest = src.join("ROMs");
        write(&src.join("A.sfc"), &snes_rom(false));
        write(&dest.join("snes/Old.sfc"), &snes_rom(true));
        let p = rt()
            .block_on(plan(
                std::slice::from_ref(&src),
                &dest,
                &ImportOptions::default(),
                None,
            ))
            .unwrap();
        assert_eq!(p.items.len(), 1);
    }

    #[test]
    fn not_enough_space_changes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("in");
        let dest = t.path().join("ROMs");
        write(&src.join("A.sfc"), &snes_rom(false));
        let mut p = rt()
            .block_on(plan(
                std::slice::from_ref(&src),
                &dest,
                &ImportOptions::default(),
                None,
            ))
            .unwrap();
        p.items[0].size = u64::MAX / 4; // pretend it's enormous
        let s = execute(
            &p,
            &HashMap::new(),
            &HashSet::new(),
            &AtomicBool::new(false),
            &|_| {},
        );
        assert_eq!(s.imported, 0);
        assert!(s.errors[0].contains("Not enough space"));
        assert!(!dest.join("snes").exists());
    }

    fn walkdir_all(p: &Path) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    v.extend(walkdir_all(&e.path()));
                } else {
                    v.push(e.path());
                }
            }
        }
        v
    }
}
