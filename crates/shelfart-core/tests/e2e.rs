//! End-to-end regression tests. They run fully offline: the artwork lists,
//! game databases and images are pre-seeded into a temporary cache, so the
//! whole pipeline (scan → identify → match → fetch → validate → render →
//! save → launcher metadata) runs exactly as in the app, for every launcher.

use image::{Rgba, RgbaImage};
use shelfart_core::identify::Identifier;
use shelfart_core::launchers::LauncherKind;
use shelfart_core::net::SafeClient;
use shelfart_core::paths::safe_component;
use shelfart_core::scan;
use shelfart_core::scrape::{Emit, GameStatus};
use shelfart_core::{ArtKind, Event, Libretro, ScrapeConfig, Sources, Summary};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

const SNES_REPO: &str = "Nintendo_-_Super_Nintendo_Entertainment_System";
const PSX_REPO: &str = "Sony_-_PlayStation";
const SNES_ROM: &[u8] = b"SHELFART-TEST-ROM-CONTENTS";

fn png(w: u32, h: u32, c: [u8; 4]) -> Vec<u8> {
    let img = RgbaImage::from_pixel(w, h, Rgba(c));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn ps1_iso(serial_exe: &str) -> Vec<u8> {
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
    let content = format!("BOOT = cdrom:\\{serial_exe};1\r\n");
    iso[d + 10..d + 14].copy_from_slice(&(content.len() as u32).to_le_bytes());
    iso[d + 32] = name.len() as u8;
    iso[d + 33..d + 33 + name.len()].copy_from_slice(name);
    iso[19 * 2048..19 * 2048 + content.len()].copy_from_slice(content.as_bytes());
    iso
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    cache: PathBuf,
}

/// A small library + a fully seeded offline cache.
fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("roms");
    let cache = tmp.path().join("cache");
    fs::create_dir_all(root.join("snes")).unwrap();
    fs::create_dir_all(root.join("psx")).unwrap();
    // Name match, scene-style name, hash match on a meaningless name, and an unknown game.
    fs::write(root.join("snes/Chrono Trigger (USA).sfc"), b"ct").unwrap();
    fs::write(root.join("snes/super_mario_world.sfc"), b"smw").unwrap();
    fs::write(root.join("snes/game1.sfc"), SNES_ROM).unwrap();
    fs::write(root.join("snes/Totally Unknown Homebrew.sfc"), b"zz").unwrap();
    // Disc identified by serial only.
    fs::write(root.join("psx/my backup.iso"), ps1_iso("SCUS_949.00")).unwrap();

    let seed_index = |repo: &str, names: &[&str]| {
        let mut kinds = BTreeMap::new();
        for k in ["boxart", "snap", "title", "logo"] {
            kinds.insert(k, names.to_vec());
        }
        let json = serde_json::json!({ "fetched_at": chrono_now(), "kinds": kinds });
        fs::create_dir_all(cache.join("index")).unwrap();
        fs::write(
            cache.join("index").join(format!("{repo}.json")),
            json.to_string(),
        )
        .unwrap();
        for (i, n) in names.iter().enumerate() {
            for (dir, w, h) in [
                ("Named_Boxarts", 256, 360),
                ("Named_Snaps", 256, 224),
                ("Named_Titles", 256, 224),
                ("Named_Logos", 400, 150),
            ] {
                let p = cache.join("art").join(repo).join(dir);
                fs::create_dir_all(&p).unwrap();
                fs::write(
                    p.join(format!("{}.png", safe_component(n))),
                    png(w, h, [40 + i as u8 * 30, 90, 160, 255]),
                )
                .unwrap();
            }
        }
    };
    seed_index(
        SNES_REPO,
        &[
            "Chrono Trigger (USA)",
            "Super Mario World (USA)",
            "Super Metroid (USA)",
        ],
    );
    seed_index(PSX_REPO, &["Crash Bandicoot (USA)"]);

    let crc = format!("{:08X}", crc32fast::hash(SNES_ROM));
    fs::create_dir_all(cache.join("db")).unwrap();
    fs::write(
        cache.join("db").join(format!("no-intro-{SNES_REPO}.dat")),
        format!("game (\n\tname \"Super Metroid (USA)\"\n\trom ( name \"Super Metroid (USA).sfc\" size 26 crc {crc} )\n)\n"),
    )
    .unwrap();
    fs::write(
        cache.join("db").join(format!("redump-{PSX_REPO}.dat")),
        "game (\n\tname \"Crash Bandicoot (USA)\"\n\tserial \"SCUS-94900\"\n\trom ( name \"Crash Bandicoot (USA).bin\" size 1 crc 41B5F211 )\n)\n",
    )
    .unwrap();
    Fixture {
        _tmp: tmp,
        root,
        cache,
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

struct Run {
    summary: Summary,
    games: Vec<(String, GameStatus, Option<String>, Option<String>)>,
}

fn scrape(fx: &Fixture, cfg: ScrapeConfig) -> Run {
    let lib = scan::scan_roots(std::slice::from_ref(&fx.root));
    assert_eq!(lib.systems.len(), 2, "both systems detected");
    let client = SafeClient::new().unwrap();
    let sources = Arc::new(Sources {
        libretro: Libretro::new(client.clone(), fx.cache.clone()),
        identifier: Identifier::new(client, fx.cache.clone()),
        screenscraper: None,
    });
    let games = Arc::new(Mutex::new(Vec::new()));
    let g2 = games.clone();
    let emit: Emit = Arc::new(move |e| {
        if let Event::GameDone {
            game,
            status,
            matched,
            method,
            ..
        } = e
        {
            g2.lock().unwrap().push((game, status, matched, method));
        }
    });
    let rt = tokio::runtime::Runtime::new().unwrap();
    let summary = rt.block_on(shelfart_core::run_scrape(
        lib,
        vec![],
        cfg,
        sources,
        Arc::new(AtomicBool::new(false)),
        emit,
    ));
    let games = games.lock().unwrap().clone();
    Run { summary, games }
}

fn cfg(launcher: LauncherKind, folder: Option<PathBuf>, kinds: &[ArtKind]) -> ScrapeConfig {
    ScrapeConfig {
        launcher,
        folder,
        kinds: kinds.to_vec(),
        use_screenscraper: false,
        concurrency: 2,
        ..Default::default()
    }
}

fn assert_png(p: &Path) {
    let b = fs::read(p).unwrap_or_else(|_| panic!("missing {}", p.display()));
    assert!(b.starts_with(b"\x89PNG"), "{} is not a PNG", p.display());
    image::load_from_memory(&b).unwrap();
}

fn check_matches(run: &Run) {
    let find = |g: &str| {
        run.games
            .iter()
            .find(|x| x.0 == g)
            .unwrap_or_else(|| panic!("no result for {g}"))
    };
    assert_eq!(
        find("Chrono Trigger (USA)").2.as_deref(),
        Some("Chrono Trigger (USA)")
    );
    assert_eq!(
        find("super_mario_world").2.as_deref(),
        Some("Super Mario World (USA)")
    );
    let m = find("game1");
    assert_eq!(m.2.as_deref(), Some("Super Metroid (USA)"));
    assert_eq!(m.3.as_deref(), Some("hash"));
    let c = find("my backup");
    assert_eq!(c.2.as_deref(), Some("Crash Bandicoot (USA)"));
    assert_eq!(c.3.as_deref(), Some("hash"));
    assert_eq!(find("Totally Unknown Homebrew").1, GameStatus::Missing);
}

const ALL_IMAGES: &[ArtKind] = &[
    ArtKind::Boxart,
    ArtKind::Box3d,
    ArtKind::Snap,
    ArtKind::Title,
    ArtKind::Logo,
    ArtKind::Mix,
];

#[test]
fn es_de_layout_and_rerun() {
    let fx = fixture();
    let media = fx.root.parent().unwrap().join("downloaded_media");
    let run = scrape(
        &fx,
        cfg(LauncherKind::EsDe, Some(media.clone()), ALL_IMAGES),
    );
    check_matches(&run);
    assert_eq!(run.summary.done, 4);
    assert_eq!(run.summary.missing, 1);
    for (dir, _) in [
        ("covers", 0),
        ("3dboxes", 0),
        ("screenshots", 0),
        ("titlescreens", 0),
        ("marquees", 0),
        ("miximages", 0),
    ] {
        assert_png(&media.join("snes").join(dir).join("game1.png"));
        assert_png(&media.join("psx").join(dir).join("my backup.png"));
    }
    // Running again does nothing new …
    let again = scrape(
        &fx,
        cfg(LauncherKind::EsDe, Some(media.clone()), ALL_IMAGES),
    );
    assert_eq!(again.summary.skipped, 4);
    assert_eq!(again.summary.files_saved, 0);
    // … unless overwrite is on.
    let mut c = cfg(LauncherKind::EsDe, Some(media), &[ArtKind::Boxart]);
    c.overwrite = true;
    let over = scrape(&fx, c);
    assert_eq!(over.summary.files_saved, 4);
}

#[test]
fn pegasus_layout() {
    let fx = fixture();
    let run = scrape(
        &fx,
        cfg(
            LauncherKind::Pegasus,
            None,
            &[ArtKind::Boxart, ArtKind::Snap, ArtKind::Logo],
        ),
    );
    check_matches(&run);
    let m = fx.root.join("snes/media/game1");
    for f in ["boxFront.png", "screenshot.png", "logo.png"] {
        assert_png(&m.join(f));
    }
    let meta = fs::read_to_string(fx.root.join("snes/metadata.pegasus.txt")).unwrap();
    assert!(meta.contains("collection: Super Nintendo"));
    assert!(meta.contains("file: game1.sfc"));
    // An existing metadata file is never overwritten.
    fs::write(
        fx.root.join("psx/metadata.pegasus.txt"),
        "collection: Mine\n",
    )
    .unwrap();
    fs::remove_dir_all(fx.root.join("psx/media")).unwrap();
    scrape(&fx, cfg(LauncherKind::Pegasus, None, &[ArtKind::Boxart]));
    assert_eq!(
        fs::read_to_string(fx.root.join("psx/metadata.pegasus.txt")).unwrap(),
        "collection: Mine\n"
    );
}

#[test]
fn pegasus_3d_box_takes_box_front() {
    let fx = fixture();
    scrape(
        &fx,
        cfg(
            LauncherKind::Pegasus,
            None,
            &[ArtKind::Boxart, ArtKind::Box3d],
        ),
    );
    let img = image::open(fx.root.join("snes/media/game1/boxFront.png"))
        .unwrap()
        .to_rgba8();
    // A rendered 3D box has transparent corners; a flat cover would not.
    assert_eq!(img.get_pixel(0, 0)[3], 0);
}

#[test]
fn emulationstation_gamelist() {
    let fx = fixture();
    fs::write(
        fx.root.join("snes/gamelist.xml"),
        "<?xml version=\"1.0\"?>\n<gameList><game><path>./game1.sfc</path><name>My Name</name><playcount>7</playcount></game></gameList>",
    )
    .unwrap();
    let run = scrape(
        &fx,
        cfg(
            LauncherKind::EmulationStation,
            None,
            &[ArtKind::Mix, ArtKind::Boxart, ArtKind::Logo],
        ),
    );
    check_matches(&run);
    let xml = fs::read_to_string(fx.root.join("snes/gamelist.xml")).unwrap();
    roxmltree::Document::parse(&xml).unwrap();
    assert!(xml.contains("<playcount>7</playcount>"));
    assert!(xml.contains("<name>My Name</name>"));
    assert!(xml.contains("<image>./images/game1-mix.png</image>"));
    assert!(xml.contains("<thumbnail>./images/game1-thumb.png</thumbnail>"));
    assert!(xml.contains("<marquee>./images/game1-marquee.png</marquee>"));
    assert!(fx.root.join("snes/gamelist.xml.bak").exists());
    assert_png(&fx.root.join("snes/images/game1-mix.png"));
    // PSX had no gamelist; one is created.
    assert!(fs::read_to_string(fx.root.join("psx/gamelist.xml"))
        .unwrap()
        .contains("my backup-mix.png"));
}

#[test]
fn retroarch_layout_and_playlist() {
    let fx = fixture();
    let ra = fx.root.parent().unwrap().join("RetroArch");
    let run = scrape(
        &fx,
        cfg(
            LauncherKind::RetroArch,
            Some(ra.clone()),
            &[
                ArtKind::Boxart,
                ArtKind::Snap,
                ArtKind::Title,
                ArtKind::Logo,
            ],
        ),
    );
    check_matches(&run);
    let t = ra.join("thumbnails/Nintendo - Super Nintendo Entertainment System");
    assert_png(&t.join("Named_Boxarts/game1.png"));
    // Also saved under the database name for playlists made by RetroArch's scanner.
    assert_png(&t.join("Named_Boxarts/Super Metroid (USA).png"));
    assert_png(&t.join("Named_Logos/game1.png"));
    let lpl: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            ra.join("playlists/Nintendo - Super Nintendo Entertainment System.lpl"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(lpl["items"].as_array().unwrap().len(), 4);
}

#[test]
fn launchbox_and_folder_layouts() {
    let fx = fixture();
    let lb = fx.root.parent().unwrap().join("LaunchBox");
    scrape(
        &fx,
        cfg(
            LauncherKind::LaunchBox,
            Some(lb.clone()),
            &[ArtKind::Boxart, ArtKind::Box3d, ArtKind::Logo],
        ),
    );
    let p = lb.join("Images/Super Nintendo Entertainment System");
    assert_png(&p.join("Box - Front/Chrono Trigger-01.png"));
    assert_png(&p.join("Box - 3D/Chrono Trigger-01.png"));
    assert_png(&p.join("Clear Logo/Chrono Trigger-01.png"));

    let out = fx.root.parent().unwrap().join("out");
    scrape(
        &fx,
        cfg(LauncherKind::Folder, Some(out.clone()), ALL_IMAGES),
    );
    for k in ["boxart", "box3d", "snap", "title", "logo", "mix"] {
        assert_png(&out.join("snes").join(k).join("game1.png"));
    }
}

#[test]
fn sizes_follow_preset() {
    let fx = fixture();
    let media = fx.root.parent().unwrap().join("m");
    let mut c = cfg(LauncherKind::EsDe, Some(media.clone()), &[ArtKind::Snap]);
    c.size_scale = 0.6;
    scrape(&fx, c);
    let img = image::open(media.join("snes/screenshots/game1.png")).unwrap();
    // 256x224 fitted into 768x576 (1280x960 * 0.6), crisp integer scaling.
    assert_eq!((img.width(), img.height()), (658, 576));
}

#[test]
fn missing_folder_is_refused() {
    let fx = fixture();
    let run = scrape(&fx, cfg(LauncherKind::EsDe, None, &[ArtKind::Boxart]));
    assert_eq!(run.summary.files_saved, 0);
    assert_eq!(run.summary.failed, 5);
}
