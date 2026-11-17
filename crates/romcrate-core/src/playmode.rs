//! Play modes: switch the handheld's emulators between playing on its own
//! screen, on a TV by cable or streamed to a TV, and with its own buttons, a
//! wired or a Bluetooth controller.
//!
//! What a mode changes: RetroArch's run-ahead (hides a game's own input delay)
//! and automatic frame delay, per core, in the same core override files Game
//! settings uses — so the same backups and Undo apply. Only light consoles
//! (SNES, NES, Game Boy, GBA) get run-ahead; heavier ones would slow down.
//! Things no setting can fix (TV picture processing, streaming delay) become
//! short steps for you to do.

use crate::gamesettings::{self, DeviceFs, Profile, Setting, SettingsPlan};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Where RomCrate notes the mode last applied, on the handheld.
pub const MODE_FILE: &str = "/storage/emulated/0/RomCrate/play-mode.txt";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub desc: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Modes {
    pub screens: Vec<Choice>,
    pub controllers: Vec<Choice>,
    pub runahead_profiles: Vec<String>,
    pub heavy_note: String,
    pub steps: std::collections::BTreeMap<String, Vec<String>>,
}

pub fn modes() -> &'static Modes {
    static M: OnceLock<Modes> = OnceLock::new();
    M.get_or_init(|| {
        serde_json::from_str(include_str!("../data/play_modes.json"))
            .expect("data/play_modes.json is valid")
    })
}

/// How many frames to run ahead for a mode: one to hide the game's own delay,
/// two when a Bluetooth controller or streaming adds more on top.
pub fn runahead_frames(screen: &str, controller: &str) -> u32 {
    if screen == "tv-stream" || controller == "bluetooth" {
        2
    } else {
        1
    }
}

fn setting(key: &str, value: &str, label: &str, why: &str) -> Setting {
    Setting {
        section: None,
        key: key.into(),
        value: value.into(),
        label: label.into(),
        why: why.into(),
        tested: false,
    }
}

/// The profiles a mode applies: RetroArch light cores get run-ahead settings;
/// everything else becomes steps only.
pub fn profiles(screen: &str, controller: &str) -> anyhow::Result<Vec<Profile>> {
    let m = modes();
    anyhow::ensure!(
        m.screens.iter().any(|c| c.id == screen),
        "unknown screen {screen}"
    );
    anyhow::ensure!(
        m.controllers.iter().any(|c| c.id == controller),
        "unknown controller {controller}"
    );
    let mut steps: Vec<String> = Vec::new();
    for k in [screen, controller] {
        steps.extend(m.steps.get(k).cloned().unwrap_or_default());
    }
    let frames = runahead_frames(screen, controller);
    let mut out = Vec::new();
    for p in &gamesettings::db().profiles {
        let mut q = p.clone();
        q.options.clear();
        q.options_if_driver.clear();
        q.note_if_other_driver = None;
        q.shader = None;
        q.per_game = None;
        q.files.clear();
        q.note = None;
        q.tested = false;
        q.source = "RomCrate play modes (RetroArch run-ahead; not yet tested on a device)".into();
        q.steps = steps.clone();
        if p.kind == "retroarch-core" && m.runahead_profiles.contains(&p.id) {
            q.r#override = vec![
                setting("run_ahead_enabled", "true", "Run-ahead", "Hides the delay the game itself has, so buttons feel instant."),
                setting(
                    "run_ahead_frames",
                    &frames.to_string(),
                    "Run-ahead frames",
                    if frames == 2 {
                        "Two frames: makes up for the extra delay of Bluetooth or streaming."
                    } else {
                        "One frame: safe for almost every game."
                    },
                ),
                setting("run_ahead_hide_warnings", "true", "Hide run-ahead messages", "No pop-ups during play."),
                setting("video_frame_delay_auto", "true", "Automatic frame delay", "Reads the buttons as late as possible before each frame, and backs off by itself if a game slows down."),
            ];
        } else {
            // Not changed by play modes: say why, as a checklist.
            q.kind = "checklist".into();
            q.r#override.clear();
            q.why_checklist = Some(if p.kind == "retroarch-core" {
                m.heavy_note.replace("{emulator}", &p.emulator)
            } else {
                format!("{} has no delay settings RomCrate can change safely; the steps below still help.", p.emulator)
            });
        }
        out.push(q);
    }
    Ok(out)
}

/// What switching to this mode would change.
pub fn plan(
    fs: &mut dyn DeviceFs,
    systems: &[String],
    screen: &str,
    controller: &str,
) -> anyhow::Result<SettingsPlan> {
    Ok(gamesettings::plan_profiles(
        fs,
        systems,
        &profiles(screen, controller)?,
    ))
}

/// Note the applied mode on the handheld (a small text file RomCrate owns).
pub fn record(fs: &mut dyn DeviceFs, screen: &str, controller: &str) -> anyhow::Result<()> {
    fs.write(
        MODE_FILE,
        &format!("# Written by RomCrate: the play mode last applied.\nscreen={screen}\ncontroller={controller}\n"),
    )
}

/// The mode last applied with RomCrate, if any.
pub fn current(fs: &mut dyn DeviceFs) -> Option<(String, String)> {
    let t = fs.read(MODE_FILE).ok().flatten()?;
    let get = |k: &str| {
        t.lines().find_map(|l| {
            l.strip_prefix(&format!("{k}="))
                .map(|v| v.trim().to_string())
        })
    };
    Some((get("screen")?, get("controller")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_by_mode() {
        assert_eq!(runahead_frames("handheld", "builtin"), 1);
        assert_eq!(runahead_frames("tv-cable", "wired"), 1);
        assert_eq!(runahead_frames("handheld", "bluetooth"), 2);
        assert_eq!(runahead_frames("tv-stream", "builtin"), 2);
    }

    #[test]
    fn only_light_cores_run_ahead() {
        let ps = profiles("tv-stream", "bluetooth").unwrap();
        let snes = ps.iter().find(|p| p.id == "snes").unwrap();
        assert_eq!(snes.kind, "retroarch-core");
        assert!(snes
            .r#override
            .iter()
            .any(|s| s.key == "run_ahead_frames" && s.value == "2"));
        let n64 = ps.iter().find(|p| p.id == "n64").unwrap();
        assert_eq!(n64.kind, "checklist");
        assert!(n64.why_checklist.as_deref().unwrap().contains("too heavy"));
        assert!(snes.steps.iter().any(|s| s.contains("Streaming")));
        assert!(snes.steps.iter().any(|s| s.contains("Bluetooth")));
        assert!(profiles("moon", "builtin").is_err());
    }
}
