//! A different picture style for one game (RetroArch): e.g. Donkey Kong Country
//! on CRT TV while the other SNES games stay Sharp.
//!
//! RetroArch looks for per-game files next to the core's files, named after the
//! game file without its extension:
//! `<config>/<Core>/<game>.slangp|.glslp` (the shader) and `<config>/<Core>/<game>.cfg`
//! (RetroArch's own smoothing). RomCrate writes them with its MARKER line and lists
//! them in the per-game record on the handheld, so "Undo RomCrate's changes"
//! removes them. A per-game file without the marker is yours and is never touched.

use crate::gamesettings::{
    find_preset, preset_text, retroarch_shader_info, style_of_preset, ApplyResult, DeviceFs, PresetFound,
    MARKER, STYLE_NONE,
};
use crate::pergame::{games_in, manifest, profile_for, save_manifest, stem};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleGame {
    pub system: String,
    /// The game file on the handheld.
    pub file: String,
    pub name: String,
    /// Its own style (None = the console's style).
    pub style: Option<String>,
    /// A per-game shader preset you made yourself: RomCrate leaves this game alone.
    pub own: bool,
}

fn files_for(cfg: &str, core: &str, driver: &str, game_file: &str) -> (String, String) {
    let ext = if driver == "vulkan" { "slangp" } else { "glslp" };
    let s = stem(game_file);
    (format!("{cfg}/{core}/{s}.{ext}"), format!("{cfg}/{core}/{s}.cfg"))
}

/// The games in these console folders (system, folder) that can have a style of their own.
pub fn games(fs: &mut dyn DeviceFs, dirs: &[(String, String)]) -> Vec<StyleGame> {
    let Some((cfg, driver, _)) = retroarch_shader_info(fs) else {
        return vec![];
    };
    let mut out = Vec::new();
    for (system, dir) in dirs {
        let Some(p) = profile_for(system).filter(|p| !p.styles().is_empty()) else {
            continue;
        };
        let core = p.core.clone().unwrap_or_default();
        for file in games_in(fs, system, dir) {
            let (preset, _) = files_for(&cfg, &core, &driver, &file);
            let (style, own) = match fs.read(&preset).ok().flatten() {
                Some(t) if t.starts_with(MARKER) => (
                    style_of_preset(p, &t).or_else(|| t.contains("/stock.").then(|| STYLE_NONE.to_string())),
                    false,
                ),
                Some(_) => (None, true),
                None => (None, false),
            };
            out.push(StyleGame {
                system: system.clone(),
                name: stem(&file),
                file,
                style,
                own,
            });
        }
    }
    out
}

/// One game's style: `Some(style id)` (or "none") for its own, `None` to follow the console again.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    pub system: String,
    pub file: String,
    pub style: Option<String>,
}

pub fn apply(fs: &mut dyn DeviceFs, picks: &[Pick]) -> ApplyResult {
    let mut res = ApplyResult::default();
    let Some((cfg, driver, shader_dir)) = retroarch_shader_info(fs) else {
        res.failed += 1;
        res.lines.push("FAILED RetroArch wasn't found on the handheld".into());
        return res;
    };
    let mut record = manifest(fs);
    let mut stopped = false;
    for pk in picks {
        let name = stem(&pk.file);
        let r = (|| -> anyhow::Result<String> {
            let p = profile_for(&pk.system)
                .filter(|p| !p.styles().is_empty())
                .ok_or_else(|| anyhow::anyhow!("no picture styles for this console"))?;
            let core = p.core.clone().unwrap_or_default();
            let (preset, cfgf) = files_for(&cfg, &core, &driver, &pk.file);
            // Never touch a per-game file you made yourself.
            for f in [&preset, &cfgf] {
                if let Some(t) = fs.read(f)? {
                    anyhow::ensure!(
                        t.starts_with(MARKER),
                        "kept — {f} is your own file, so RomCrate leaves this game alone"
                    );
                }
            }
            if !stopped {
                for pkg in crate::gamesettings::RETROARCH_PACKAGES {
                    fs.stop_app(pkg);
                }
                stopped = true;
            }
            match pk.style.as_deref() {
                None => {
                    remove(fs, &preset, &mut record)?;
                    remove(fs, &cfgf, &mut record)?;
                    Ok("follows the console's style again".into())
                }
                Some(STYLE_NONE) => {
                    // The console has a shader; to turn it off for this game alone,
                    // the game's preset is one pass of RetroArch's plain pass-through
                    // shader (`stock.slang` / `stock.glsl`, at the top of both packs).
                    let (folder, ext) = if driver == "vulkan" { ("shaders_slang", "slang") } else { ("shaders_glsl", "glsl") };
                    let stock = shader_dir
                        .as_deref()
                        .map(|d| format!("{d}/{folder}/stock.{ext}"))
                        .filter(|f| fs.read(f).ok().flatten().is_some())
                        .ok_or_else(|| anyhow::anyhow!("RetroArch's shaders aren't on the handheld yet: Main Menu → Online Updater → Update {} Shaders, then try again", if driver == "vulkan" { "Slang" } else { "GLSL" }))?;
                    let text = format!("{MARKER}\nshaders = \"1\"\nshader0 = \"{stock}\"\n");
                    write_text(fs, &preset, &text, &mut record)?;
                    remove(fs, &cfgf, &mut record)?;
                    Ok("no shader".into())
                }
                Some(id) => {
                    let st = p
                        .styles()
                        .into_iter()
                        .find(|s| s.id == id)
                        .ok_or_else(|| anyhow::anyhow!("unknown style {id}"))?;
                    write_preset(fs, &driver, shader_dir.as_deref(), &st, &preset, &mut record)?;
                    if id == "sharp" {
                        remove(fs, &cfgf, &mut record)?;
                    } else {
                        write_text(fs, &cfgf, &format!("{MARKER}\nvideo_smooth = \"false\"\n"), &mut record)?;
                    }
                    Ok(st.label)
                }
            }
        })();
        match r {
            Ok(what) => {
                res.changed_files += 1;
                res.lines.push(format!("OK {name} ({}): {what}", pk.system));
            }
            Err(e) => {
                res.failed += 1;
                res.lines.push(format!("FAILED {name} ({}): {e}", pk.system));
            }
        }
    }
    if let Err(e) = save_manifest(fs, &record) {
        res.lines.push(format!("Couldn't update RomCrate's list of created files: {e}"));
    }
    res
}

fn write_preset(
    fs: &mut dyn DeviceFs,
    driver: &str,
    shader_dir: Option<&str>,
    st: &crate::gamesettings::ShaderStyle,
    path: &str,
    record: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    let f = match find_preset(fs, driver, shader_dir, st) {
        PresetFound::At(f) => f,
        PresetFound::NotForDriver => anyhow::bail!("“{}” needs RetroArch's Vulkan video driver", st.label),
        PresetFound::Missing { updater } => anyhow::bail!(
            "RetroArch's shaders aren't on the handheld yet: Main Menu → Online Updater → Update {updater} Shaders, then try again"
        ),
    };
    write_text(fs, path, &preset_text(&f), record)
}

/// Write one of RomCrate's own per-game files and note it for Undo.
fn write_text(fs: &mut dyn DeviceFs, path: &str, text: &str, record: &mut Vec<(String, String)>) -> anyhow::Result<()> {
    fs.write(path, text)?;
    anyhow::ensure!(fs.read(path)?.as_deref() == Some(text), "didn't read back the same");
    if !record.iter().any(|(_, x)| x == path) {
        record.push(("created".into(), path.to_string()));
    }
    Ok(())
}

/// Remove one of RomCrate's own per-game files (checked to be ours before).
fn remove(fs: &mut dyn DeviceFs, path: &str, record: &mut Vec<(String, String)>) -> anyhow::Result<()> {
    if fs.read(path)?.is_some() {
        fs.remove(path)?;
    }
    record.retain(|(_, x)| x != path);
    Ok(())
}
