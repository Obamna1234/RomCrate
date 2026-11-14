//! Penetration-style tests: feed ShelfArt hostile and malformed input and make
//! sure it never crashes, never writes outside the chosen folder and never
//! accepts something dangerous.

use shelfart_core::identify::{self, GameDb};
use shelfart_core::imaging;
use shelfart_core::launchers::{self, GameArt, LauncherKind};
use shelfart_core::matcher::{display_title, normalize_title, split_title, MatchPrefs, NameIndex};
use shelfart_core::paths::{is_within, join_safe, safe_component};
use shelfart_core::ps3bios;
use shelfart_core::scan;
use shelfart_core::source::{parse_listing, ArtKind};
use shelfart_core::systems;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Tiny deterministic PRNG so the fuzz cases are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
    /// Random text biased toward characters that matter to our parsers.
    fn text(&mut self, n: usize) -> String {
        const ALPH: &[char] = &[
            '(', ')', '[', ']', ',', ' ', '-', '.', '/', '\\', ':', '"', '\'', '&', '<', '>', '\n',
            'a', 'Z', '0', '9', 'é', '漢', '\u{0}', '_', ';', '=',
        ];
        (0..n)
            .map(|_| ALPH[(self.next() as usize) % ALPH.len()])
            .collect()
    }
}

#[test]
fn name_handling_never_panics() {
    let mut r = Rng(0x5eed);
    let idx = NameIndex::new(
        ["Game (USA)", "Other, The - Sub (Europe) (Rev 1)"]
            .into_iter()
            .map(String::from),
    );
    for _ in 0..20_000 {
        let n = (r.next() % 80) as usize;
        let s = r.text(n);
        let _ = split_title(&s);
        let _ = normalize_title(&s);
        let _ = display_title(&s);
        let _ = idx.best_match(&s, &MatchPrefs::default());
        let c = safe_component(&s);
        assert!(!c.contains('/') && !c.contains('\\') && c != ".." && !c.is_empty());
    }
}

#[test]
fn path_traversal_is_impossible() {
    let root = Path::new("/safe/root");
    for evil in [
        "../../etc/passwd",
        "..\\..\\Windows\\System32",
        "/absolute/path",
        "C:\\x",
        "a/../../b",
        "....//....//x",
        ".",
        "..",
        "CON",
        "nul.png",
        "x\u{0}y",
    ] {
        let p = join_safe(root, &[evil, "file.png"]);
        assert!(is_within(root, &p), "{evil} escaped to {}", p.display());
        assert!(p.starts_with(root));
    }
}

#[test]
fn hostile_listing_names_stay_in_cache() {
    let html = r#"<a href="..%2F..%2Fevil.png">x</a><a href="%2E%2E%2Fup.png">y</a><a href="ok%20name.png">z</a>"#;
    let names = parse_listing(html);
    // Names containing a path separator are dropped entirely.
    assert_eq!(names, vec!["ok name"]);
}

#[test]
fn dat_parser_survives_garbage() {
    let mut r = Rng(42);
    for _ in 0..500 {
        let t = r.text(2000);
        let db = GameDb::parse_dat(&t);
        let _ = db.by_serial("SLUS-00001");
        let _ = db.by_crc(0);
    }
    // Truncated / weird but plausible input.
    let db = GameDb::parse_dat(
        "game (\n name \"A\"\n rom ( crc ZZZZZZZZ )\n serial \"\"\n rom ( name \"x\" crc 1234ABCD",
    );
    assert_eq!(db.by_crc(0x1234ABCD), Some("A"));
    let _ = identify::parse_system_cnf("BOOT2 = cdrom0:\\;1");
    let _ = identify::parse_system_cnf("BOOT2");
    let _ = identify::parse_system_cnf("BOOT = \\\\\\\\");
}

#[test]
fn file_fingerprinting_survives_garbage() {
    let t = tempfile::tempdir().unwrap();
    let mut r = Rng(7);
    let mut lib_dirs = Vec::new();
    for sys in ["psx", "ps2", "psp", "gc", "wii", "snes", "nes"] {
        let d = t.path().join(sys);
        fs::create_dir_all(&d).unwrap();
        lib_dirs.push(d);
    }
    for i in 0..40 {
        let n = (r.next() % 70_000) as usize;
        let mut data = r.bytes(n);
        // Sometimes plant real-looking headers so parsers go deeper.
        if i % 3 == 0 && n > 0x8100 {
            data[0x8001..0x8006].copy_from_slice(b"CD001");
            data[0x8000] = 1;
        }
        if i % 4 == 0 && n > 30 {
            let l = data.len();
            data[l - 22..l - 18].copy_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        }
        if i % 5 == 0 && n > 8 {
            data[..4].copy_from_slice(b"NES\x1a");
        }
        for (d, ext) in [
            ("psx", "iso"),
            ("ps2", "iso"),
            ("psp", "iso"),
            ("gc", "rvz"),
            ("wii", "wbfs"),
            ("snes", "zip"),
            ("nes", "nes"),
        ] {
            fs::write(t.path().join(d).join(format!("g{i}.{ext}")), &data).unwrap();
        }
    }
    // A cue sheet pointing outside its folder must not be followed.
    fs::write(
        t.path().join("psx/evil.cue"),
        "FILE \"../../../../etc/passwd\" BINARY\n",
    )
    .unwrap();
    let lib = scan::scan_roots(&[t.path().to_path_buf()]);
    for s in &lib.systems {
        let sys = systems::by_id(&s.system_id).unwrap();
        for g in &s.games {
            let _ = identify::fingerprint(sys, g); // must not panic
        }
    }
}

#[test]
fn ps3_update_checks_reject_fakes() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("PS3UPDAT.PUP");
    // Right magic, absurd table.
    let mut b = b"SCEUF\0\0\0".to_vec();
    b.extend_from_slice(&[0xFF; 40]);
    fs::write(&p, &b).unwrap();
    assert!(ps3bios::check_pup(&p)
        .map(|c| !c.is_genuine())
        .unwrap_or(true));
    // Right magic and a fake version string claiming 4.93 still isn't genuine.
    let mut b = b"SCEUF\0\0\0".to_vec();
    b.extend_from_slice(&1u64.to_be_bytes());
    b.extend_from_slice(&1u64.to_be_bytes());
    b.extend_from_slice(&1u64.to_be_bytes()); // 1 entry
    b.extend_from_slice(&0x50u64.to_be_bytes());
    b.extend_from_slice(&0u64.to_be_bytes());
    b.extend_from_slice(&0x100u64.to_be_bytes());
    b.extend_from_slice(&0x50u64.to_be_bytes());
    b.extend_from_slice(&5u64.to_be_bytes());
    b.extend_from_slice(&0u64.to_be_bytes());
    b.extend_from_slice(b"4.93\n");
    fs::write(&p, &b).unwrap();
    let c = ps3bios::check_pup(&p).unwrap();
    assert_eq!(c.version.as_deref(), Some("4.93"));
    assert!(!c.is_genuine(), "a forged file must not pass");
    let mut r = Rng(9);
    for _ in 0..200 {
        let n = (r.next() % 4000) as usize;
        let _ = ps3bios::extract_bios(&r.bytes(n));
    }
}

fn png_header(w: u32, h: u32) -> Vec<u8> {
    // Valid PNG signature + IHDR claiming huge dimensions, no data.
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(b"IHDR");
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    v.extend_from_slice(&13u32.to_be_bytes());
    v.extend_from_slice(&ihdr);
    v.extend_from_slice(&crc32fast::hash(&ihdr).to_be_bytes());
    v
}

#[test]
fn image_bombs_and_garbage_are_rejected() {
    // Decompression bomb: 60000 x 60000 RGBA would need ~14 GB.
    assert!(imaging::decode_validated(&png_header(60_000, 60_000)).is_err());
    assert!(imaging::decode_validated(&png_header(9000, 10)).is_err());
    // HTML error page, truncated PNG, empty.
    assert!(imaging::decode_validated(b"<!doctype html><title>404</title>").is_err());
    assert!(imaging::decode_validated(&png_header(100, 100)).is_err());
    assert!(imaging::decode_validated(b"").is_err());
    let mut r = Rng(3);
    for _ in 0..300 {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        let n = (r.next() % 3000) as usize;
        b.extend(r.bytes(n));
        let _ = imaging::decode_validated(&b);
        let mut j = vec![0xFF, 0xD8, 0xFF];
        j.extend(r.bytes(n));
        let _ = imaging::decode_validated(&j);
    }
}

fn es_fixture(
    gamelist: &str,
) -> (
    tempfile::TempDir,
    Result<Vec<String>, String>,
    std::path::PathBuf,
) {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("snes");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Game (USA).sfc"), b"x").unwrap();
    fs::write(dir.join("gamelist.xml"), gamelist).unwrap();
    let sys = systems::by_id("snes").unwrap();
    let ss = scan::scan_system_folder(sys, &dir);
    let mut files = BTreeMap::new();
    files.insert(ArtKind::Boxart, dir.join("images/Game (USA)-thumb.png"));
    let arts = vec![GameArt {
        game: ss.games[0].clone(),
        files,
    }];
    let r = launchers::finalize_system(LauncherKind::EmulationStation, None, sys, &ss, &arts)
        .map_err(|e| e.to_string());
    (t, r, dir)
}

#[test]
fn hostile_gamelists() {
    // Billion laughs.
    let bomb = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol1 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
<!ENTITY lol2 "&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;"><!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
<!ENTITY lol4 "&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;"><!ENTITY lol5 "&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;">
<!ENTITY lol6 "&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;"><!ENTITY lol7 "&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;">
<!ENTITY lol8 "&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;"><!ENTITY lol9 "&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;">]>
<gameList><game><path>./Game (USA).sfc</path><name>&lol9;</name></game></gameList>"#;
    let started = std::time::Instant::now();
    let (_t, r, dir) = es_fixture(bomb);
    assert!(
        started.elapsed().as_secs() < 10,
        "entity expansion took too long"
    );
    // Either refused (and left untouched) or handled without exploding.
    if r.is_err() {
        assert_eq!(fs::read_to_string(dir.join("gamelist.xml")).unwrap(), bomb);
    }

    // External entity (XXE) must never read local files.
    let xxe = r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><gameList><game><path>./Game (USA).sfc</path><name>&xxe;</name></game></gameList>"#;
    let (_t, _r, dir) = es_fixture(xxe);
    assert!(!fs::read_to_string(dir.join("gamelist.xml"))
        .unwrap()
        .contains("root:"));

    // Broken XML is left exactly as it was.
    let broken = "<gameList><game><path>./Game (USA).sfc</path><name>oops</game>";
    let (_t, r, dir) = es_fixture(broken);
    assert!(r.is_err());
    assert_eq!(
        fs::read_to_string(dir.join("gamelist.xml")).unwrap(),
        broken
    );

    // Markup in values is escaped on the way out.
    let tricky = "<gameList><game><path>./Game (USA).sfc</path><name>a &lt;/name&gt;&lt;script&gt; &amp; b</name></game></gameList>";
    let (_t, r, dir) = es_fixture(tricky);
    r.unwrap();
    let out = fs::read_to_string(dir.join("gamelist.xml")).unwrap();
    roxmltree::Document::parse(&out).unwrap();
    assert!(!out.contains("<script>"));
}

#[test]
fn pegasus_metadata_cannot_be_injected() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("gba");
    fs::create_dir_all(&dir).unwrap();
    // File names can't contain newlines on Windows, but can on Linux/macOS.
    #[cfg(unix)]
    fs::write(dir.join("Evil\nlaunch: rm -rf ~ (USA).gba"), b"x").unwrap();
    fs::write(dir.join("Fine (USA).gba"), b"x").unwrap();
    let sys = systems::by_id("gba").unwrap();
    let ss = scan::scan_system_folder(sys, &dir);
    launchers::finalize_system(LauncherKind::Pegasus, None, sys, &ss, &[]).unwrap();
    let meta = fs::read_to_string(dir.join("metadata.pegasus.txt")).unwrap();
    for line in meta.lines() {
        assert!(
            !line.trim_start().starts_with("launch:"),
            "injected line: {line}"
        );
    }
}
