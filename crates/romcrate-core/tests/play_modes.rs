//! Play modes end to end on the stand-in handheld: switch modes, check only the
//! run-ahead keys move, then Undo puts the owner's files back exactly.
#![cfg(unix)]

use romcrate_core::adb::{Adb, AdbFs};
use romcrate_core::gamesettings::{self, BACKUP_SUFFIX};
use romcrate_core::playmode;
use std::fs;
use std::path::{Path, PathBuf};

const RA: &str = "storage/emulated/0/Android/data/com.retroarch/files";

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join("device").join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
fn get(root: &Path, rel: &str) -> Option<String> {
    fs::read_to_string(root.join("device").join(rel)).ok()
}

fn handheld(root: &Path) -> Adb {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/simulate/fake_adb.py");
    let wrapper = root.join("adb");
    fs::write(&wrapper, format!(
        "#!/bin/sh\nFAKE_ADB_ROOT='{}' FAKE_ADB_PACKAGES='com.retroarch,org.dolphinemu.dolphinemu' exec python3 '{}' \"$@\"\n",
        root.join("device").display(), script.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let ra = "/storage/emulated/0/Android/data/com.retroarch/files";
    put(root, &format!("{RA}/retroarch.cfg"), &format!("video_driver = \"vulkan\"\nrgui_config_directory = \"{ra}/config\"\nrun_ahead_enabled = \"false\"\n"));
    // The owner's own SNES override.
    put(
        root,
        &format!("{RA}/config/Snes9x/Snes9x.cfg"),
        "input_overlay_enable = \"false\"\n",
    );
    Adb { exe: wrapper }
}

fn sys() -> Vec<String> {
    ["snes", "gba", "n64", "wii"].map(String::from).to_vec()
}

#[test]
fn switch_modes_then_undo() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r);
    let mut dev = AdbFs {
        adb: &adb,
        serial: "FAKE0001",
    };
    let before = get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg")).unwrap();

    // TV streamed + Bluetooth: two frames of run-ahead on SNES and GBA.
    let plan = playmode::plan(&mut dev, &sys(), "tv-stream", "bluetooth").unwrap();
    let st = |id: &str| plan.consoles.iter().find(|c| c.id == id).unwrap().clone();
    assert_eq!(st("snes").status, "ready");
    assert_eq!(st("gba").status, "ready");
    assert_eq!(
        st("n64").status,
        "checklist",
        "N64 is too heavy for run-ahead"
    );
    assert_eq!(st("wii").status, "checklist");
    assert!(st("snes").steps.iter().any(|s| s.contains("Streaming")));
    let ids: Vec<String> = plan.consoles.iter().map(|c| c.id.clone()).collect();
    let res = gamesettings::apply(&mut dev, &plan, &ids);
    assert_eq!(res.failed, 0, "{:?}", res.lines);
    playmode::record(&mut dev, "tv-stream", "bluetooth").unwrap();
    let snes = get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg")).unwrap();
    assert!(
        snes.contains("input_overlay_enable = \"false\""),
        "owner's line kept"
    );
    assert!(
        snes.contains("run_ahead_enabled = \"true\"") && snes.contains("run_ahead_frames = \"2\"")
    );
    assert!(snes.contains("video_frame_delay_auto = \"true\""));
    assert_eq!(
        get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg{BACKUP_SUFFIX}")).unwrap(),
        before
    );
    let gba = get(r, &format!("{RA}/config/mGBA/mGBA.cfg")).unwrap();
    assert!(
        gba.starts_with("# Written by RomCrate"),
        "new file is marked as RomCrate's"
    );
    assert!(
        get(
            r,
            &format!("{RA}/config/Mupen64Plus-Next/Mupen64Plus-Next.cfg")
        )
        .is_none(),
        "N64 untouched"
    );
    // The global retroarch.cfg is never touched.
    assert!(get(r, &format!("{RA}/retroarch.cfg"))
        .unwrap()
        .contains("run_ahead_enabled = \"false\""));
    assert_eq!(
        playmode::current(&mut dev),
        Some(("tv-stream".into(), "bluetooth".into()))
    );

    // Back to handheld with built-in controls: only the frame count changes.
    let plan = playmode::plan(&mut dev, &sys(), "handheld", "builtin").unwrap();
    let snes_plan = plan.consoles.iter().find(|c| c.id == "snes").unwrap();
    let keys: Vec<_> = snes_plan
        .changes
        .iter()
        .flat_map(|c| {
            c.keys
                .iter()
                .map(|k| (k.key.clone(), k.old.clone(), k.new.clone()))
        })
        .collect();
    assert_eq!(
        keys,
        vec![(
            "run_ahead_frames".to_string(),
            Some("2".to_string()),
            "1".to_string()
        )]
    );
    let res = gamesettings::apply(&mut dev, &plan, &ids);
    assert_eq!(res.failed, 0);
    assert!(get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg"))
        .unwrap()
        .contains("run_ahead_frames = \"1\""));
    // The backup is still the owner's original, not the in-between version.
    assert_eq!(
        get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg{BACKUP_SUFFIX}")).unwrap(),
        before
    );

    // Undo: owner's file back exactly, RomCrate's new file gone.
    let res = gamesettings::undo(&mut dev, &sys());
    assert_eq!(res.failed, 0, "{:?}", res.lines);
    assert_eq!(
        get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg")).unwrap(),
        before
    );
    assert!(get(r, &format!("{RA}/config/mGBA/mGBA.cfg")).is_none());
}
