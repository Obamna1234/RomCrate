//! Picture styles (spec for v0.7.0), on the stand-in handheld: console styles,
//! the Vulkan/GL split, missing shaders, your own presets, per-game styles, Undo.
#![cfg(unix)]

use romcrate_core::adb::{Adb, AdbFs};
use romcrate_core::gamesettings::{self, Profile, MARKER};
use romcrate_core::picstyle;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const RA: &str = "storage/emulated/0/Android/data/com.retroarch/files";
const RA_ABS: &str = "/storage/emulated/0/Android/data/com.retroarch/files";
const SNES: &str = "storage/1A2B-3C4D/ROMs/snes";

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join("device").join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
fn get(root: &Path, rel: &str) -> Option<String> {
    fs::read_to_string(root.join("device").join(rel)).ok()
}

/// A handheld with RetroArch on `driver`, the shader packs (unless `shaders` is
/// false), two SNES games, and the owner's own SNES override.
fn handheld(root: &Path, driver: &str, shaders: bool) -> Adb {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/simulate/fake_adb.py");
    let wrapper = root.join("adb");
    fs::write(&wrapper, format!(
        "#!/bin/sh\nFAKE_ADB_ROOT='{}' FAKE_ADB_PACKAGES='com.retroarch' exec python3 '{}' \"$@\"\n",
        root.join("device").display(), script.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    put(root, &format!("{RA}/retroarch.cfg"), &format!(
        "video_driver = \"{driver}\"\nrgui_config_directory = \"{RA_ABS}/config\"\nvideo_shader_dir = \"{RA_ABS}/shaders\"\nvideo_smooth = \"true\"\n"));
    put(root, &format!("{RA}/config/Snes9x/Snes9x.cfg"), "input_overlay_enable = \"false\"\n");
    if shaders {
        for f in [
            "shaders_slang/interpolation/quilez.slangp",
            "shaders_slang/edge-smoothing/scalefx/scalefx+rAA.slangp",
            "shaders_slang/crt/crt-easymode.slangp",
            "shaders_slang/crt/crt-guest-advanced.slangp",
            "shaders_slang/stock.slang",
            "shaders_glsl/interpolation/quilez.glslp",
            "shaders_glsl/scalefx/scalefx+rAA.glslp",
            "shaders_glsl/crt/crt-easymode.glslp",
            "shaders_glsl/stock.glsl",
        ] {
            put(root, &format!("{RA}/shaders/{f}"), "shaders = 1\n");
        }
    }
    put(root, &format!("{SNES}/Donkey Kong Country (USA).sfc"), "x");
    put(root, &format!("{SNES}/Super Mario World (USA).sfc"), "x");
    Adb { exe: wrapper }
}

fn styled(dev: &mut AdbFs, snes: &str) -> gamesettings::SettingsPlan {
    let mut m = BTreeMap::new();
    m.insert("snes".to_string(), snes.to_string());
    gamesettings::plan_styled(dev, &["snes".to_string()], &m)
}

fn snes(plan: &gamesettings::SettingsPlan) -> gamesettings::ConsolePlan {
    plan.consoles.iter().find(|c| c.id == "snes").unwrap().clone()
}

fn apply(dev: &mut AdbFs, plan: &gamesettings::SettingsPlan) {
    let r = gamesettings::apply(dev, plan, &["snes".to_string()]);
    assert_eq!(r.failed, 0, "{:?}", r.lines);
}

/// 1. Vulkan + Smooth HD: ScaleFX preset, RetroArch smoothing off, retroarch.cfg untouched.
#[test]
fn vulkan_smooth() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "vulkan", true);
    let mut dev = AdbFs { adb: &adb, serial: "FAKE0001" };
    let global = get(r, &format!("{RA}/retroarch.cfg")).unwrap();
    let plan = styled(&mut dev, "smooth");
    let c = snes(&plan);
    assert_eq!(c.style.as_deref(), Some("smooth"));
    assert!(c.styles.iter().any(|s| s.id == "crt-best" && s.available), "best CRT offered on Vulkan");
    let keys: Vec<_> = c.changes.iter().flat_map(|f| f.keys.iter().map(|k| k.key.clone())).collect();
    assert!(keys.contains(&"video_smooth".to_string()) && keys.contains(&"shader preset".to_string()), "{keys:?}");
    apply(&mut dev, &plan);
    let preset = get(r, &format!("{RA}/config/Snes9x/Snes9x.slangp")).unwrap();
    assert!(preset.starts_with(MARKER));
    assert!(preset.contains(&format!("#reference \"{RA_ABS}/shaders/shaders_slang/edge-smoothing/scalefx/scalefx+rAA.slangp\"")), "{preset}");
    let cfg = get(r, &format!("{RA}/config/Snes9x/Snes9x.cfg")).unwrap();
    assert!(cfg.contains("video_smooth = \"false\"") && cfg.contains("input_overlay_enable = \"false\""), "{cfg}");
    assert_eq!(get(r, &format!("{RA}/retroarch.cfg")).unwrap(), global, "global config untouched");
}

/// 2. GL + Smooth HD: the GLSL ScaleFX preset.  3. GL: best CRT isn't usable.
#[test]
fn gl_smooth_and_no_best_crt() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "gl", true);
    let mut dev = AdbFs { adb: &adb, serial: "FAKE0001" };
    let plan = styled(&mut dev, "smooth");
    apply(&mut dev, &plan);
    let preset = get(r, &format!("{RA}/config/Snes9x/Snes9x.glslp")).unwrap();
    assert!(preset.contains("shaders_glsl/scalefx/scalefx+rAA.glslp"), "{preset}");
    assert!(get(r, &format!("{RA}/config/Snes9x/Snes9x.slangp")).is_none());

    let plan = styled(&mut dev, "crt-best");
    let c = snes(&plan);
    assert!(c.styles.iter().any(|s| s.id == "crt-best" && !s.available), "shown as not available on GL");
    assert!(c.steps.iter().any(|s| s.contains("needs RetroArch's Vulkan video driver")), "{:?}", c.steps);
    assert!(!c.changes.iter().any(|f| f.path.ends_with(".glslp")), "nothing written for an unusable style");
    assert!(!c.changes.iter().flat_map(|f| &f.keys).any(|k| k.key == "video_smooth"), "no smoothing change either");
}

/// 4. Shaders not downloaded: the Online Updater step, no preset written.
#[test]
fn missing_shaders_give_a_step() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "vulkan", false);
    let mut dev = AdbFs { adb: &adb, serial: "FAKE0001" };
    let c = snes(&styled(&mut dev, "crt"));
    assert!(c.steps.iter().any(|s| s.contains("Online Updater → Update Slang Shaders")), "{:?}", c.steps);
    assert!(!c.changes.iter().any(|f| f.path.ends_with(".slangp")));
}

/// 5. Switching styles replaces RomCrate's preset; "No shader" removes it; your
///    own preset is never replaced.
#[test]
fn switching_styles_and_your_own_preset() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "vulkan", true);
    let mut dev = AdbFs { adb: &adb, serial: "FAKE0001" };
    let p = format!("{RA}/config/Snes9x/Snes9x.slangp");
    let pl = styled(&mut dev, "smooth");
    apply(&mut dev, &pl);
    let plan = styled(&mut dev, "sharp");
    let k = snes(&plan).changes.iter().flat_map(|f| f.keys.clone()).find(|k| k.key == "shader preset").unwrap();
    assert_eq!((k.old.as_deref(), k.new.as_str()), (Some("Smooth HD"), "Sharp pixels"), "shows what it was");
    apply(&mut dev, &plan);
    assert!(get(r, &p).unwrap().contains("interpolation/quilez.slangp"));
    let pl = styled(&mut dev, "none");
    apply(&mut dev, &pl);
    assert!(get(r, &p).is_none(), "No shader removes RomCrate's preset");

    // Your own preset: kept whatever style is picked.
    put(r, &p, "#reference \"my/own.slangp\"\n");
    for st in ["smooth", "sharp", "none"] {
        let c = snes(&styled(&mut dev, st));
        assert!(!c.changes.iter().any(|f| f.path.ends_with(".slangp")), "{st}");
        assert!(c.steps.iter().any(|s| s.contains("Your own shader preset is kept")), "{st}");
        let pl = styled(&mut dev, st);
    apply(&mut dev, &pl);
        assert_eq!(get(r, &p).unwrap(), "#reference \"my/own.slangp\"\n", "{st}");
    }
}

/// 6. Per-game: Donkey Kong Country gets its own style; Undo removes its files.
#[test]
fn per_game_style_then_undo() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "vulkan", true);
    let mut dev = AdbFs { adb: &adb, serial: "FAKE0001" };
    let dirs = vec![("snes".to_string(), format!("/{SNES}"))];
    let games = picstyle::games(&mut dev, &dirs);
    assert_eq!(games.len(), 2);
    let dkc = games.iter().find(|g| g.name == "Donkey Kong Country (USA)").unwrap().clone();
    assert_eq!((dkc.style.clone(), dkc.own), (None, false));
    let res = picstyle::apply(&mut dev, &[picstyle::Pick { system: "snes".into(), file: dkc.file.clone(), style: Some("crt".into()) }]);
    assert_eq!(res.failed, 0, "{:?}", res.lines);
    let preset = get(r, &format!("{RA}/config/Snes9x/Donkey Kong Country (USA).slangp")).unwrap();
    assert!(preset.starts_with(MARKER) && preset.contains("crt/crt-easymode.slangp"));
    assert!(get(r, &format!("{RA}/config/Snes9x/Donkey Kong Country (USA).cfg")).unwrap().contains("video_smooth = \"false\""));
    assert!(get(r, &format!("{RA}/config/Snes9x/Super Mario World (USA).slangp")).is_none(), "other games untouched");
    let again = picstyle::games(&mut dev, &dirs);
    assert_eq!(again.iter().find(|g| g.file == dkc.file).unwrap().style.as_deref(), Some("crt"));

    // "No shader" for this one game.
    picstyle::apply(&mut dev, &[picstyle::Pick { system: "snes".into(), file: dkc.file.clone(), style: Some("none".into()) }]);
    assert!(get(r, &format!("{RA}/config/Snes9x/Donkey Kong Country (USA).slangp")).unwrap().contains("shaders_slang/stock.slang"));
    assert_eq!(picstyle::games(&mut dev, &dirs).iter().find(|g| g.file == dkc.file).unwrap().style.as_deref(), Some("none"));

    // Your own per-game file for Mario is never touched.
    let mine = format!("{RA}/config/Snes9x/Super Mario World (USA).slangp");
    put(r, &mine, "#reference \"mine.slangp\"\n");
    let res = picstyle::apply(&mut dev, &[picstyle::Pick { system: "snes".into(), file: format!("/{SNES}/Super Mario World (USA).sfc"), style: Some("smooth".into()) }]);
    assert_eq!(res.failed, 1);
    assert!(res.lines[0].contains("your own file"), "{:?}", res.lines);
    assert_eq!(get(r, &mine).unwrap(), "#reference \"mine.slangp\"\n");
    assert!(picstyle::games(&mut dev, &dirs).iter().find(|g| g.name.starts_with("Super Mario")).unwrap().own);

    // Undo RomCrate's changes removes DKC's files (and leaves yours).
    let u = gamesettings::undo(&mut dev, &["snes".to_string()]);
    assert_eq!(u.failed, 0, "{:?}", u.lines);
    assert!(get(r, &format!("{RA}/config/Snes9x/Donkey Kong Country (USA).slangp")).is_none());
    assert!(get(r, &format!("{RA}/config/Snes9x/Donkey Kong Country (USA).cfg")).is_none());
    assert_eq!(get(r, &mine).unwrap(), "#reference \"mine.slangp\"\n");
}

/// 7. Old data with a single `shader` still works, as Sharp.
#[test]
fn old_single_shader_reads_as_sharp() {
    let p: Profile = serde_json::from_str(r#"{
        "id": "snes", "systems": ["snes"], "emulator": "RetroArch", "kind": "retroarch-core", "core": "Snes9x",
        "shader": { "preset": "interpolation/quilez", "label": "Sharp scaling shader (quilez)", "why": "Keeps pixels sharp.", "tested": true },
        "source": "test"
    }"#).unwrap();
    let st = p.styles();
    assert_eq!(st.len(), 1);
    assert_eq!((st[0].id.as_str(), st[0].default, st[0].tested), ("sharp", true, true));
    assert_eq!(st[0].slang.as_deref(), Some("interpolation/quilez"));
    assert_eq!(p.default_style().as_deref(), Some("sharp"));
}

/// The default is still Sharp, so nothing changes for anyone who doesn't pick a style.
#[test]
fn default_is_sharp_as_before() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let adb = handheld(r, "vulkan", true);
    let mut dev = AdbFs { adb: &adb, serial: "FAKE0001" };
    let plan = gamesettings::plan(&mut dev, &["snes".to_string()]);
    let c = snes(&plan);
    assert_eq!(c.style.as_deref(), Some("sharp"));
    assert!(!c.changes.iter().flat_map(|f| &f.keys).any(|k| k.key == "video_smooth"), "Sharp leaves RetroArch's smoothing alone");
    apply(&mut dev, &plan);
    assert!(get(r, &format!("{RA}/config/Snes9x/Snes9x.slangp")).unwrap().contains("interpolation/quilez.slangp"));
}
