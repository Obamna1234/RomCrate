//! Per-game settings end to end against the stand-in handheld
//! (tools/simulate/fake_adb.py). Game files are synthetic: just the bytes
//! that carry each game's ID.
#![cfg(unix)]

use romcrate_core::adb::{Adb, AdbFs};
use romcrate_core::gamesettings::BACKUP_SUFFIX;
use romcrate_core::pergame::{self, Choice, MANIFEST};
use std::fs;
use std::path::{Path, PathBuf};

const RA: &str = "storage/emulated/0/Android/data/com.retroarch/files";
const DOL: &str = "storage/emulated/0/Android/data/org.dolphinemu.dolphinemu/files";
const PSP: &str = "storage/emulated/0/PSP/SYSTEM";
const SD: &str = "storage/1A2B-3C4D/ROMs";

fn put(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join("device").join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, data).unwrap();
}
fn get(root: &Path, rel: &str) -> Option<String> {
    fs::read_to_string(root.join("device").join(rel.trim_start_matches('/'))).ok()
}

/// A minimal ISO-9660 image with the given root files.
fn tiny_iso(files: &[(&str, &[u8])]) -> Vec<u8> {
    let sec = 2048usize;
    let mut img = vec![0u8; sec * (20 + files.len() * 2)];
    let rec = |lba: u32, len: u32, name: &[u8], dir: bool| {
        let n = name.len();
        let mut r = vec![0u8; (33 + n + 1) & !1];
        r[0] = r.len() as u8;
        r[2..6].copy_from_slice(&lba.to_le_bytes());
        r[10..14].copy_from_slice(&len.to_le_bytes());
        r[25] = if dir { 2 } else { 0 };
        r[32] = n as u8;
        r[33..33 + n].copy_from_slice(name);
        r
    };
    // Primary volume descriptor at sector 16; root directory at sector 18.
    let pvd = &mut img[16 * sec..17 * sec];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    let root = rec(18, sec as u32, &[0], true);
    pvd[156..156 + root.len()].copy_from_slice(&root);
    img[17 * sec] = 255; // terminator
    img[17 * sec + 1..17 * sec + 6].copy_from_slice(b"CD001");
    let mut dir = Vec::new();
    dir.extend(rec(18, sec as u32, &[0], true));
    dir.extend(rec(18, sec as u32, &[1], true));
    for (i, (name, data)) in files.iter().enumerate() {
        let lba = 20 + i as u32 * 2;
        dir.extend(rec(lba, data.len() as u32, format!("{name};1").as_bytes(), false));
        let at = lba as usize * sec;
        img[at..at + data.len()].copy_from_slice(data);
    }
    img[18 * sec..18 * sec + dir.len()].copy_from_slice(&dir);
    img
}

fn with_id_at(off: usize, id: &str, size: usize) -> Vec<u8> {
    let mut b = vec![0u8; size];
    b[off..off + 6].copy_from_slice(id.as_bytes());
    b
}

fn handheld(root: &Path) -> Adb {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/simulate/fake_adb.py");
    let wrapper = root.join("adb");
    fs::write(&wrapper, format!(
        "#!/bin/sh\nFAKE_ADB_ROOT='{}' FAKE_ADB_PACKAGES='com.retroarch,org.dolphinemu.dolphinemu,org.ppsspp.ppsspp,come.nanodata.armsx2' exec python3 '{}' \"$@\"\n",
        root.join("device").display(), script.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let ra = "/storage/emulated/0/Android/data/com.retroarch/files";
    put(root, &format!("{RA}/retroarch.cfg"), format!(
        "video_driver = \"vulkan\"\nrgui_config_directory = \"{ra}/config\"\n").as_bytes());
    put(root, &format!("{RA}/retroarch-core-options.cfg"),
        b"swanstation_GPU_ResolutionScale = \"5\"\nswanstation_GPU_PGXPEnable = \"true\"\nmupen64plus-rdp-plugin = \"parallel\"\n");
    put(root, &format!("{DOL}/Config/Dolphin.ini"), b"[Core]\nGFXBackend = Vulkan\n");
    // The owner already has their own Zelda game file.
    put(root, &format!("{DOL}/GameSettings/RZDE01.ini"), b"[Video_Settings]\nInternalResolution = 1\n[Core]\nCPUThread = False\n");
    put(root, &format!("{PSP}/ppsspp.ini"), b"[Graphics]\nInternalResolution = 4\n");
    put(root, &format!("{PSP}/controls.ini"), b"[ControlMapping]\nCross = 10-96\nCircle = 10-97\n");
    // Games on the SD card.
    put(root, &format!("{SD}/psx/Crash.cue"), b"FILE \"Crash.bin\" BINARY\n");
    put(root, &format!("{SD}/psx/Crash.bin"), &[0u8; 4096]);
    put(root, &format!("{SD}/psx/Spyro.chd"), &[0u8; 4096]);
    put(root, &format!("{SD}/wii/Mario Kart Wii.rvz"), &with_id_at(0x58, "RMCE01", 70_000));
    put(root, &format!("{SD}/wii/Zelda.wbfs"), &with_id_at(0x200, "RZDE01", 70_000));
    put(root, &format!("{SD}/psp/Patapon.iso"), &tiny_iso(&[("UMD_DATA.BIN", b"UCUS-98711|0000000000000001|0001|G")]));
    put(root, &format!("{SD}/psp/God of War.cso"), &[0u8; 4096]);
    put(root, &format!("{SD}/ps2/Gran Turismo 4.iso"), &[0u8; 4096]);
    put(root, &format!("{SD}/ps2/Okami.iso"), &[0u8; 4096]);
    put(root, &format!("{SD}/n64/Mario Kart 64.z64"), &[0u8; 4096]);
    Adb { exe: wrapper }
}

fn dirs() -> Vec<(String, String)> {
    ["psx", "wii", "psp", "ps2", "n64"]
        .iter()
        .map(|s| (s.to_string(), format!("/{SD}/{s}")))
        .collect()
}

fn genre(_: &str, name: &str) -> Option<String> {
    match name {
        "Gran Turismo 4" => Some("Racing, Simulation".into()),
        "Okami" => Some("Action, Adventure".into()),
        _ => None,
    }
}

#[test]
fn finds_every_game_with_its_id_and_suggestion() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r);
    let mut dev = AdbFs { adb: &adb, serial: "FAKE0001" };
    let rows = pergame::scan(&mut dev, &dirs(), &genre);
    let row = |n: &str| rows.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n} missing: {:?}", rows.iter().map(|x| &x.name).collect::<Vec<_>>()));
    assert!(!rows.iter().any(|x| x.file.ends_with("Crash.bin")), "track .bin belongs to the .cue");
    let crash = row("Crash");
    assert_eq!(crash.console_value, "5");
    assert!(crash.settings_file.as_deref().unwrap().ends_with("/config/SwanStation/Crash.opt"));
    assert_eq!(row("Mario Kart Wii").id.as_deref(), Some("RMCE01"));
    assert!(row("Mario Kart Wii").settings_file.as_deref().unwrap().ends_with("/GameSettings/RMCE01.ini"));
    assert_eq!(row("Zelda").own_value.as_deref(), Some("1"), "reads the owner's existing per-game value");
    assert_eq!(row("Patapon").id.as_deref(), Some("UCUS98711"));
    assert!(row("Patapon").settings_file.as_deref().unwrap().ends_with("/PSP/SYSTEM/UCUS98711_ppsspp.ini"));
    assert!(row("God of War").note.as_deref().unwrap().contains(".cso"));
    assert!(row("God of War").settings_file.is_none());
    let gt4 = row("Gran Turismo 4").suggestion.clone().expect("racing → 2×");
    assert_eq!(gt4.value, "2");
    assert!(gt4.tested && gt4.why.contains("Racing"));
    assert!(row("Okami").suggestion.is_none());
    assert_eq!(row("Okami").kind, "checklist");
    assert_eq!(row("Mario Kart 64").console_value, "2x");
}

#[test]
fn per_game_files_are_written_safely_and_undo_puts_everything_back() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r);
    let mut dev = AdbFs { adb: &adb, serial: "FAKE0001" };
    let rows = pergame::scan(&mut dev, &dirs(), &genre);
    let file_of = |n: &str| rows.iter().find(|x| x.name == n).unwrap().settings_file.clone().unwrap();
    let choice = |sys: &str, n: &str, v: Option<&str>| Choice {
        system: sys.into(), file: n.into(), settings_file: file_of(n), value: v.map(String::from),
    };
    let zelda_before = get(r, &format!("{DOL}/GameSettings/RZDE01.ini")).unwrap();
    let res = pergame::apply(&mut dev, &[
        choice("psx", "Crash", Some("3")),
        choice("wii", "Mario Kart Wii", Some("1")),
        choice("wii", "Zelda", Some("3")),
        choice("psp", "Patapon", Some("2")),
    ]);
    assert_eq!(res.failed, 0, "{:?}", res.lines);

    // RetroArch game options start from the core's options and change one line.
    let crash = get(r, &file_of("Crash")).unwrap();
    assert!(crash.contains("swanstation_GPU_ResolutionScale = \"3\""));
    assert!(crash.contains("swanstation_GPU_PGXPEnable = \"true\""), "kept the other PS1 options: {crash}");
    assert!(!crash.contains("mupen64plus"), "no other core's options");
    // New Dolphin file; the owner's Zelda file changed with a backup and its other lines kept.
    assert!(get(r, &file_of("Mario Kart Wii")).unwrap().contains("[Video_Settings]\nInternalResolution = 1"));
    let zelda = get(r, &file_of("Zelda")).unwrap();
    assert!(zelda.contains("InternalResolution = 3") && zelda.contains("CPUThread = False"));
    assert_eq!(get(r, &format!("{}{BACKUP_SUFFIX}", file_of("Zelda"))).unwrap(), zelda_before);
    // PPSSPP game file carries the controls over, so they don't reset to default.
    let pat = get(r, &file_of("Patapon")).unwrap();
    assert!(pat.contains("[Graphics]\nInternalResolution = 2") && pat.contains("[ControlMapping]\nCross = 10-96"), "{pat}");
    let m = get(r, MANIFEST).unwrap();
    assert!(m.contains(&format!("created {}", file_of("Crash"))) && m.contains(&format!("changed {}", file_of("Zelda"))));

    // Rescan sees the new values.
    let rows2 = pergame::scan(&mut dev, &dirs(), &genre);
    assert_eq!(rows2.iter().find(|x| x.name == "Crash").unwrap().own_value.as_deref(), Some("3"));

    // One game back to the console setting: RomCrate's file is removed.
    let back = pergame::apply(&mut dev, &[choice("psx", "Crash", None)]);
    assert_eq!(back.failed, 0);
    assert!(get(r, &file_of("Crash")).is_none());

    // A path outside the emulator's settings folders is refused.
    let bad = pergame::apply(&mut dev, &[Choice { system: "wii".into(), file: "x".into(),
        settings_file: "/storage/emulated/0/DCIM/photo.ini".into(), value: Some("2".into()) }]);
    assert_eq!(bad.failed, 1);
    assert!(get(r, "storage/emulated/0/DCIM/photo.ini").is_none());

    // Undo everything.
    let u = pergame::undo_all(&mut dev);
    assert_eq!(u.failed, 0, "{:?}", u.lines);
    assert!(get(r, &file_of("Mario Kart Wii")).is_none());
    assert!(get(r, &file_of("Patapon")).is_none());
    assert_eq!(get(r, &file_of("Zelda")).unwrap(), zelda_before);
    assert!(get(r, &format!("{}{BACKUP_SUFFIX}", file_of("Zelda"))).is_none());
    assert!(!get(r, MANIFEST).unwrap().lines().any(|l| l.starts_with("created") || l.starts_with("changed")));
}
