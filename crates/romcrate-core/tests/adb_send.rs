//! End-to-end: send a console folder to a stand-in handheld (tools/simulate/fake_adb.py),
//! which keeps its storage in a temp folder. Unix only (the stand-in is a script).
#![cfg(unix)]

use romcrate_core::adb::{self, Adb, SendEvent, Target};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// Tests here change environment variables, so run them one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn stand_in(root: &Path, state: &str) -> Adb {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/simulate/fake_adb.py");
    let wrapper = root.join("adb");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nFAKE_ADB_ROOT='{}' FAKE_ADB_STATE='{state}' exec python3 '{}' \"$@\"\n",
            root.join("device").display(),
            script.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(root.join("device/storage/emulated/0")).unwrap();
    fs::create_dir_all(root.join("device/storage/1A2B-3C4D/ROMs/PS2")).unwrap();
    Adb { exe: wrapper }
}

fn game(p: &Path, bytes: usize, seed: u8) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(
        p,
        (0..bytes)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

#[test]
fn sends_to_the_sd_card_checks_every_game_and_skips_on_the_second_run() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    let a = stand_in(t.path(), "device");
    let pc = t.path().join("pc/ROMs");
    game(
        &pc.join("ps2/Ratchet & Clank (USA).iso"),
        3 * 1024 * 1024,
        1,
    );
    game(
        &pc.join("ps2/media/box2dfront/Ratchet & Clank (USA).png"),
        5000,
        2,
    );
    fs::write(
        pc.join("ps2/metadata.pegasus.txt"),
        "collection: PlayStation 2\n",
    )
    .unwrap();
    game(
        &pc.join("snes/Itchy & Scratchy's Game.smc"),
        2 * 1024 * 1024,
        3,
    );

    let devs = a.devices().unwrap();
    assert_eq!(devs[0].state, "device");
    assert_eq!(devs[0].model.as_deref(), Some("Test Handheld"));
    let vols = a.volumes("FAKE0001").unwrap();
    let sd = vols
        .iter()
        .find(|v| v.label.starts_with("SD card"))
        .expect("SD card listed");
    assert_eq!(sd.roms_dir.as_deref(), Some("ROMs"));
    assert!(sd.console_folders.contains(&"PS2".to_string()));
    let internal = vols.iter().find(|v| v.label == "Internal storage").unwrap();
    assert!(internal.roms_dir.is_none());

    let targets = vec![
        Target {
            local: pc.join("ps2"),
            system_id: "ps2".into(),
            volume: sd.path.clone(),
        },
        Target {
            local: pc.join("snes"),
            system_id: "snes".into(),
            volume: internal.path.clone(),
        },
    ];
    let plan = adb::plan(&targets, &vols, &mut |d| {
        a.list_files("FAKE0001", d).unwrap()
    })
    .unwrap();
    assert_eq!(plan.files.len(), 4);
    let events = Mutex::new(Vec::new());
    let sum = adb::send(
        &a,
        "FAKE0001",
        &plan,
        &|e| events.lock().unwrap().push(e),
        &|| false,
    );
    assert_eq!(
        (sum.copied, sum.verified, sum.failed),
        (4, 2, 0),
        "{:?}",
        events.lock().unwrap()
    );

    let dev = t.path().join("device/storage");
    let iso = dev.join("1A2B-3C4D/ROMs/PS2/Ratchet & Clank (USA).iso");
    assert_eq!(
        fs::read(&iso).unwrap(),
        fs::read(pc.join("ps2/Ratchet & Clank (USA).iso")).unwrap(),
        "into the existing PS2 folder"
    );
    assert!(dev
        .join("1A2B-3C4D/ROMs/PS2/media/box2dfront/Ratchet & Clank (USA).png")
        .exists());
    assert!(
        dev.join("emulated/0/ROMs/snes/Itchy & Scratchy's Game.smc")
            .exists(),
        "ROMs/snes made on internal storage"
    );

    // Second run: everything is already there.
    let vols = a.volumes("FAKE0001").unwrap();
    let again = adb::plan(&targets, &vols, &mut |d| {
        a.list_files("FAKE0001", d).unwrap()
    })
    .unwrap();
    assert_eq!((again.files.len(), again.already_there), (0, 4));
}

#[test]
fn a_damaged_copy_is_caught_and_removed() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    let a = stand_in(t.path(), "device");
    let pc = t.path().join("pc/ROMs");
    game(&pc.join("n64/Mario Kart 64 (USA).z64"), 2 * 1024 * 1024, 7);
    std::env::set_var("FAKE_ADB_CORRUPT", "Mario Kart");
    let vols = a.volumes("FAKE0001").unwrap();
    let targets = vec![Target {
        local: pc.join("n64"),
        system_id: "n64".into(),
        volume: "/storage/emulated/0".into(),
    }];
    let plan = adb::plan(&targets, &vols, &mut |d| {
        a.list_files("FAKE0001", d).unwrap()
    })
    .unwrap();
    let msgs = Mutex::new(Vec::new());
    let sum = adb::send(
        &a,
        "FAKE0001",
        &plan,
        &|e| {
            if let SendEvent::File { message, .. } = e {
                msgs.lock().unwrap().push(message)
            }
        },
        &|| false,
    );
    std::env::remove_var("FAKE_ADB_CORRUPT");
    assert_eq!((sum.copied, sum.failed), (0, 1));
    assert!(
        msgs.lock().unwrap()[0].contains("didn't match"),
        "{:?}",
        msgs.lock().unwrap()
    );
    assert!(
        !t.path()
            .join("device/storage/emulated/0/ROMs/n64/Mario Kart 64 (USA).z64")
            .exists(),
        "bad copy removed"
    );
}

#[test]
fn an_sd_card_android_wont_let_us_write_to_is_caught_before_copying() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    let a = stand_in(t.path(), "device");
    std::env::set_var("FAKE_ADB_READONLY", "/storage/1A2B-3C4D");
    let sd = a.can_write("FAKE0001", "/storage/1A2B-3C4D");
    let internal = a.can_write("FAKE0001", "/storage/emulated/0");
    std::env::remove_var("FAKE_ADB_READONLY");
    let e = sd.unwrap_err().to_string();
    assert!(
        e.contains("Choose Internal storage") && e.contains("card reader"),
        "{e}"
    );
    assert!(internal.is_ok());
    assert!(
        !t.path()
            .join("device/storage/emulated/0/.romcrate-write-test")
            .exists(),
        "test file cleaned up"
    );
}

#[test]
fn unauthorized_handheld_gets_a_clear_message() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    let a = stand_in(t.path(), "unauthorized");
    assert_eq!(a.devices().unwrap()[0].state, "unauthorized");
    let e = a.volumes("FAKE0001").unwrap_err().to_string();
    assert!(e.contains("tap \"Allow\""), "{e}");
}

#[test]
fn a_file_that_appears_after_the_check_is_never_replaced_or_removed() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    let a = stand_in(t.path(), "device");
    let pc = t.path().join("pc/ROMs");
    game(&pc.join("n64/Homebrew Racer.z64"), 2 * 1024 * 1024, 7);
    game(&pc.join("n64/Other.z64"), 4096, 8);
    let vols = a.volumes("FAKE0001").unwrap();
    let targets = vec![Target {
        local: pc.join("n64"),
        system_id: "n64".into(),
        volume: "/storage/emulated/0".into(),
    }];
    let plan = adb::plan(&targets, &vols, &mut |d| {
        a.list_files("FAKE0001", d).unwrap()
    })
    .unwrap();
    assert_eq!(plan.files.len(), 2);
    // Meanwhile, the person copies their own file there by hand. And this copy
    // would come out damaged, which used to mean "remove what's there".
    let theirs = t
        .path()
        .join("device/storage/emulated/0/ROMs/n64/Homebrew Racer.z64");
    fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    fs::write(&theirs, b"their own file").unwrap();
    std::env::set_var("FAKE_ADB_CORRUPT", "Homebrew Racer");
    let sum = adb::send(&a, "FAKE0001", &plan, &|_| {}, &|| false);
    std::env::remove_var("FAKE_ADB_CORRUPT");
    assert_eq!(fs::read(&theirs).unwrap(), b"their own file");
    assert_eq!((sum.copied, sum.failed, sum.conflicts), (1, 0, 1));
}
