//! Game settings end to end, against the stand-in handheld (tools/simulate/fake_adb.py)
//! holding realistic RetroArch / Dolphin / PPSSPP settings files.
#![cfg(unix)]

use romcrate_core::adb::{Adb, AdbFs};
use romcrate_core::gamesettings::{self, BACKUP_SUFFIX, MARKER};
use std::fs;
use std::path::{Path, PathBuf};

const RA: &str = "storage/emulated/0/Android/data/com.retroarch/files";
const DOLPHIN: &str = "storage/emulated/0/Android/data/org.dolphinemu.dolphinemu/files/Config";
const PPSSPP: &str = "storage/emulated/0/PSP/SYSTEM/ppsspp.ini";

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join("device").join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
fn get(root: &Path, rel: &str) -> Option<String> {
    fs::read_to_string(root.join("device").join(rel)).ok()
}

fn handheld(root: &Path, driver: &str) -> Adb {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/simulate/fake_adb.py");
    let wrapper = root.join("adb");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nFAKE_ADB_ROOT='{}' FAKE_ADB_PACKAGES='com.retroarch,org.dolphinemu.dolphinemu,org.ppsspp.ppsspp,me.magnum.melonds' exec python3 '{}' \"$@\"\n",
            root.join("device").display(),
            script.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let ra = "/storage/emulated/0/Android/data/com.retroarch/files";
    put(root, &format!("{RA}/retroarch.cfg"), &format!(
        "video_driver = \"{driver}\"\nrgui_config_directory = \"{ra}/config\"\nvideo_shader_dir = \"{ra}/shaders\"\ncore_options_path = \"\"\nvideo_vsync = \"true\"\n"));
    put(root, &format!("{RA}/retroarch-core-options.cfg"),
        "mupen64plus-rdp-plugin = \"gliden64\"\nmupen64plus-43screensize = \"960x720\"\nsnes9x_overclock = \"disabled\"\n");
    put(
        root,
        &format!("{RA}/shaders/shaders_slang/interpolation/quilez.slangp"),
        "shaders = 1\n",
    );
    // The owner already has a SNES override with a key of their own.
    put(
        root,
        &format!("{RA}/config/Snes9x/Snes9x.cfg"),
        "input_overlay_enable = \"false\"\nvideo_vsync = \"true\"\n",
    );
    put(root, &format!("{DOLPHIN}/Dolphin.ini"), "[General]\nISOPath0 = /storage/1A2B-3C4D/ROMs/wii\n[Core]\nCPUThread = True\nGFXBackend = OGL\n");
    put(
        root,
        &format!("{DOLPHIN}/GFX.ini"),
        "[Settings]\nInternalResolution = 1\nShowFPS = True\n",
    );
    put(root, PPSSPP, "[General]\nLanguage = en_US\n[Graphics]\nGraphicsBackend = 0 (OPENGL)\nInternalResolution = 2\nAnisotropyLevel = 4\n");
    Adb { exe: wrapper }
}

fn systems() -> Vec<String> {
    ["snes", "n64", "wii", "psp", "ps2", "nds", "gba"]
        .map(String::from)
        .to_vec()
}

#[test]
fn applies_only_our_keys_with_backups_then_undo_puts_everything_back() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "vulkan");
    let mut dev = AdbFs {
        adb: &adb,
        serial: "FAKE0001",
    };
    let before_dolphin = get(r, &format!("{DOLPHIN}/Dolphin.ini")).unwrap();
    let before_snes = get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg")).unwrap();
    let before_ppsspp = get(r, PPSSPP).unwrap();

    let plan = gamesettings::plan(&mut dev, &systems());
    let status = |id: &str| {
        plan.consoles
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .status
            .clone()
    };
    assert_eq!(status("snes"), "ready");
    assert_eq!(status("n64"), "ready");
    assert_eq!(status("wii"), "ready");
    assert_eq!(status("psp"), "ready");
    assert_eq!(status("gba"), "ready");
    assert_eq!(status("ps2"), "checklist");
    assert_eq!(status("nds"), "checklist");
    // SNES: VSync was already on, so it isn't listed as a change.
    let snes = plan.consoles.iter().find(|c| c.id == "snes").unwrap();
    let keys: Vec<_> = snes
        .changes
        .iter()
        .flat_map(|c| c.keys.iter().map(|k| k.key.as_str()))
        .collect();
    assert!(
        !keys.contains(&"video_vsync")
            && keys.contains(&"aspect_ratio_index")
            && keys.contains(&"shader preset"),
        "{keys:?}"
    );

    let ids: Vec<String> = plan.consoles.iter().map(|c| c.id.clone()).collect();
    let res = gamesettings::apply(&mut dev, &plan, &ids);
    assert_eq!(res.failed, 0, "{:?}", res.lines);

    // Only our keys changed; the owner's own lines are still there.
    let snes_cfg = get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg")).unwrap();
    assert!(
        snes_cfg.contains("input_overlay_enable = \"false\"")
            && snes_cfg.contains("aspect_ratio_index = \"0\"")
    );
    assert_eq!(
        get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg{BACKUP_SUFFIX}")).unwrap(),
        before_snes
    );
    let preset = get(r, &format!("{RA}/config/Snes9x/Snes9x.slangp")).unwrap();
    assert!(preset.contains("#reference \"/storage/emulated/0/Android/data/com.retroarch/files/shaders/shaders_slang/interpolation/quilez.slangp\""));
    let dol = get(r, &format!("{DOLPHIN}/Dolphin.ini")).unwrap();
    assert_eq!(
        dol,
        before_dolphin.replace("GFXBackend = OGL", "GFXBackend = Vulkan"),
        "ISO path and other keys untouched"
    );
    assert!(get(r, &format!("{DOLPHIN}/GFX.ini"))
        .unwrap()
        .contains("InternalResolution = 2\nShowFPS = True"));
    let pp = get(r, PPSSPP).unwrap();
    assert!(
        pp.contains("GraphicsBackend = 3 (VULKAN)")
            && pp.contains("InternalResolution = 4")
            && pp.contains("Language = en_US")
    );
    // New N64 options file starts with this core's existing shared options.
    let opt = get(
        r,
        &format!("{RA}/config/Mupen64Plus-Next/Mupen64Plus-Next.opt"),
    )
    .unwrap();
    assert!(opt.starts_with(MARKER));
    assert!(
        opt.contains("mupen64plus-43screensize = \"960x720\""),
        "kept the owner's other N64 options: {opt}"
    );
    assert!(opt.contains("mupen64plus-rdp-plugin = \"parallel\"") && !opt.contains("snes9x_"));
    // The emulators were closed first; melonDS (checklist) wasn't.
    let stopped = fs::read_to_string(r.join("device/force-stopped.log")).unwrap();
    for p in [
        "com.retroarch",
        "org.dolphinemu.dolphinemu",
        "org.ppsspp.ppsspp",
    ] {
        assert!(stopped.contains(p), "{stopped}");
    }
    assert!(!stopped.contains("melonds"));

    // Running again: nothing left to change.
    let again = gamesettings::plan(&mut dev, &systems());
    assert!(
        again
            .consoles
            .iter()
            .filter(|c| c.status != "checklist")
            .all(|c| c.status == "already"),
        "{:?}",
        again
            .consoles
            .iter()
            .map(|c| (&c.id, &c.status))
            .collect::<Vec<_>>()
    );

    // Undo: originals back, RomCrate's own files gone, no backups left.
    let u = gamesettings::undo(&mut dev, &systems());
    assert_eq!(u.failed, 0, "{:?}", u.lines);
    assert_eq!(
        get(r, &format!("{DOLPHIN}/Dolphin.ini")).unwrap(),
        before_dolphin
    );
    assert_eq!(
        get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg")).unwrap(),
        before_snes
    );
    assert_eq!(get(r, PPSSPP).unwrap(), before_ppsspp);
    assert!(get(r, &format!("{RA}/config/Snes9x/Snes9x.slangp")).is_none());
    assert!(get(
        r,
        &format!("{RA}/config/Mupen64Plus-Next/Mupen64Plus-Next.opt")
    )
    .is_none());
    assert!(get(r, &format!("{RA}/config/mGBA/mGBA.cfg")).is_none());
    assert!(get(r, &format!("{DOLPHIN}/Dolphin.ini{BACKUP_SUFFIX}")).is_none());
}

#[test]
fn opengl_retroarch_leaves_n64_alone_and_missing_shaders_become_a_step() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "gl");
    let mut dev = AdbFs {
        adb: &adb,
        serial: "FAKE0001",
    };
    let plan = gamesettings::plan(&mut dev, &["n64".into(), "snes".into()]);
    let n64 = plan.consoles.iter().find(|c| c.id == "n64").unwrap();
    assert_eq!(
        n64.status, "already",
        "nothing to change with the OpenGL driver"
    );
    assert!(n64.message.as_deref().unwrap().contains("OpenGL"));
    let snes = plan.consoles.iter().find(|c| c.id == "snes").unwrap();
    assert!(
        snes.steps.iter().any(|s| s.contains("Update GLSL Shaders")),
        "{:?}",
        snes.steps
    );
}

#[test]
fn emulators_that_are_missing_or_never_opened_are_explained() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "vulkan");
    fs::remove_file(r.join("device").join(PPSSPP)).unwrap();
    let mut dev = AdbFs {
        adb: &adb,
        serial: "FAKE0001",
    };
    let plan = gamesettings::plan(&mut dev, &["psp".into(), "psx".into()]);
    let psp = plan.consoles.iter().find(|c| c.id == "psp").unwrap();
    assert_eq!(psp.status, "problem");
    assert!(psp.message.as_deref().unwrap().contains("Open PPSSPP once"));
    // PS1 uses RetroArch (installed): fine.
    assert_eq!(
        plan.consoles.iter().find(|c| c.id == "psx").unwrap().status,
        "ready"
    );
}
