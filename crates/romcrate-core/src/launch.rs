//! Pegasus launch lines: how Pegasus starts each emulator.
//!
//! The commands live in `data/launch_profiles.json` (data, not code), so a new
//! emulator or a fixed command is a one-line change with its source recorded.
//! Only profiles tested on a real device (`verified: true`) are written as an
//! active `launch:` line; others are written commented out with a request to
//! test and report.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DATA: &str = include_str!("../data/launch_profiles.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LaunchProfile {
    pub id: String,
    pub emulator: String,
    pub platform: String,
    pub systems: Vec<String>,
    pub package: String,
    pub activity: String,
    /// Command with Pegasus placeholders; continuation lines are indented.
    pub launch: String,
    pub verified: bool,
    #[serde(default)]
    pub verified_on: Option<String>,
    pub verified_date: String,
    pub source: String,
    #[serde(default)]
    pub notes: Option<String>,
}

/// A launch line known not to work, and what to use instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnownBad {
    pub pattern: String,
    pub systems: Vec<String>,
    pub why: String,
    pub fix: String,
}

#[derive(Debug, Deserialize)]
pub struct LaunchDb {
    pub profiles: Vec<LaunchProfile>,
    pub known_bad: Vec<KnownBad>,
}

/// The built-in launch database.
pub fn db() -> &'static LaunchDb {
    static DB: OnceLock<LaunchDb> = OnceLock::new();
    DB.get_or_init(|| {
        serde_json::from_str(DATA).expect("data/launch_profiles.json is valid (checked by tests)")
    })
}

pub fn profile(id: &str) -> Option<&'static LaunchProfile> {
    db().profiles.iter().find(|p| p.id == id)
}

/// Profiles for a system on a platform, tested ones first.
pub fn profiles_for(system: &str, platform: &str) -> Vec<&'static LaunchProfile> {
    let mut v: Vec<_> = db()
        .profiles
        .iter()
        .filter(|p| p.platform == platform && p.systems.iter().any(|s| s == system))
        .collect();
    v.sort_by_key(|p| !p.verified);
    v
}

/// What the user chose for launch lines.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchChoice {
    /// "android" to write Android launch lines; `None` = don't write any.
    #[serde(default)]
    pub platform: Option<String>,
    /// Emulator per system (system id → profile id). Missing = best known.
    #[serde(default)]
    pub per_system: HashMap<String, String>,
    /// Opt-in: add/fix the launch line in metadata files you already have
    /// (a .bak backup is made first).
    #[serde(default)]
    pub fix_existing: bool,
}

impl LaunchChoice {
    /// The profile to use for a system, if launch lines are wanted.
    pub fn profile_for(&self, system: &str) -> Option<&'static LaunchProfile> {
        let platform = self.platform.as_deref()?;
        if let Some(id) = self.per_system.get(system) {
            if let Some(p) = profile(id) {
                if p.platform == platform && p.systems.iter().any(|s| s == system) {
                    return Some(p);
                }
            }
        }
        profiles_for(system, platform).into_iter().next()
    }
}

/// The `launch:` block as it appears in metadata.pegasus.txt. Untested
/// profiles come out commented, with a note.
pub fn render_block(p: &LaunchProfile) -> String {
    let mut lines = p.launch.lines();
    let mut out = String::new();
    let first = lines.next().unwrap_or_default();
    if p.verified {
        out.push_str(&format!(
            "# Launch line for {} — tested on {} ({}).\n",
            p.emulator,
            p.verified_on.as_deref().unwrap_or("a real device"),
            p.verified_date
        ));
        out.push_str(&format!("launch: {first}\n"));
        for l in lines {
            out.push_str(l);
            out.push('\n');
        }
    } else {
        out.push_str(&format!(
            "# Launch line for {} — NOT tested on a device yet. Remove the '# ' in front of\n# the lines below to try it, and please report whether it works.\n",
            p.emulator
        ));
        out.push_str(&format!("# launch: {first}\n"));
        for l in lines {
            out.push_str(&format!("# {l}\n"));
        }
    }
    if let Some(n) = &p.notes {
        out.push_str(&format!("# Note: {}\n", n.replace('\n', " ")));
    }
    out
}

/// Does this metadata text already have a `launch:` line (active, not commented)?
pub fn has_launch(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().starts_with("launch:"))
}

/// A known-bad launch line in this text for this system.
pub fn find_known_bad(text: &str, system: &str) -> Option<&'static KnownBad> {
    let active: String = active_launch_text(text);
    db().known_bad
        .iter()
        .find(|b| b.systems.iter().any(|s| s == system) && active.contains(&b.pattern))
}

/// The active launch value (first line + its indented continuation lines).
fn active_launch_text(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for l in text.lines() {
        if l.trim_start().starts_with("launch:") {
            inside = true;
            out.push_str(l);
            out.push('\n');
            continue;
        }
        if inside {
            if l.starts_with(' ') || l.starts_with('\t') {
                out.push_str(l);
                out.push('\n');
            } else {
                inside = false;
            }
        }
    }
    out
}

/// Insert a launch block into the collection header: after the last of the
/// header keys (`collection:`, `shortname:`, `extension(s):`, `ignore-file(s):`…)
/// that appear before the first `game:` entry. `None` if there's no collection.
pub fn insert_launch(text: &str, block: &str) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let first_game = lines
        .iter()
        .position(|l| l.trim_start().starts_with("game:"))
        .unwrap_or(lines.len());
    let collection = lines[..first_game]
        .iter()
        .position(|l| l.trim_start().starts_with("collection:"))?;
    // Last header key line (plus its indented continuation lines) before the first game.
    let mut insert_at = collection + 1;
    let mut i = collection + 1;
    while i < first_game {
        let l = lines[i];
        let t = l.trim_start();
        if l.starts_with(' ') || l.starts_with('\t') {
            if insert_at == i {
                insert_at = i + 1;
            }
        } else if t.contains(':') && !t.starts_with('#') {
            insert_at = i + 1;
        } else if t.is_empty() {
            // blank line ends the header
            break;
        }
        i += 1;
    }
    let mut out = String::with_capacity(text.len() + block.len() + 2);
    for l in &lines[..insert_at] {
        out.push_str(l);
    }
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out.push_str(block);
    for l in &lines[insert_at..] {
        out.push_str(l);
    }
    Some(out)
}

/// Replace the active launch block (a known-bad one) with `block`.
pub fn replace_launch(text: &str, block: &str) -> String {
    let mut out = String::with_capacity(text.len() + block.len());
    let mut inside = false;
    let mut done = false;
    for l in text.split_inclusive('\n') {
        if !done && l.trim_start().starts_with("launch:") {
            inside = true;
            done = true;
            out.push_str(block);
            continue;
        }
        if inside {
            if l.starts_with(' ') || l.starts_with('\t') {
                continue;
            }
            inside = false;
        }
        out.push_str(l);
    }
    out
}

/// A free backup name: metadata.pegasus.txt.bak, then .bak2, .bak3…
pub fn backup_path(file: &Path) -> PathBuf {
    let mut n = 1;
    loop {
        let mut s = file.as_os_str().to_owned();
        if n == 1 {
            s.push(".bak");
        } else {
            s.push(format!(".bak{n}"));
        }
        let p = PathBuf::from(s);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn database_is_valid() {
        let db: LaunchDb = serde_json::from_str(DATA).expect("valid JSON");
        let mut ids = HashSet::new();
        for p in &db.profiles {
            assert!(ids.insert(&p.id), "duplicate id {}", p.id);
            assert!(!p.source.trim().is_empty(), "{} needs a source", p.id);
            assert!(
                !p.verified_date.trim().is_empty(),
                "{} needs verified_date",
                p.id
            );
            assert!(
                p.launch.contains("{file.uri}") || p.launch.contains("{file.path}"),
                "{} launch must pass the game file",
                p.id
            );
            assert!(!p.systems.is_empty());
            for s in &p.systems {
                assert!(
                    crate::systems::by_id(s).is_some(),
                    "{}: unknown system {s}",
                    p.id
                );
            }
            if p.verified {
                assert!(p.verified_on.is_some(), "{} verified but no device", p.id);
            }
            assert!(
                p.launch.contains(&p.package),
                "{}: launch doesn't use its package",
                p.id
            );
        }
        for b in &db.known_bad {
            assert!(profile(&b.fix).is_some(), "known_bad fix {} missing", b.fix);
            for s in &b.systems {
                assert!(crate::systems::by_id(s).is_some());
            }
        }
        // The obsolete activity is never emitted.
        assert!(!db
            .profiles
            .iter()
            .any(|p| p.launch.contains("kr.co.iefriends")));
    }

    #[test]
    fn armsx2_playstore_block_is_exact() {
        let p = profile("armsx2-playstore").unwrap();
        assert!(p.verified);
        let b = render_block(p);
        assert!(b.contains(
            "launch: am start\n  -n come.nanodata.armsx2/com.armsx2.MainActivity\n  -a android.intent.action.VIEW\n  -d \"{file.uri}\"\n  --activity-clear-task\n  --activity-clear-top\n  --activity-no-history\n"
        ));
        let c = LaunchChoice {
            platform: Some("android".into()),
            ..Default::default()
        };
        assert_eq!(
            c.profile_for("ps2").unwrap().id,
            "armsx2-playstore",
            "tested profile preferred"
        );
        assert!(LaunchChoice::default().profile_for("ps2").is_none());
    }

    #[test]
    fn owners_tested_lines_are_on_and_exact() {
        let c = LaunchChoice {
            platform: Some("android".into()),
            ..Default::default()
        };
        let cases = [
            ("n64", "retroarch-n64", "launch: am start\n  -n com.retroarch/.browser.retroactivity.RetroActivityFuture\n  -e ROM \"{file.path}\"\n  -e LIBRETRO /data/data/com.retroarch/cores/mupen64plus_next_gles3_libretro_android.so\n"),
            ("nds", "melonds", "launch: am start\n  -n me.magnum.melonds/me.magnum.melonds.ui.emulator.EmulatorActivity\n  -d {file.uri}\n  --grant-read-uri-permission\n"),
            ("wii", "dolphin-wii", "launch: am start\n  -n org.dolphinemu.dolphinemu/.ui.main.MainActivity\n  -a android.intent.action.VIEW\n  --es AutoStartFile \"{file.path}\"\n"),
            ("ps2", "armsx2-playstore", "launch: am start\n  -n come.nanodata.armsx2/"),
        ];
        for (sys, id, line) in cases {
            let p = c.profile_for(sys).unwrap();
            assert_eq!(p.id, id, "{sys}");
            assert!(p.verified, "{id}");
            let b = render_block(p);
            assert!(has_launch(&b) && b.contains(line), "{b}");
        }
        // Not launched by anyone yet: written, but switched off.
        for sys in ["snes", "nes", "gb", "gbc", "gba", "psx", "psp", "gc"] {
            let p = c.profile_for(sys).unwrap();
            assert!(!p.verified, "{sys}");
            assert!(!has_launch(&render_block(p)), "{sys}");
        }
        // Play Store RetroArch is the default over the retroarch.com build.
        assert_eq!(c.profile_for("snes").unwrap().package, "com.retroarch");
    }

    #[test]
    fn untested_profiles_are_commented_out() {
        let b = render_block(profile("retroarch64-snes").unwrap());
        assert!(!has_launch(&b), "{b}");
        assert!(b.lines().all(|l| l.starts_with('#')));
    }

    #[test]
    fn inserts_after_the_header_only() {
        let text = "collection: PlayStation 2\nshortname: ps2\nextensions: iso, chd\n  , cso\n\ngame: A\nfile: a.chd\n";
        let out = insert_launch(text, "launch: X\n  -d y\n").unwrap();
        assert_eq!(
            out,
            "collection: PlayStation 2\nshortname: ps2\nextensions: iso, chd\n  , cso\nlaunch: X\n  -d y\n\ngame: A\nfile: a.chd\n"
        );
        // Removing the inserted lines gives back the original exactly.
        assert_eq!(out.replace("launch: X\n  -d y\n", ""), text);
        assert!(insert_launch("game: A\nfile: a\n", "launch: X\n").is_none());
    }

    #[test]
    fn finds_and_replaces_known_bad() {
        let text = "collection: PS2\nlaunch: am start\n  -n com.armsx2/com.armsx2.MainActivity\n  -d \"{file.uri}\"\ngame: A\n";
        let bad = find_known_bad(text, "ps2").unwrap();
        assert_eq!(bad.fix, "armsx2-playstore");
        assert!(find_known_bad(text, "snes").is_none());
        let fixed = replace_launch(text, "launch: NEW\n");
        assert_eq!(fixed, "collection: PS2\nlaunch: NEW\ngame: A\n");
        // A commented-out bad line is not "in use".
        assert!(find_known_bad(
            "# launch: am start -n com.armsx2/com.armsx2.MainActivity\n",
            "ps2"
        )
        .is_none());
        // The good Play Store line is not flagged.
        let good = render_block(profile("armsx2-playstore").unwrap());
        assert!(find_known_bad(&good, "ps2").is_none());
    }
}
