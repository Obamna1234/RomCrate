//! "Check my files": problems worth knowing about *before* importing,
//! compressing or copying games to a handheld — unfinished downloads, damaged
//! archives, PlayStation Store packages, damaged N64 ROMs, disc formats that
//! need converting, and PC games whose disc copy protection won't run under
//! Wine/GameNative.
//!
//! Copy-protection findings are information only: RomCrate never modifies,
//! patches or works around copy protection, and gives no advice to do so.

use crate::identify::{self, N64Order};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: Severity,
    pub path: PathBuf,
    /// Short, plain-English summary.
    pub title: String,
    /// What to do about it.
    pub advice: String,
    /// Stable id of the check (for tests and filtering).
    pub check: &'static str,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub findings: Vec<Finding>,
    pub files_checked: usize,
    pub errors: usize,
    pub warnings: usize,
}

fn f(
    severity: Severity,
    path: &Path,
    check: &'static str,
    title: impl Into<String>,
    advice: impl Into<String>,
) -> Finding {
    Finding {
        severity,
        path: path.to_path_buf(),
        title: title.into(),
        advice: advice.into(),
        check,
    }
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase()
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

const UNFINISHED_EXTS: &[&str] = &["crdownload", "part", "download", "partial", "opdownload"];
/// Folders other programs keep their own files in. Nothing in them is a game
/// download, so "Check my files" doesn't look inside (that only caused false
/// alarms like a game's save files when someone checked all of Documents).
const OTHER_PROGRAM_DIRS: &[&str] = &[
    "my games",
    "saved games",
    "appdata",
    "node_modules",
    "$recycle.bin",
    "system volume information",
    ".godot",
    ".import",
    "__pycache__",
    ".cache",
    ".git",
];
/// Zips smaller than this that can't be opened aren't game downloads (a game
/// zip, even a broken one, is far bigger) — usually a program's own data file.
const TINY_ZIP: u64 = 64 * 1024;
const ART_DIRS: &[&str] = &[
    "media",
    "images",
    "videos",
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
];
const MAX_ZIP_ENTRY_RATIO: u64 = 1_000;
const MAX_ZIP_TOTAL: u64 = 256 * 1024 * 1024 * 1024;

// ---------------------------------------------------------------- per file

/// Checks that only need the file itself (used by import for every game file).
pub fn check_file(path: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    let ext = ext_of(path);
    let name = name_of(path);
    let lname = name.to_lowercase();

    // ".tmp" is used by every program for scratch files; it only means an
    // unfinished download when it sits on a game or archive name ("x.iso.tmp").
    let tmp_download = ext == "tmp"
        && path
            .file_stem()
            .map(|s| is_game_or_archive_ext(&ext_of(Path::new(s))))
            .unwrap_or(false);
    if UNFINISHED_EXTS.contains(&ext.as_str()) || tmp_download || lname.starts_with("unconfirmed ")
    {
        out.push(f(
            Severity::Error,
            path,
            "unfinished-download",
            "Download didn't finish",
            "This file is an incomplete download. Re-download it — never copy it to your handheld.",
        ));
        return out;
    }
    if ext == "pkg" || lname.contains("ps2 classics") {
        out.push(f(
            Severity::Error,
            path,
            "store-package",
            "PlayStation Store package, not a disc image",
            "This is a PS3/PS4 store package. PS2 emulators can't use it — you need a PS2 disc image (.iso or .chd) of the game.",
        ));
        return out;
    }
    match ext.as_str() {
        "zip" => out.extend(check_zip(path)),
        "7z" | "rar" => out.push(f(
            Severity::Info,
            path,
            "archive-unchecked",
            format!("Can't check inside .{ext} files yet"),
            "If unpacking it fails, the download is probably damaged — re-download it.",
        )),
        "z64" | "n64" | "v64" => out.extend(check_n64(path)),
        "mdx" => out.push(f(
            Severity::Warn,
            path,
            "needs-conversion",
            "Disc image format emulators and chdman can't read (.mdx)",
            "Mount it (for example in Windows Explorer or a disc tool) and save it as an .iso first, or use a converter.",
        )),
        "mds" | "mdf" => {
            let other = if ext == "mds" { "mdf" } else { "mds" };
            let partner = sibling_with_ext(path, other);
            if ext == "mds" || partner.is_none() {
                out.push(f(
                    Severity::Warn,
                    path,
                    "needs-conversion",
                    match &partner {
                        Some(_) => "Disc image in .mds/.mdf format — emulators and chdman can't read it".to_string(),
                        None => format!("Half of a .mds/.mdf disc image: the .{other} file is missing"),
                    },
                    match &partner {
                        Some(_) => "Mount it and save it as an .iso (or .bin/.cue) first, or use a converter.",
                        None => "A .mds/.mdf disc needs both files. Re-download the missing one, then convert it to .iso.",
                    },
                ));
            }
        }
        "bin" => out.extend(check_lone_bin(path)),
        "iso" => out.extend(check_iso(path)),
        _ => {}
    }
    out
}

fn looks_like_web_page(path: &Path) -> bool {
    let Ok(b) = fs::read(path) else { return false };
    let t = String::from_utf8_lossy(&b[..b.len().min(512)]).to_lowercase();
    let t = t.trim_start_matches('\u{feff}').trim_start();
    t.starts_with('<') || t.contains("<html")
}

fn is_game_or_archive_ext(ext: &str) -> bool {
    !ext.is_empty()
        && ([
            "zip", "7z", "rar", "iso", "chd", "bin", "cue", "rvz", "wbfs", "cso", "pbp", "pkg",
        ]
        .contains(&ext)
            || crate::systems::SYSTEMS
                .iter()
                .any(|s| s.extensions.contains(&ext)))
}

fn sibling_with_ext(path: &Path, ext: &str) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().to_lowercase();
    let dir = path.parent()?;
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            ext_of(p) == ext
                && p.file_stem()
                    .map(|s| s.to_string_lossy().to_lowercase() == stem)
                    .unwrap_or(false)
        })
}

/// Test every file in a zip against its stored CRC (bounded, bomb-safe).
fn check_zip(path: &Path) -> Vec<Finding> {
    let bad = |what: String| {
        vec![f(
            Severity::Error,
            path,
            "corrupt-archive",
            "This archive is damaged",
            format!("{what} Re-download it — ideally using the site's torrent link, which checks every piece."),
        )]
    };
    let Ok(file) = File::open(path) else {
        return vec![];
    };
    let small = file.metadata().map(|m| m.len() < TINY_ZIP).unwrap_or(false);
    let mut ar = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) if small && looks_like_web_page(path) => {
            return vec![f(
                Severity::Error,
                path,
                "corrupt-archive",
                "This \".zip\" is a web page, not a game",
                "The download link failed and saved an error page instead. Download the game again.",
            )]
        }
        Err(_) if small => {
            return vec![f(
                Severity::Info,
                path,
                "not-a-game-zip",
                "Small .zip that isn't a working archive — ignored",
                "It's too small to be a game, so it's probably another program's data file. RomCrate leaves it alone.",
            )]
        }
        Err(e) => return bad(format!("It can't be opened ({e}).")),
    };
    let mut total = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    for i in 0..ar.len() {
        let mut e = match ar.by_index(i) {
            Ok(e) => e,
            Err(err) => return bad(format!("Entry {i} can't be read ({err}).")),
        };
        if e.is_dir() {
            continue;
        }
        let inner = e
            .enclosed_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("entry {i}"));
        let size = e.size();
        let packed = e.compressed_size().max(1);
        total = total.saturating_add(size);
        if size / packed > MAX_ZIP_ENTRY_RATIO || total > MAX_ZIP_TOTAL {
            return vec![f(
                Severity::Error,
                path,
                "corrupt-archive",
                "This archive looks unsafe (it would expand to an absurd size)",
                format!("\"{inner}\" claims to unpack to {}. Delete it and get the game from a trusted source.", crate::archive::human(size)),
            )];
        }
        let want = e.crc32();
        let mut h = crc32fast::Hasher::new();
        let mut read = 0u64;
        loop {
            match e.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    read += n as u64;
                    if read > size {
                        return bad(format!("\"{inner}\" is longer than the archive says."));
                    }
                    h.update(&buf[..n]);
                }
                Err(err) => return bad(format!("\"{inner}\" is broken ({err}).")),
            }
        }
        if read != size || h.finalize() != want {
            return bad(format!("\"{inner}\" inside it doesn't match its checksum."));
        }
    }
    vec![]
}

/// N64 cartridge header checksum (CRC1/CRC2 at 0x10/0x14), any byte order.
pub fn n64_checksum(rom_z64: &[u8]) -> Option<N64Check> {
    const START: usize = 0x1000;
    const LEN: usize = 0x100000;
    if rom_z64.len() < START + LEN {
        return None;
    }
    let cic = match crc32fast::hash(&rom_z64[0x40..0x1000]) {
        0x6170_A4A1 => 6101,
        0x90BB_6CB5 => 6102,
        0x0B05_0EE0 => 6103,
        0x98BC_2C86 => 6105,
        0xACC8_580A => 6106,
        _ => 6102,
    };
    let (crc1, crc2) = n64_crc(rom_z64, cic);
    let be = |o: usize| u32::from_be_bytes(rom_z64[o..o + 4].try_into().unwrap());
    Some(N64Check {
        cic,
        crc1,
        crc2,
        header1: be(0x10),
        header2: be(0x14),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct N64Check {
    pub cic: u32,
    pub crc1: u32,
    pub crc2: u32,
    pub header1: u32,
    pub header2: u32,
}

impl N64Check {
    pub fn ok(&self) -> bool {
        self.crc1 == self.header1 && self.crc2 == self.header2
    }
}

fn n64_crc(rom: &[u8], cic: u32) -> (u32, u32) {
    let seed: u32 = match cic {
        6103 => 0xA388_6759,
        6105 => 0xDF26_F436,
        6106 => 0x1FEA_617A,
        _ => 0xF8CA_4DDC,
    };
    let (mut t1, mut t2, mut t3, mut t4, mut t5, mut t6) = (seed, seed, seed, seed, seed, seed);
    let be = |o: usize| u32::from_be_bytes(rom[o..o + 4].try_into().unwrap());
    let mut i = 0x1000usize;
    while i < 0x101000 {
        let d = be(i);
        if t6.wrapping_add(d) < t6 {
            t4 = t4.wrapping_add(1);
        }
        t6 = t6.wrapping_add(d);
        t3 ^= d;
        let r = d.rotate_left(d & 0x1F);
        t5 = t5.wrapping_add(r);
        t2 = if t2 > d { t2 ^ r } else { t2 ^ (t6 ^ d) };
        if cic == 6105 {
            t1 = t1.wrapping_add(be(0x750 + (i & 0xFF)) ^ d);
        } else {
            t1 = t1.wrapping_add(t5 ^ d);
        }
        i += 4;
    }
    match cic {
        6103 => ((t6 ^ t4).wrapping_add(t3), (t5 ^ t2).wrapping_add(t1)),
        6106 => (
            t6.wrapping_mul(t4).wrapping_add(t3),
            t5.wrapping_mul(t2).wrapping_add(t1),
        ),
        _ => (t6 ^ t4 ^ t3, t5 ^ t2 ^ t1),
    }
}

fn check_n64(path: &Path) -> Vec<Finding> {
    let Ok(file) = File::open(path) else {
        return vec![];
    };
    let mut head = Vec::with_capacity(0x101000);
    if file.take(0x101000).read_to_end(&mut head).is_err() {
        return vec![];
    }
    let order = N64Order::detect(&head);
    let magic_ok = matches!(
        head.get(..4),
        Some([0x80, 0x37, 0x12, 0x40] | [0x37, 0x80, 0x40, 0x12] | [0x40, 0x12, 0x37, 0x80])
    );
    if !magic_ok {
        return vec![f(
            Severity::Warn,
            path,
            "n64-checksum",
            "Doesn't look like an N64 ROM",
            "The start of the file isn't an N64 header. It may be damaged or not an N64 game.",
        )];
    }
    order.to_z64(&mut head);
    match n64_checksum(&head) {
        Some(c) if c.ok() => vec![f(
            Severity::Info,
            path,
            "n64-checksum",
            format!("N64 ROM verified (CIC-{})", c.cic),
            "The cartridge checksum matches.",
        )],
        Some(c) => vec![f(
            Severity::Error,
            path,
            "n64-checksum",
            "N64 ROM is damaged or modified",
            format!(
                "Its checksum is {:08X} {:08X} but the header says {:08X} {:08X}. Re-download it (hacks and translations can also cause this).",
                c.crc1, c.crc2, c.header1, c.header2
            ),
        )],
        None => vec![],
    }
}

fn check_lone_bin(path: &Path) -> Vec<Finding> {
    let lname = name_of(path).to_lowercase();
    if lname.starts_with("scph") || lname.contains("bios") {
        return vec![];
    }
    let Some(dir) = path.parent() else {
        return vec![];
    };
    // Referenced by a .cue/.gdi next to it?
    let referenced = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten().any(|e| {
                let p = e.path();
                matches!(ext_of(&p).as_str(), "cue" | "gdi")
                    && fs::read_to_string(&p)
                        .map(|t| t.to_lowercase().contains(&lname))
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if referenced || sibling_with_ext(path, "cue").is_some() {
        return vec![];
    }
    // Small .bin files are usually cartridge dumps (Mega Drive, Atari…), which are fine.
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let is_disc = identify::iso_probe(path).is_some() || size >= 32 * 1024 * 1024;
    if !is_disc {
        return vec![];
    }
    vec![f(
        Severity::Warn,
        path,
        "bin-without-cue",
        "Disc image without its .cue file",
        "Disc games in .bin format need the matching .cue file next to them. Re-download the .cue (or the whole set).",
    )]
}

fn check_iso(path: &Path) -> Vec<Finding> {
    let Some(probe) = identify::iso_probe(path) else {
        return vec![];
    };
    let mut out = Vec::new();
    if probe
        .root
        .iter()
        .any(|n| n == "PS3_GAME" || n == "PS3_DISC.SFB")
    {
        out.push(ps3_finding(path));
    }
    if let Some(cp) = protection_from_names(probe.root.iter().map(String::as_str)) {
        out.push(protection_finding(path, cp));
    }
    out
}

fn ps3_finding(path: &Path) -> Finding {
    f(
        Severity::Info,
        path,
        "ps3",
        "PlayStation 3 game",
        "PS3 games don't use CHD — keep it as an .iso or folder. RomCrate won't compress it.",
    )
}

// ---------------------------------------------------------------- copy protection

/// Which disc copy protection a set of file names points to.
fn protection_from_names<'a>(names: impl Iterator<Item = &'a str>) -> Option<&'static str> {
    let mut found = None;
    for n in names {
        let l = n.to_lowercase();
        let l = l.rsplit(['/', '\\']).next().unwrap_or(&l).to_string();
        if l == "secdrv.sys" || l == "dplayerx.dll" || l.ends_with(".icd") {
            return Some("SafeDisc");
        }
        if l.starts_with("cmdlineext") && l.ends_with(".dll") {
            found = Some("SecuROM");
        }
        if (l == "protect.dll" || l == "protect.exe") && found.is_none() {
            found = Some("StarForce");
        }
    }
    found
}

fn protection_in_text(text: &str) -> Option<&'static str> {
    let l = text.to_lowercase();
    if l.contains("securom") {
        Some("SecuROM")
    } else if l.contains("safedisc") {
        Some("SafeDisc")
    } else if l.contains("starforce") {
        Some("StarForce")
    } else {
        None
    }
}

fn protection_finding(path: &Path, kind: &str) -> Finding {
    f(
        Severity::Warn,
        path,
        "copy-protection",
        format!("This PC game uses {kind} disc copy protection"),
        "It probably won't start in GameNative/Wine on Android, and may not run on Windows 10/11 either. Check whether there's a digital (Steam/GOG) or console version.",
    )
}

// ---------------------------------------------------------------- folders

/// Check everything under these folders (or single files).
pub fn check(roots: &[PathBuf], progress: &dyn Fn(&Path)) -> Report {
    let mut report = Report::default();
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for r in roots {
        if r.is_file() {
            files.push(r.clone());
        } else {
            walk(r, &mut files, &mut dirs, 0);
        }
    }
    for p in &files {
        progress(p);
        report.findings.extend(check_file(p));
    }
    report.files_checked = files.len();

    // PS3 game folders.
    for d in &dirs {
        if d.file_name()
            .map(|n| n.eq_ignore_ascii_case("PS3_GAME"))
            .unwrap_or(false)
        {
            if let Some(parent) = d.parent() {
                report.findings.push(ps3_finding(parent));
            }
        }
    }

    // Copy protection per installed/disc folder (reported once per game folder).
    let mut by_folder: BTreeMap<PathBuf, Option<&'static str>> = BTreeMap::new();
    for p in &files {
        let Some(dir) = p.parent() else { continue };
        let name = name_of(p);
        let mut kind = protection_from_names(std::iter::once(name.as_str()));
        let l = name.to_lowercase();
        if kind.is_none()
            && (l.starts_with("readme")
                || l.ends_with(".nfo")
                || (l.ends_with(".txt") && l.contains("read")))
            && fs::metadata(p)
                .map(|m| m.len() < 1_000_000)
                .unwrap_or(false)
        {
            kind = fs::read(p)
                .ok()
                .and_then(|b| protection_in_text(&String::from_utf8_lossy(&b)));
        }
        if let Some(k) = kind {
            let game = game_folder(dir, roots);
            let entry = by_folder.entry(game).or_insert(None);
            // SafeDisc file evidence beats a readme mention.
            if entry.is_none() || k == "SafeDisc" {
                *entry = Some(k);
            }
        }
    }
    let mut seen = HashSet::new();
    for (folder, kind) in by_folder {
        if let Some(k) = kind {
            if seen.insert(folder.clone()) {
                report.findings.push(protection_finding(&folder, k));
            }
        }
    }

    report
        .findings
        .sort_by(|a, b| b.severity.cmp(&a.severity).then(a.path.cmp(&b.path)));
    report.errors = report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    report.warnings = report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Warn)
        .count();
    report
}

/// The folder a PC game lives in: the nearest folder (from the protection file
/// upwards, not above the chosen root) holding a program, installer or autorun;
/// otherwise the folder the file is in.
fn game_folder(dir: &Path, roots: &[PathBuf]) -> PathBuf {
    let root = roots.iter().find(|r| dir.starts_with(r));
    let looks_like_game = |d: &Path| {
        fs::read_dir(d)
            .map(|rd| {
                rd.flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    n.ends_with(".exe") || n == "autorun.inf" || n.ends_with(".msi")
                })
            })
            .unwrap_or(false)
    };
    let mut d = Some(dir);
    while let Some(cur) = d {
        if looks_like_game(cur) {
            return cur.to_path_buf();
        }
        if Some(&cur.to_path_buf()) == root {
            break;
        }
        d = cur
            .parent()
            .filter(|p| root.map(|r| p.starts_with(r)).unwrap_or(false));
    }
    dir.to_path_buf()
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>, depth: usize) {
    if depth > 16 || files.len() > 500_000 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_lowercase();
        if ft.is_symlink() || name.starts_with("._") || name == ".ds_store" {
            continue;
        }
        let p = e.path();
        if ft.is_dir() {
            // Skip artwork folders only: PC games keep real files in "System", "bin"…
            if ART_DIRS.contains(&name.as_str()) || OTHER_PROGRAM_DIRS.contains(&name.as_str()) {
                continue;
            }
            dirs.push(p.clone());
            walk(&p, files, dirs, depth + 1);
        } else if ft.is_file() {
            files.push(p);
        }
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn rnd(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    }

    /// A synthetic z64 ROM with a correct header checksum.
    fn n64_rom() -> Vec<u8> {
        let mut r = rnd(0x101000 + 0x1000, 7);
        r[0..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        let c = n64_checksum(&r).unwrap();
        r[0x10..0x14].copy_from_slice(&c.crc1.to_be_bytes());
        r[0x14..0x18].copy_from_slice(&c.crc2.to_be_bytes());
        r
    }

    fn to_v64(z: &[u8]) -> Vec<u8> {
        z.chunks(2).flat_map(|c| [c[1], c[0]]).collect()
    }
    fn to_n64(z: &[u8]) -> Vec<u8> {
        z.chunks(4).flat_map(|c| [c[3], c[2], c[1], c[0]]).collect()
    }

    fn write(p: &Path, b: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    }

    #[test]
    fn n64_checksum_all_byte_orders() {
        let t = tempfile::tempdir().unwrap();
        let z = n64_rom();
        assert!(n64_checksum(&z).unwrap().ok());
        for (name, data) in [
            ("a.z64", z.clone()),
            ("a.v64", to_v64(&z)),
            ("a.n64", to_n64(&z)),
        ] {
            let p = t.path().join(name);
            write(&p, &data);
            let r = check_file(&p);
            assert_eq!(r[0].severity, Severity::Info, "{name}: {r:?}");
            assert!(r[0].title.contains("verified"));
            // One flipped byte in the checked range → damaged.
            let mut bad = z.clone();
            bad[0x5000] ^= 0x01;
            let bad = match name {
                "a.v64" => to_v64(&bad),
                "a.n64" => to_n64(&bad),
                _ => bad,
            };
            write(&p, &bad);
            let r = check_file(&p);
            assert_eq!(r[0].severity, Severity::Error, "{name}: {r:?}");
        }
    }

    #[test]
    fn n64_cic_variants_differ() {
        let z = n64_rom();
        let a = n64_crc(&z, 6102);
        for cic in [6103, 6105, 6106] {
            assert_ne!(n64_crc(&z, cic), a, "CIC {cic} must use its own maths");
        }
        assert!(n64_checksum(&z[..0x1000]).is_none(), "too small to check");
    }

    #[test]
    fn unfinished_downloads_and_store_packages() {
        let t = tempfile::tempdir().unwrap();
        for n in [
            "Unconfirmed 123456.crdownload",
            "Game.iso.part",
            "x.download",
            "y.partial",
        ] {
            let p = t.path().join(n);
            write(&p, b"half");
            let r = check_file(&p);
            assert_eq!(
                (r[0].check, r[0].severity),
                ("unfinished-download", Severity::Error),
                "{n}"
            );
        }
        let p = t
            .path()
            .join("Manhunt (PS2 Classics Vault) [SLUS20827].pkg");
        write(&p, b"PKG");
        assert_eq!(check_file(&p)[0].check, "store-package");
    }

    #[test]
    fn other_programs_files_are_not_flagged() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        let w = |p: &str, b: &[u8]| {
            let p = r.join(p);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b).unwrap();
        };
        // What a real Documents folder had: a game's save temp file, a Godot
        // project's scratch file, a game's tiny config.zip that isn't a zip.
        w(
            "My Games/Skyrim Special Edition/Saves/Quicksave0.ess.tmp",
            b"save",
        );
        w("skit-gubbe-3d/game_world.tscn6552314879.tmp", b"godot");
        w(
            "Call of Duty Modern Warfare/players/config.zip",
            b"not really a zip",
        );
        // Real unfinished game downloads are still caught.
        w("Downloads/Crash Bandicoot.iso.tmp", b"x");
        w("Downloads/Spyro.zip.crdownload", b"x");
        let rep = check(&[r.to_path_buf()], &|_| {});
        let errs: Vec<_> = rep
            .findings
            .iter()
            .filter(|f| f.severity != Severity::Info)
            .map(|f| name_of(&f.path))
            .collect();
        assert_eq!(
            errs,
            ["Crash Bandicoot.iso.tmp", "Spyro.zip.crdownload"],
            "{:?}",
            rep.findings
        );
        assert_eq!(rep.errors, 2);
    }

    #[test]
    fn damaged_zip_names_the_bad_file() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("Manhunt (Europe) (Disc 1).zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
            let o = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            z.start_file("Manhunt (Europe) (Disc 1).bin", o).unwrap();
            z.write_all(&rnd(100_000, 3)).unwrap();
            z.finish().unwrap();
        }
        assert!(check_file(&p).is_empty(), "good zip passes");
        // Flip a byte inside the stored data.
        let mut b = fs::read(&p).unwrap();
        b[200] ^= 0xFF;
        fs::write(&p, &b).unwrap();
        let r = check_file(&p);
        assert_eq!(r[0].check, "corrupt-archive");
        assert!(r[0].advice.contains("Disc 1).bin"), "{r:?}");
        // Not a zip at all.
        fs::write(&p, b"<!DOCTYPE html><title>403 Forbidden</title>").unwrap();
        assert_eq!(
            check_file(&p)[0].check,
            "corrupt-archive",
            "error page saved as .zip"
        );
        // Small, not a zip, not a web page: another program's file — ignored.
        fs::write(&p, b"nope").unwrap();
        assert_eq!(check_file(&p)[0].severity, Severity::Info);
        // Big and broken: still a damaged download.
        fs::write(&p, rnd(200_000, 9)).unwrap();
        assert_eq!(check_file(&p)[0].check, "corrupt-archive");
    }

    #[test]
    fn disc_formats_that_need_converting() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("GTA San Andreas.mds"), b"MEDIA DESCRIPTOR");
        write(&t.path().join("GTA San Andreas.mdf"), &rnd(1000, 1));
        let r = check_file(&t.path().join("GTA San Andreas.mds"));
        assert_eq!(r[0].check, "needs-conversion");
        assert!(
            check_file(&t.path().join("GTA San Andreas.mdf")).is_empty(),
            "pair reported once"
        );
        write(&t.path().join("Alone.mdf"), b"x");
        assert!(check_file(&t.path().join("Alone.mdf"))[0]
            .title
            .contains(".mds file is missing"));
        write(&t.path().join("x.mdx"), b"x");
        assert_eq!(
            check_file(&t.path().join("x.mdx"))[0].check,
            "needs-conversion"
        );
    }

    #[test]
    fn copy_protection_is_information_only() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("PC");
        write(&root.join("Manhunt/CmdLineExt03.dll"), b"x");
        write(&root.join("Manhunt/manhunt.exe"), b"MZ");
        write(&root.join("Enter the Matrix/SECDRV.SYS"), b"x");
        write(
            &root.join("FEAR/Readme.txt"),
            b"This product uses SecuROM technology.",
        );
        write(&root.join("Clean Game/game.exe"), b"MZ");
        write(&root.join("Scarface/System/CmdLineExt03.dll"), b"x");
        write(&root.join("Scarface/Scarface.exe"), b"MZ");
        let r = check(&[t.path().to_path_buf()], &|_| {});
        let cp: Vec<_> = r
            .findings
            .iter()
            .filter(|f| f.check == "copy-protection")
            .collect();
        assert_eq!(
            cp.len(),
            4,
            "one per game, even checked from a parent folder: {cp:?}"
        );
        assert!(cp.iter().any(|f| f.path == root.join("Scarface")), "{cp:?}");
        let get = |n: &str| cp.iter().find(|f| f.path == root.join(n)).unwrap();
        assert!(get("Manhunt").title.contains("SecuROM"));
        assert!(get("Enter the Matrix").title.contains("SafeDisc"));
        assert!(get("FEAR").title.contains("SecuROM"));
        for f in &cp {
            assert_eq!(f.severity, Severity::Warn);
            let a = f.advice.to_lowercase();
            for word in ["crack", "no-cd", "nocd", "patch", "bypass", "remove"] {
                assert!(!a.contains(word), "no bypass advice ever: {a}");
            }
        }
    }

    #[test]
    fn ps3_and_lone_bins() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("Some PS3 Game/PS3_GAME/PARAM.SFO"), b"x");
        write(&t.path().join("Some PS3 Game/PS3_DISC.SFB"), b"x");
        // A big disc .bin with no .cue, a small cartridge .bin, and a .bin that has its .cue.
        let big = t.path().join("Disc Game.bin");
        File::create(&big)
            .unwrap()
            .set_len(40 * 1024 * 1024)
            .unwrap();
        write(&t.path().join("Sonic.bin"), &rnd(512 * 1024, 2));
        write(&t.path().join("Ok Game.bin"), &rnd(1000, 4));
        write(
            &t.path().join("Ok Game.cue"),
            b"FILE \"Ok Game.bin\" BINARY\n",
        );
        let r = check(&[t.path().to_path_buf()], &|_| {});
        assert!(r
            .findings
            .iter()
            .any(|f| f.check == "ps3" && f.path == t.path().join("Some PS3 Game")));
        let lone: Vec<_> = r
            .findings
            .iter()
            .filter(|f| f.check == "bin-without-cue")
            .collect();
        assert_eq!(lone.len(), 1, "{lone:?}");
        assert_eq!(lone[0].path, big);
        assert_eq!(r.errors, 0);
    }
}
