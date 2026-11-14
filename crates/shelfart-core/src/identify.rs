//! Identify games by their contents rather than their file names.
//!
//! * Cartridge games: CRC32 of the ROM (or the CRC stored in a .zip) looked up
//!   in the No-Intro database.
//! * Disc games: the disc serial read from inside the image (PS1/PS2/PSP
//!   SYSTEM.CNF / UMD_DATA.BIN, GameCube/Wii disc header) looked up in Redump.
//!
//! Both databases come from the public libretro-database repository on GitHub
//! (no account) and are cached locally.

use crate::net::{SafeClient, MAX_INDEX_BYTES};
use crate::scan::Game;
use crate::systems::System;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use url::Url;

/// Files bigger than this are not hashed (disc images use their serial instead).
const MAX_HASH_BYTES: u64 = 96 * 1024 * 1024;
const DB_TTL_SECS: u64 = 30 * 24 * 3600;

/// Systems whose games are disc images (Redump database).
const DISC_SYSTEMS: &[&str] = &[
    "psx",
    "ps2",
    "psp",
    "gc",
    "wii",
    "segacd",
    "saturn",
    "dreamcast",
    "pcenginecd",
    "pcfx",
    "3do",
    "cdi",
    "neogeocd",
    "amigacd32",
];
/// Systems we don't try to hash (arcade sets, computers with many formats, modern consoles).
const NO_DB_SYSTEMS: &[&str] = &[
    "mame",
    "fbneo",
    "naomi",
    "naomi2",
    "atomiswave",
    "dos",
    "scummvm",
    "ps3",
    "psvita",
    "wiiu",
    "xbox",
    "3ds",
];

#[derive(Debug, Default)]
pub struct GameDb {
    by_crc: HashMap<u32, String>,
    by_serial: HashMap<String, Vec<String>>,
    /// GameCube/Wii 4-character game codes ("GW7P") from serials like "DL-DOL-GW7P-EUR".
    by_code: HashMap<String, Vec<String>>,
}

impl GameDb {
    pub fn len(&self) -> usize {
        self.by_crc.len().max(self.by_serial.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Parse a clrmamepro-format .dat file.
    pub fn parse_dat(text: &str) -> GameDb {
        let mut db = GameDb::default();
        let mut name: Option<String> = None;
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with("game (") {
                name = None;
            } else if let Some(rest) = l.strip_prefix("name \"") {
                if name.is_none() {
                    name = rest.strip_suffix('"').map(String::from);
                }
            } else if let Some(rest) = l.strip_prefix("serial \"") {
                if let (Some(n), Some(s)) = (&name, rest.strip_suffix('"')) {
                    for s in s.split(',') {
                        db.by_serial
                            .entry(norm_serial(s))
                            .or_default()
                            .push(n.clone());
                        let up = s.trim().to_uppercase();
                        if up.contains("DOL-") || up.contains("RVL-") {
                            for part in up.split('-').filter(|p| p.len() == 4) {
                                db.by_code
                                    .entry(part.to_string())
                                    .or_default()
                                    .push(n.clone());
                            }
                        }
                    }
                }
            } else if l.starts_with("rom (") {
                if let (Some(n), Some(crc)) = (&name, field(l, "crc")) {
                    if let Ok(c) = u32::from_str_radix(&crc, 16) {
                        db.by_crc.entry(c).or_insert_with(|| n.clone());
                    }
                }
            }
        }
        db
    }

    pub fn by_crc(&self, crc: u32) -> Option<&str> {
        self.by_crc.get(&crc).map(|s| s.as_str())
    }

    pub fn by_serial(&self, serial: &str) -> Option<&str> {
        let hit = if serial.len() == 4 {
            self.by_code.get(&serial.to_uppercase())?
        } else {
            self.by_serial.get(&norm_serial(serial))?
        };
        hit.iter()
            .find(|n| n.contains("(Disc 1)"))
            .or_else(|| hit.first())
            .map(|s| s.as_str())
    }
}

fn field(line: &str, key: &str) -> Option<String> {
    let pat = format!(" {key} ");
    let i = line.find(&pat)? + pat.len();
    line[i..]
        .split_whitespace()
        .next()
        .map(|s| s.trim_matches('"').to_string())
}

fn norm_serial(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase()
}

pub struct Identifier {
    client: SafeClient,
    cache_dir: PathBuf,
}

impl Identifier {
    pub fn new(client: SafeClient, cache_dir: PathBuf) -> Self {
        Self { client, cache_dir }
    }

    pub fn supports(sys: &System) -> bool {
        !NO_DB_SYSTEMS.contains(&sys.id)
    }

    /// Load (download or cache) the database for a system.
    pub async fn load(&self, sys: &System) -> anyhow::Result<GameDb> {
        if !Self::supports(sys) {
            return Ok(GameDb::default());
        }
        let kind = if DISC_SYSTEMS.contains(&sys.id) {
            "redump"
        } else {
            "no-intro"
        };
        let path = self
            .cache_dir
            .join("db")
            .join(format!("{kind}-{}.dat", sys.repo));
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| {
                t.elapsed()
                    .map(|e| e.as_secs() < DB_TTL_SECS)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if !fresh {
            let mut url = Url::parse(
                "https://raw.githubusercontent.com/libretro/libretro-database/HEAD/metadat/",
            )?;
            url.path_segments_mut()
                .map_err(|_| anyhow::anyhow!("bad url"))?
                .push(kind)
                .push(&format!("{}.dat", sys.libretro));
            match self.client.get(&url, MAX_INDEX_BYTES).await {
                Ok(body) => {
                    if let Some(d) = path.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let _ = std::fs::write(&path, &body);
                }
                Err(e) if !path.exists() => return Err(e.into()),
                Err(_) => {}
            }
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Ok(GameDb::parse_dat(&text))
    }
}

/// What we learned about a file's contents.
#[derive(Debug, Clone, Default)]
pub struct Fingerprint {
    pub crc: Option<u32>,
    /// CRC without a copier/iNES header, when the file had one.
    pub crc_headerless: Option<u32>,
    pub serial: Option<String>,
    pub size: Option<u64>,
}

/// Read a fingerprint from a game file (blocking; run on a worker thread).
pub fn fingerprint(sys: &System, game: &Game) -> Fingerprint {
    let path = &game.path;
    let mut fp = Fingerprint {
        size: std::fs::metadata(path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len()),
        ..Default::default()
    };
    let Some(size) = fp.size else { return fp };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();

    if DISC_SYSTEMS.contains(&sys.id) {
        fp.serial = read_serial(sys.id, path, &ext);
        return fp;
    }
    match ext.as_str() {
        "zip" => fp.crc = zip_first_crc(path),
        "7z" => {}
        _ if size <= MAX_HASH_BYTES => {
            // Stream the file (no whole-file buffering) and hash it both with
            // and without a copier/iNES header in one pass.
            if let Ok(mut f) = File::open(path) {
                let mut head = [0u8; 16];
                let n = f.read(&mut head).unwrap_or(0);
                let head = &head[..n];
                let header: u64 = match ext.as_str() {
                    "nes" if head.starts_with(b"NES\x1a") => 16,
                    "fds" if head.starts_with(b"FDS\x1a") => 16,
                    "a78" if head.len() >= 10 && &head[1..10] == b"ATARI7800" => 128,
                    "lnx" if head.starts_with(b"LYNX") => 64,
                    "sfc" | "smc" if size % 1024 == 512 => 512,
                    _ => 0,
                };
                if f.seek(SeekFrom::Start(0)).is_ok() {
                    let mut full = crc32fast::Hasher::new();
                    let mut body = crc32fast::Hasher::new();
                    let mut pos: u64 = 0;
                    let mut buf = vec![0u8; 1 << 20];
                    let mut ok = true;
                    loop {
                        match f.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                let chunk = &buf[..n];
                                full.update(chunk);
                                let skip = header.saturating_sub(pos).min(n as u64) as usize;
                                body.update(&chunk[skip..]);
                                pos += n as u64;
                            }
                            Err(_) => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        fp.crc = Some(full.finalize());
                        if header > 0 && size > header {
                            fp.crc_headerless = Some(body.finalize());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    fp
}

/// Look a fingerprint up in the database.
pub fn lookup<'a>(db: &'a GameDb, fp: &Fingerprint) -> Option<&'a str> {
    if let Some(s) = &fp.serial {
        if let Some(n) = db.by_serial(s) {
            return Some(n);
        }
    }
    fp.crc_headerless
        .and_then(|c| db.by_crc(c))
        .or_else(|| fp.crc.and_then(|c| db.by_crc(c)))
}

/// CRC32 of the first file in a zip, read from the central directory (no decompression).
fn zip_first_crc(path: &Path) -> Option<u32> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let tail = len.min(65_557 + 22);
    f.seek(SeekFrom::Start(len - tail)).ok()?;
    let mut buf = vec![0u8; tail as usize];
    f.read_exact(&mut buf).ok()?;
    // End of central directory record.
    let eocd = (0..buf.len().saturating_sub(21))
        .rev()
        .find(|&i| buf[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])?;
    let entries = u16::from_le_bytes([buf[eocd + 10], buf[eocd + 11]]);
    let cd_off = u32::from_le_bytes(buf[eocd + 16..eocd + 20].try_into().ok()?) as u64;
    if entries == 0 {
        return None;
    }
    f.seek(SeekFrom::Start(cd_off)).ok()?;
    let mut hdr = [0u8; 46];
    f.read_exact(&mut hdr).ok()?;
    if hdr[0..4] != [0x50, 0x4b, 0x01, 0x02] {
        return None;
    }
    Some(u32::from_le_bytes(hdr[16..20].try_into().ok()?))
}

fn read_at(f: &mut File, off: u64, n: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut b = vec![0u8; n];
    f.read_exact(&mut b).ok()?;
    Some(b)
}

fn read_serial(sys_id: &str, path: &Path, ext: &str) -> Option<String> {
    let mut f = File::open(path).ok()?;
    match sys_id {
        "gc" | "wii" => {
            let off = match ext {
                "iso" | "gcm" => 0,
                "wbfs" => 0x200,
                "rvz" | "wia" => 0x58,
                "ciso" => 0x8000,
                _ => return None,
            };
            let id = read_at(&mut f, off, 6)?;
            let id = std::str::from_utf8(&id).ok()?;
            id.chars()
                .all(|c| c.is_ascii_alphanumeric())
                .then(|| id[..4].to_string())
        }
        "psx" | "ps2" | "psp" => {
            let target = if ext == "cue" {
                cue_first_track(path)?
            } else {
                path.to_path_buf()
            };
            let mut f = File::open(&target).ok()?;
            let iso = IsoReader::detect(&mut f)?;
            if sys_id == "psp" {
                let data = iso.read_file(&mut f, "UMD_DATA.BIN")?;
                let s = std::str::from_utf8(&data[..data.len().min(10)]).ok()?;
                return Some(s.to_string());
            }
            let cnf = iso.read_file(&mut f, "SYSTEM.CNF")?;
            parse_system_cnf(&String::from_utf8_lossy(&cnf))
        }
        _ => None,
    }
}

fn cue_first_track(cue: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(cue).ok()?;
    let line = text
        .lines()
        .find(|l| l.trim_start().to_uppercase().starts_with("FILE"))?;
    let name = line.split('"').nth(1)?;
    // Only allow a file in the same folder.
    let name = Path::new(name).file_name()?;
    Some(cue.with_file_name(name))
}

/// "BOOT2 = cdrom0:\SLUS_208.27;1" -> "SLUS-20827"
pub fn parse_system_cnf(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|l| l.trim_start().to_uppercase().starts_with("BOOT"))?;
    let after = line.rsplit(['\\', ':']).next()?;
    let exe = after.split(';').next()?.trim();
    let clean: String = exe.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if clean.len() < 9 {
        return None;
    }
    let (pre, num) = clean.split_at(4);
    Some(format!("{}-{}", pre.to_uppercase(), num))
}

/// Minimal ISO-9660 reader supporting cooked (2048) and raw (2352) sectors.
struct IsoReader {
    sector: u64,
    data_off: u64,
    root_lba: u64,
    root_len: u64,
}

impl IsoReader {
    fn detect(f: &mut File) -> Option<IsoReader> {
        for (sector, data_off) in [(2048u64, 0u64), (2352, 24), (2352, 16)] {
            let pvd = read_at(f, 16 * sector + data_off, 2048)?;
            if pvd[0] == 1 && &pvd[1..6] == b"CD001" {
                let root = &pvd[156..156 + 34];
                let lba = u32::from_le_bytes(root[2..6].try_into().ok()?) as u64;
                let len = u32::from_le_bytes(root[10..14].try_into().ok()?) as u64;
                return Some(IsoReader {
                    sector,
                    data_off,
                    root_lba: lba,
                    root_len: len,
                });
            }
        }
        None
    }

    fn read_sectors(&self, f: &mut File, lba: u64, len: u64) -> Option<Vec<u8>> {
        let len = len.min(4 * 1024 * 1024);
        let mut out = Vec::with_capacity(len as usize);
        let mut i = 0;
        while (out.len() as u64) < len {
            let chunk = read_at(f, (lba + i) * self.sector + self.data_off, 2048)?;
            out.extend_from_slice(&chunk);
            i += 1;
        }
        out.truncate(len as usize);
        Some(out)
    }

    fn read_file(&self, f: &mut File, name: &str) -> Option<Vec<u8>> {
        let dir = self.read_sectors(f, self.root_lba, self.root_len)?;
        let mut i = 0usize;
        while i + 33 < dir.len() {
            let rec_len = dir[i] as usize;
            if rec_len == 0 {
                // Records don't cross sector boundaries.
                i = (i / 2048 + 1) * 2048;
                continue;
            }
            let name_len = dir[i + 32] as usize;
            if i + 33 + name_len > dir.len() {
                break;
            }
            let fname = String::from_utf8_lossy(&dir[i + 33..i + 33 + name_len]).to_uppercase();
            let fname = fname.split(';').next().unwrap_or_default().to_string();
            if fname == name {
                let lba = u32::from_le_bytes(dir[i + 2..i + 6].try_into().ok()?) as u64;
                let len = u32::from_le_bytes(dir[i + 10..i + 14].try_into().ok()?) as u64;
                return self.read_sectors(f, lba, len.min(64 * 1024));
            }
            i += rec_len;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAT: &str = r#"clrmamepro (
	name "Sony - PlayStation"
)

game (
	name "Crash Bandicoot (USA)"
	region "USA"
	serial "SCUS-94900"
	rom ( name "Crash Bandicoot (USA).bin" size 632083536 crc 41B5F211 md5 B4 sha1 61 serial "SCUS-94900" )
)
game (
	name "007 - Agent Under Fire (Europe)"
	serial "DL-DOL-GW7P-EUR"
	rom ( name "007 - Agent Under Fire (Europe).iso" size 1 crc E3B90F14 )
)
"#;

    #[test]
    fn parses_dat() {
        let db = GameDb::parse_dat(DAT);
        assert_eq!(db.by_crc(0x41B5F211), Some("Crash Bandicoot (USA)"));
        assert_eq!(db.by_serial("SCUS_949.00"), Some("Crash Bandicoot (USA)"));
        assert_eq!(
            db.by_serial("GW7P"),
            Some("007 - Agent Under Fire (Europe)")
        );
    }

    #[test]
    fn headered_roms_hash_both_ways() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("nes");
        std::fs::create_dir_all(&dir).unwrap();
        let mut rom = b"NES\x1a".to_vec();
        rom.extend_from_slice(&[0u8; 12]);
        let body: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        rom.extend_from_slice(&body);
        std::fs::write(dir.join("x.nes"), &rom).unwrap();
        let sys = crate::systems::by_id("nes").unwrap();
        let ss = crate::scan::scan_system_folder(sys, &dir);
        let fp = fingerprint(sys, &ss.games[0]);
        assert_eq!(fp.crc, Some(crc32fast::hash(&rom)));
        assert_eq!(fp.crc_headerless, Some(crc32fast::hash(&body)));
    }

    #[test]
    fn system_cnf() {
        assert_eq!(
            parse_system_cnf("BOOT2 = cdrom0:\\SLUS_208.27;1\nVER = 1.00"),
            Some("SLUS-20827".into())
        );
        assert_eq!(
            parse_system_cnf("BOOT = cdrom:\\SCUS_949.00;1"),
            Some("SCUS-94900".into())
        );
    }

    #[test]
    fn reads_iso_serial() {
        // Build a tiny ISO: PVD at sector 16, root dir at sector 18, SYSTEM.CNF at 19.
        let mut iso = vec![0u8; 2048 * 20];
        let pvd = 16 * 2048;
        iso[pvd] = 1;
        iso[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        let root = pvd + 156;
        iso[root] = 34;
        iso[root + 2..root + 6].copy_from_slice(&18u32.to_le_bytes());
        iso[root + 10..root + 14].copy_from_slice(&2048u32.to_le_bytes());
        let d = 18 * 2048;
        let name = b"SYSTEM.CNF;1";
        iso[d] = (33 + name.len()) as u8;
        iso[d + 2..d + 6].copy_from_slice(&19u32.to_le_bytes());
        let content = b"BOOT2 = cdrom0:\\SLES_123.45;1\n";
        iso[d + 10..d + 14].copy_from_slice(&(content.len() as u32).to_le_bytes());
        iso[d + 32] = name.len() as u8;
        iso[d + 33..d + 33 + name.len()].copy_from_slice(name);
        iso[19 * 2048..19 * 2048 + content.len()].copy_from_slice(content);
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("Game.iso");
        std::fs::write(&p, &iso).unwrap();
        assert_eq!(read_serial("ps2", &p, "iso"), Some("SLES-12345".into()));
    }

    #[test]
    fn reads_wii_header_and_zip_crc() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("x.wbfs");
        let mut b = vec![0u8; 0x400];
        b[0x200..0x206].copy_from_slice(b"SMNP01");
        std::fs::write(&p, &b).unwrap();
        assert_eq!(read_serial("wii", &p, "wbfs"), Some("SMNP".into()));

        // Minimal stored zip with one file "a.sfc" containing "hello".
        let data = b"hello";
        let crc = crc32fast::hash(data);
        let mut z = Vec::new();
        z.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(5u32).to_le_bytes());
        z.extend_from_slice(&(5u32).to_le_bytes());
        z.extend_from_slice(&(5u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(b"a.sfc");
        z.extend_from_slice(data);
        let cd = z.len() as u32;
        z.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(5u32).to_le_bytes());
        z.extend_from_slice(&(5u32).to_le_bytes());
        z.extend_from_slice(&(5u16).to_le_bytes());
        z.extend_from_slice(&[0u8; 12]);
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(b"a.sfc");
        let cd_len = z.len() as u32 - cd;
        z.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 1, 0, 1, 0]);
        z.extend_from_slice(&cd_len.to_le_bytes());
        z.extend_from_slice(&cd.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        let zp = t.path().join("g.zip");
        std::fs::write(&zp, &z).unwrap();
        assert_eq!(zip_first_crc(&zp), Some(crc));
    }
}
