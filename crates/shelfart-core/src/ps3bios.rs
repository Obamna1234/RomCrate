//! PS1/PS2 BIOS from the user's own copy of Sony's official PS3 system software.
//!
//! ShelfArt never ships or downloads BIOS files or firmware. The user downloads
//! PS3UPDAT.PUP from Sony; we verify it is the genuine, unmodified file, RPCS3
//! (fetched from rpcs3.net) installs and decrypts it, and this module cuts the
//! BIOS images out of the decrypted emulator files.
//!
//! The extraction logic is a reimplementation of the approach used by the
//! "PS BIOS Claim Tool" PowerShell script: find the "compatible mode" and
//! "Parity error" markers, cut between them, pad to 4 MiB, and make a
//! retail-patched copy (0220JD → 0220JC).

use md5::{Digest, Md5};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Official PS3 system software releases we can verify against.
/// The MD5 is part of Sony's own download URL for PS3UPDAT.PUP.
#[derive(Debug, Clone, Serialize)]
pub struct KnownFirmware {
    pub version: &'static str,
    pub released: &'static str,
    pub md5: &'static str,
    pub official_page: &'static str,
}

pub const OFFICIAL_PAGE: &str =
    "https://www.playstation.com/en-us/support/hardware/ps3/system-software/";

pub static KNOWN_FIRMWARE: &[KnownFirmware] = &[KnownFirmware {
    version: "4.93",
    released: "2026-03-18",
    md5: "a2b60b6ac1d2e49e230144345616927c",
    official_page: OFFICIAL_PAGE,
}];

/// Checksums of correctly extracted BIOS files (from firmware 4.9x).
/// Only checksums are stored — never the files themselves.
pub static KNOWN_BIOS: &[(&str, &str)] = &[
    ("ps3_ps1_bios.bin", "81bbe60ba7a3d1cea1d48c14cbcc647b"),
    ("ps3_ps2_emu_bios.bin", "ca27dd917b4be56a229dd1fabb0c07da"),
    (
        "ps3_ps2_emu_bios_retail_patched.bin",
        "9ebe71c33587a2b4d4963981407c9b51",
    ),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PupCheck {
    /// File starts with Sony's "SCEUF" update header.
    pub valid_header: bool,
    /// Version text stored inside the update (e.g. "4.93").
    pub version: Option<String>,
    pub md5: String,
    /// Matches an official release exactly.
    pub official: Option<KnownFirmware>,
}

impl PupCheck {
    pub fn is_genuine(&self) -> bool {
        self.valid_header && self.official.is_some()
    }
}

/// Verify a PS3UPDAT.PUP file (reads the whole file once for the checksum).
pub fn check_pup(path: &Path) -> anyhow::Result<PupCheck> {
    let mut f = std::fs::File::open(path)?;
    let mut head = [0u8; 0x30];
    f.read_exact(&mut head)?;
    let valid_header = &head[0..5] == b"SCEUF";
    let mut version = None;
    if valid_header {
        let count = u64::from_be_bytes(head[0x18..0x20].try_into()?).min(64);
        let mut table = vec![0u8; (count as usize) * 0x20];
        f.read_exact(&mut table)?;
        for e in table.chunks(0x20) {
            let id = u64::from_be_bytes(e[0..8].try_into()?);
            let off = u64::from_be_bytes(e[8..16].try_into()?);
            let len = u64::from_be_bytes(e[16..24].try_into()?);
            if id == 0x100 && len < 64 {
                f.seek(SeekFrom::Start(off))?;
                let mut v = vec![0u8; len as usize];
                f.read_exact(&mut v)?;
                version = Some(String::from_utf8_lossy(&v).trim().to_string());
            }
        }
    }
    f.seek(SeekFrom::Start(0))?;
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let md5 = format!("{:x}", hasher.finalize());
    let official = KNOWN_FIRMWARE.iter().find(|k| k.md5 == md5).cloned();
    Ok(PupCheck {
        valid_header,
        version,
        md5,
        official,
    })
}

fn find(hay: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if start >= hay.len() {
        return None;
    }
    hay[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + start)
}

/// Cut a 4 MiB PS2 BIOS image out of a decrypted PS1/PS2 emulator ELF.
pub fn extract_bios(elf: &[u8]) -> Option<Vec<u8>> {
    let start = find(elf, b"compatible mode", 0)?.checked_sub(0x12F)?;
    let end = find(elf, b"Parity error", start)? + 0x262;
    if end <= start || end > elf.len() || end - start > 4 * 1024 * 1024 {
        return None;
    }
    let mut bios = elf[start..end].to_vec();
    bios.resize(4 * 1024 * 1024, 0);
    Some(bios)
}

/// Make the retail-patched variant some emulators need ("0220JD" → "0220JC").
pub fn retail_patch(bios: &[u8]) -> Option<Vec<u8>> {
    let i = find(bios, b"0220JD", 0)?;
    let mut out = bios.to_vec();
    out[i + 5] = b'C';
    Some(out)
}

pub fn md5_hex(b: &[u8]) -> String {
    format!("{:x}", Md5::digest(b))
}

/// Which known BIOS (if any) these bytes are.
pub fn identify_bios(b: &[u8]) -> Option<&'static str> {
    let h = md5_hex(b);
    KNOWN_BIOS.iter().find(|(_, m)| *m == h).map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_between_markers() {
        let mut elf = vec![0xAAu8; 0x2000];
        elf[0x400..0x40F].copy_from_slice(b"compatible mode");
        elf[0x800..0x806].copy_from_slice(b"0220JD");
        elf[0x1000..0x100C].copy_from_slice(b"Parity error");
        let bios = extract_bios(&elf).unwrap();
        assert_eq!(bios.len(), 4 * 1024 * 1024);
        let start = 0x400 - 0x12F;
        let end = 0x1000 + 0x262;
        assert_eq!(&bios[..end - start], &elf[start..end]);
        assert!(bios[end - start..].iter().all(|&b| b == 0));
        let p = retail_patch(&bios).unwrap();
        assert!(find(&p, b"0220JC", 0).is_some());
        assert!(find(&p, b"0220JD", 0).is_none());
    }

    #[test]
    fn rejects_bad_files() {
        assert!(extract_bios(b"nothing here").is_none());
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("PS3UPDAT.PUP");
        std::fs::write(&p, vec![0u8; 100]).unwrap();
        let c = check_pup(&p).unwrap();
        assert!(!c.is_genuine());
    }

    /// Run with SHELFART_TEST_PUP=/path/to/PS3UPDAT.PUP to check a real file.
    #[test]
    fn real_pup_if_available() {
        if let Ok(p) = std::env::var("SHELFART_TEST_PUP") {
            let c = check_pup(Path::new(&p)).unwrap();
            assert!(c.valid_header);
            assert_eq!(c.version.as_deref(), Some("4.93"));
            assert!(c.is_genuine(), "md5 {}", c.md5);
        }
    }
}
