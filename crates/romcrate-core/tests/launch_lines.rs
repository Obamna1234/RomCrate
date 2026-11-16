//! Pegasus launch lines end to end: new files, existing files (untouched unless
//! the user opts in), known-bad lines, and one system split across two drives.

use romcrate_core::launch::{self, LaunchChoice};
use romcrate_core::launchers::{self, LauncherKind};
use romcrate_core::{scan, systems};
use std::fs;
use std::path::Path;

fn android() -> LaunchChoice {
    LaunchChoice {
        platform: Some("android".into()),
        ..Default::default()
    }
}

fn write(p: &Path, b: &[u8]) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b).unwrap();
}

fn finalize(dir: &Path, system: &str, choice: &LaunchChoice) -> Vec<String> {
    let sys = systems::by_id(system).unwrap();
    let ss = scan::scan_system_folder(sys, dir);
    launchers::finalize_system_with(LauncherKind::Pegasus, None, sys, &ss, &[], false, choice)
        .unwrap()
}

const ARMSX2_BLOCK: &str = "launch: am start\n  -n come.nanodata.armsx2/com.armsx2.MainActivity\n  -a android.intent.action.VIEW\n  -d \"{file.uri}\"\n  --activity-clear-task\n  --activity-clear-top\n  --activity-no-history\n";

#[test]
fn new_ps2_file_gets_the_tested_armsx2_line() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("ps2");
    write(&dir.join("Manhunt 2 (USA).chd"), b"x");
    write(&dir.join("Scarface (USA).iso"), b"x");
    let notes = finalize(&dir, "ps2", &android());
    let meta = fs::read_to_string(dir.join("metadata.pegasus.txt")).unwrap();
    assert!(meta.contains(ARMSX2_BLOCK), "{meta}");
    assert_eq!(meta.matches("launch:").count(), 1);
    let exts = meta.lines().find(|l| l.starts_with("extensions:")).unwrap();
    assert!(exts.contains("chd") && exts.contains("iso"), "{exts}");
    assert!(notes[0].contains("tested launch line"), "{notes:?}");
}

#[test]
fn untested_emulators_are_commented_out() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("snes");
    write(&dir.join("Super Mario World (USA).sfc"), b"x");
    let notes = finalize(&dir, "snes", &android());
    let meta = fs::read_to_string(dir.join("metadata.pegasus.txt")).unwrap();
    assert!(!launch::has_launch(&meta), "{meta}");
    assert!(
        meta.contains("# launch: am start\n#   -n com.retroarch/"),
        "{meta}"
    );
    assert!(meta.contains("NOT tested"));
    assert!(notes[0].contains("commented out"));
}

#[test]
fn no_launch_choice_keeps_the_old_placeholder() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("ps2");
    write(&dir.join("A.iso"), b"x");
    finalize(&dir, "ps2", &LaunchChoice::default());
    let meta = fs::read_to_string(dir.join("metadata.pegasus.txt")).unwrap();
    assert!(!launch::has_launch(&meta));
    assert!(meta.contains("# launch: retroarch"));
}

#[test]
fn existing_file_is_byte_for_byte_unchanged_without_opt_in() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("ps2");
    write(&dir.join("A.chd"), b"x");
    let mine = "collection: PS2 (mine)\r\nextensions: iso\r\n\r\ngame: A\r\nfile: A.chd\r\nsummary: my notes ✓\r\n";
    write(&dir.join("metadata.pegasus.txt"), mine.as_bytes());
    let notes = finalize(&dir, "ps2", &android());
    assert_eq!(
        fs::read(dir.join("metadata.pegasus.txt")).unwrap(),
        mine.as_bytes()
    );
    assert!(!dir.join("metadata.pegasus.txt.bak").exists());
    let all = notes.join("\n");
    assert!(all.contains("no launch line"), "{all}");
    assert!(
        all.contains(ARMSX2_BLOCK.trim_end()),
        "exact block shown: {all}"
    );
    assert!(all.contains("doesn't list .chd"), "chd warning: {all}");
}

#[test]
fn opt_in_adds_only_the_launch_block_and_keeps_a_backup() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("ps2");
    write(&dir.join("A.chd"), b"x");
    let mine = "collection: PlayStation 2\nextensions: iso, chd\n\ngame: A\nfile: A.chd\n";
    write(&dir.join("metadata.pegasus.txt"), mine.as_bytes());
    let choice = LaunchChoice {
        fix_existing: true,
        ..android()
    };
    let notes = finalize(&dir, "ps2", &choice);
    let new = fs::read_to_string(dir.join("metadata.pegasus.txt")).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("metadata.pegasus.txt.bak")).unwrap(),
        mine
    );
    let block = launch::render_block(launch::profile("armsx2-playstore").unwrap());
    assert_eq!(
        new.replacen(&block, "", 1),
        mine,
        "only the launch block was added"
    );
    assert!(new.starts_with("collection: PlayStation 2\nextensions: iso, chd\n# Launch line"));
    assert!(notes.join(" ").contains("metadata.pegasus.txt.bak"));
    // Running again changes nothing and makes no second backup.
    finalize(&dir, "ps2", &choice);
    assert_eq!(
        fs::read_to_string(dir.join("metadata.pegasus.txt")).unwrap(),
        new
    );
    assert!(!dir.join("metadata.pegasus.txt.bak2").exists());
}

#[test]
fn known_bad_armsx2_line_is_flagged_and_fixed_on_request() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("ps2");
    write(&dir.join("A.chd"), b"x");
    let mine = "collection: PS2\nextensions: chd\nlaunch: am start\n  -n com.armsx2/com.armsx2.MainActivity\n  -a android.intent.action.VIEW\n  -d \"{file.uri}\"\n\ngame: A\nfile: A.chd\n";
    write(&dir.join("metadata.pegasus.txt"), mine.as_bytes());
    let notes = finalize(&dir, "ps2", &android());
    assert_eq!(
        fs::read_to_string(dir.join("metadata.pegasus.txt")).unwrap(),
        mine
    );
    assert!(notes.join(" ").contains("Activity not found"), "{notes:?}");
    finalize(
        &dir,
        "ps2",
        &LaunchChoice {
            fix_existing: true,
            ..android()
        },
    );
    let new = fs::read_to_string(dir.join("metadata.pegasus.txt")).unwrap();
    assert!(new.contains(ARMSX2_BLOCK));
    assert!(!new.contains("com.armsx2/com.armsx2.MainActivity"));
    assert!(new.contains("game: A\nfile: A.chd\n"));
    assert_eq!(
        fs::read_to_string(dir.join("metadata.pegasus.txt.bak")).unwrap(),
        mine
    );
}

#[test]
fn ps2_split_across_sd_card_and_internal_storage() {
    let t = tempfile::tempdir().unwrap();
    let sd = t.path().join("SD/ROMs");
    let internal = t.path().join("Internal/ROMs");
    write(&sd.join("ps2/Light Game (USA).chd"), b"x");
    write(&internal.join("ps2/Heavy Game (USA).chd"), b"x");
    let lib = scan::scan_roots(&[sd.clone(), internal.clone()]);
    let ps2: Vec<_> = lib
        .systems
        .iter()
        .filter(|s| s.system_id == "ps2")
        .collect();
    assert_eq!(ps2.len(), 2, "both folders found");
    let sys = systems::by_id("ps2").unwrap();
    for ss in &ps2 {
        launchers::finalize_system_with(
            LauncherKind::Pegasus,
            None,
            sys,
            ss,
            &[],
            false,
            &android(),
        )
        .unwrap();
    }
    let a = fs::read_to_string(sd.join("ps2/metadata.pegasus.txt")).unwrap();
    let b = fs::read_to_string(internal.join("ps2/metadata.pegasus.txt")).unwrap();
    let header = |m: &str| -> String {
        m.lines()
            .take_while(|l| !l.starts_with("game:"))
            .filter(|l| {
                l.starts_with("collection:") || l.starts_with("launch:") || l.starts_with("  ")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        header(&a),
        header(&b),
        "same collection name + launch block so Pegasus merges them"
    );
    assert!(a.contains("Light Game") && !a.contains("Heavy Game"));
    assert!(b.contains("Heavy Game") && !b.contains("Light Game"));
}

#[test]
fn launch_lines_only_task_writes_every_system() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("ROMs");
    write(&root.join("ps2/A.chd"), b"x");
    write(&root.join("snes/B.sfc"), b"x");
    let mine = "collection: PS2\nextensions: chd\n\ngame: A\nfile: A.chd\n";
    let lib = scan::scan_roots(std::slice::from_ref(&root));
    // Existing PS2 file + opt-in → launch block added; SNES file created.
    write(&root.join("ps2/metadata.pegasus.txt"), mine.as_bytes());
    let notes = launchers::write_launch_lines(
        &lib,
        &LaunchChoice {
            fix_existing: true,
            ..android()
        },
    );
    assert!(fs::read_to_string(root.join("ps2/metadata.pegasus.txt"))
        .unwrap()
        .contains(ARMSX2_BLOCK));
    assert!(root.join("ps2/metadata.pegasus.txt.bak").exists());
    assert!(root.join("snes/metadata.pegasus.txt").exists());
    assert!(
        notes.iter().any(|n| n.contains("Super Nintendo")),
        "{notes:?}"
    );
}

#[test]
fn launch_lines_task_says_when_there_is_nothing_to_do() {
    let t = tempfile::tempdir().unwrap();
    write(&t.path().join("n64/empty.z64"), b"");
    let lib = romcrate_core::scan::scan_roots(&[t.path().to_path_buf()]);
    let notes = romcrate_core::launchers::write_launch_lines(&lib, &android());
    assert_eq!(notes.len(), 1);
    assert!(notes[0].starts_with("No games found"), "{notes:?}");
    assert!(!t.path().join("n64/metadata.pegasus.txt").exists());
}
