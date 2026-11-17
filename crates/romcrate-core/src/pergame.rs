//! Per-game settings: one game's own resolution setting, on top of the
//! console-wide settings from `gamesettings`.
//!
//! Each emulator has its own per-game file (checked against their source code):
//! * RetroArch: `<config>/<core>/<game file name>.opt`. When this file exists
//!   RetroArch uses it *instead of* the core's options, so RomCrate starts it
//!   from the core's current options and changes one line.
//! * Dolphin: `<user>/GameSettings/<6-character game ID>.ini`, section
//!   `[Video_Settings]` (= GFX.ini `[Settings]`).
//! * PPSSPP: `<ID>_ppsspp.ini` next to ppsspp.ini. PPSSPP resets the controls
//!   to default for a game whose file has no `[ControlMapping]`, so RomCrate
//!   copies the current mapping from controls.ini into new files.
//! * ARMSX2 (PS2): settings live inside the app → a per-game checklist line.
//!
//! Files RomCrate creates are listed in `MANIFEST` on the handheld so Undo
//! can remove them; files it changes get a `.romcrate-backup` first.

use crate::gamesettings::{
    cfg_get, cfg_set, db, ini_set, ApplyResult, DeviceFs, DeviceReader, Profile, BACKUP_SUFFIX,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MANIFEST: &str = "/storage/emulated/0/RomCrate/settings-files-created.txt";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Step {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PerGame {
    /// "retroarch-opt", "dolphin", "ppsspp" or "checklist"
    pub kind: String,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    pub label: String,
    #[serde(default)]
    pub only_if_driver: Option<String>,
    pub ladder: Vec<Step>,
    pub default: String,
    #[serde(default)]
    pub step: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub systems: Vec<String>,
    pub genres_any: Vec<String>,
    pub value: String,
    pub why: String,
    #[serde(default)]
    pub tested: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub value: String,
    pub why: String,
    pub tested: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRow {
    pub system: String,
    /// The game file on the handheld.
    pub file: String,
    pub name: String,
    pub id: Option<String>,
    pub kind: String,
    pub label: String,
    pub ladder: Vec<Step>,
    /// The console-wide value this game uses unless it has its own.
    pub console_value: String,
    /// This game's own value, if it has one (None for checklist consoles).
    pub own_value: Option<String>,
    pub suggestion: Option<Suggestion>,
    /// Why this game can't have its own file, if so.
    pub note: Option<String>,
    /// The per-game settings file (None when it can't have one).
    pub settings_file: Option<String>,
    /// For checklist consoles: the step text with `{label}` to fill in.
    pub step: Option<String>,
}

fn rules() -> Vec<Rule> {
    #[derive(Deserialize)]
    struct R {
        #[serde(default)]
        per_game_rules: Vec<Rule>,
    }
    serde_json::from_str::<R>(include_str!("../data/game_settings.json"))
        .map(|r| r.per_game_rules)
        .unwrap_or_default()
}

pub(crate) fn profile_for(system: &str) -> Option<&'static Profile> {
    db().profiles
        .iter()
        .find(|p| p.systems.iter().any(|s| s == system))
}

pub fn per_game_of(p: &Profile) -> Option<PerGame> {
    p.per_game.clone()
}

pub(crate) fn stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(name)
        .to_string()
}

fn ext(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default()
}

pub fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_sec = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_sec = t[1..t.len() - 1].eq_ignore_ascii_case(section);
            continue;
        }
        if in_sec && !t.starts_with('#') && !t.starts_with(';') {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// Where each emulator keeps things on the handheld, worked out once.
pub struct Places {
    /// RetroArch config folder and the shared core options file.
    pub retroarch: Option<(String, String, String)>, // (config_dir, core_options_file, driver)
    pub dolphin_user: String,
    /// Folder holding ppsspp.ini, if found.
    pub ppsspp_dir: Option<String>,
}

pub fn places(fs: &mut dyn DeviceFs) -> Places {
    let installed = fs.installed_packages();
    let retroarch = crate::gamesettings::retroarch_paths(fs, &installed);
    let ppsspp_dir = [
        "/storage/emulated/0/PSP/SYSTEM/ppsspp.ini".to_string(),
        "/storage/emulated/0/Android/data/org.ppsspp.ppsspp/files/PSP/SYSTEM/ppsspp.ini"
            .to_string(),
    ]
    .into_iter()
    .find(|p| fs.read(p).ok().flatten().is_some())
    .or_else(|| fs.find_file("ppsspp.ini"))
    .map(|p| {
        p.rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default()
    });
    Places {
        retroarch,
        dolphin_user: "/storage/emulated/0/Android/data/org.dolphinemu.dolphinemu/files".into(),
        ppsspp_dir,
    }
}

/// Game files in a console folder, skipping the parts of multi-file games.
pub(crate) fn games_in(fs: &mut dyn DeviceFs, system: &str, dir: &str) -> Vec<String> {
    let Some(sys) = crate::systems::by_id(system) else {
        return vec![];
    };
    let files = fs.list_files(dir);
    let names: HashSet<String> = files.iter().map(|f| f.to_lowercase()).collect();
    let mut out: Vec<String> = files
        .into_iter()
        .filter(|f| {
            let e = ext(f);
            if !sys.extensions.contains(&e.as_str()) {
                return false;
            }
            // track .bin files belong to their .cue
            if e == "bin" {
                let cue = format!("{}.cue", &f[..f.len() - 4]).to_lowercase();
                return !names.contains(&cue);
            }
            true
        })
        .collect();
    out.sort_by_key(|f| f.to_lowercase());
    out
}

/// Every game in the given console folders on the handheld, with its current
/// per-game value and RomCrate's suggestion. `genre_of(system, name)` looks a
/// game's genre up (LaunchBox); return None when unknown.
pub fn scan(
    fs: &mut dyn DeviceFs,
    dirs: &[(String, String)],
    genre_of: &dyn Fn(&str, &str) -> Option<String>,
) -> Vec<GameRow> {
    let pl = places(fs);
    let rules = rules();
    let mut rows = Vec::new();
    for (system, dir) in dirs {
        let Some(prof) = profile_for(system) else {
            continue;
        };
        let Some(pg) = per_game_of(prof) else {
            continue;
        };
        let driver_ok = match (&pg.only_if_driver, &pl.retroarch) {
            (Some(want), Some((_, _, drv))) => drv == want,
            (Some(_), None) => false,
            (None, _) => true,
        };
        if !driver_ok {
            continue;
        }
        for file in games_in(fs, system, dir) {
            let name = stem(&file);
            let mut row = GameRow {
                system: system.clone(),
                file: file.clone(),
                name: name.clone(),
                id: None,
                kind: pg.kind.clone(),
                label: pg.label.clone(),
                ladder: pg.ladder.clone(),
                console_value: pg.default.clone(),
                own_value: None,
                suggestion: None,
                note: None,
                settings_file: None,
                step: pg.step.clone(),
            };
            match pg.kind.as_str() {
                "retroarch-opt" => {
                    if let (Some((cfg, shared, _)), Some(core), Some(key)) =
                        (&pl.retroarch, &prof.core, &pg.key)
                    {
                        let core_opt = format!("{cfg}/{core}/{core}.opt");
                        let console = fs
                            .read(&core_opt)
                            .ok()
                            .flatten()
                            .or_else(|| fs.read(shared).ok().flatten())
                            .and_then(|t| cfg_get(&t, key));
                        if let Some(v) = console {
                            row.console_value = v;
                        }
                        let path = format!("{cfg}/{core}/{name}.opt");
                        row.own_value =
                            fs.read(&path).ok().flatten().and_then(|t| cfg_get(&t, key));
                        row.settings_file = Some(path);
                    } else {
                        row.note =
                            Some("RetroArch's settings weren't found on the handheld.".into());
                    }
                }
                "dolphin" | "ppsspp" => {
                    let e = ext(&file);
                    let id = {
                        let mut r = DeviceReader::new(fs, &file);
                        crate::identify::disc_id(system, &mut r, &e)
                    };
                    match id {
                        None => {
                            row.note = Some(format!(
                                "Can't read this game's ID from a .{e} file, so it can't have its own settings file here — set it in the emulator instead (long-press the game → game settings)."
                            ))
                        }
                        Some(id) => {
                            let (path, section) = if pg.kind == "dolphin" {
                                (format!("{}/GameSettings/{id}.ini", pl.dolphin_user), "Video_Settings")
                            } else {
                                match &pl.ppsspp_dir {
                                    Some(d) => (format!("{d}/{id}_ppsspp.ini"), "Graphics"),
                                    None => {
                                        row.note = Some("PPSSPP's settings weren't found — open PPSSPP once.".into());
                                        row.id = Some(id);
                                        rows.push(row);
                                        continue;
                                    }
                                }
                            };
                            let key = pg.key.as_deref().unwrap_or_default();
                            row.own_value = fs.read(&path).ok().flatten().and_then(|t| ini_get(&t, section, key));
                            row.settings_file = Some(path);
                            row.id = Some(id);
                        }
                    }
                }
                _ => {}
            }
            if let Some(genre) = genre_of(system, &name) {
                let genres: Vec<String> = genre
                    .split([',', ';'])
                    .map(|g| g.trim().to_lowercase())
                    .collect();
                if let Some(r) = rules.iter().find(|r| {
                    r.systems.contains(system)
                        && r.genres_any
                            .iter()
                            .any(|g| genres.contains(&g.to_lowercase()))
                }) {
                    row.suggestion = Some(Suggestion {
                        value: r.value.clone(),
                        why: format!("{} (genre: {genre})", r.why),
                        tested: r.tested,
                    });
                }
            }
            rows.push(row);
        }
    }
    rows
}

// ---------------------------------------------------------------- writing

/// RomCrate's list on the handheld: ("created" | "changed", path).
pub(crate) fn manifest(fs: &mut dyn DeviceFs) -> Vec<(String, String)> {
    fs.read(MANIFEST)
        .ok()
        .flatten()
        .map(|t| {
            t.lines()
                .filter_map(|l| l.split_once(' '))
                .filter(|(k, _)| *k == "created" || *k == "changed")
                .map(|(k, p)| (k.to_string(), p.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn save_manifest(fs: &mut dyn DeviceFs, list: &[(String, String)]) -> anyhow::Result<()> {
    let mut t = String::from("# Per-game settings files RomCrate created or changed. \"Undo RomCrate's changes\" in RomCrate puts them back.\n");
    for (k, p) in list {
        t.push_str(&format!("{k} {p}\n"));
    }
    fs.write(MANIFEST, &t)
}

/// One change: give `row` its own value, or `None` to go back to the console value.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub system: String,
    pub file: String,
    pub settings_file: String,
    pub value: Option<String>,
}

fn new_file_text(
    fs: &mut dyn DeviceFs,
    pl: &Places,
    prof: &Profile,
    pg: &PerGame,
    value: &str,
) -> String {
    let key = pg.key.as_deref().unwrap_or_default();
    match pg.kind.as_str() {
        "retroarch-opt" => {
            let (cfg, shared, _) = pl.retroarch.clone().unwrap_or_default();
            let core = prof.core.clone().unwrap_or_default();
            let base = fs
                .read(&format!("{cfg}/{core}/{core}.opt"))
                .ok()
                .flatten()
                .map(|t| {
                    t.lines()
                        .filter(|l| !l.starts_with('#'))
                        .map(|l| format!("{l}\n"))
                        .collect::<String>()
                })
                .or_else(|| {
                    let prefix = key
                        .find(['-', '_'])
                        .map(|i| key[..=i].to_string())
                        .unwrap_or_default();
                    fs.read(&shared).ok().flatten().map(|g| {
                        g.lines()
                            .filter(|l| l.trim_start().starts_with(prefix.as_str()))
                            .map(|l| format!("{l}\n"))
                            .collect()
                    })
                })
                .unwrap_or_default();
            cfg_set(
                &format!("{}\n{base}", crate::gamesettings::MARKER),
                key,
                value,
            )
            .0
        }
        "dolphin" => {
            format!("# Written by RomCrate (Game settings)\n[Video_Settings]\n{key} = {value}\n")
        }
        "ppsspp" => {
            let mut t = format!("[Graphics]\n{key} = {value}\n");
            // Keep this game's controls the same as everywhere else.
            if let Some(dir) = &pl.ppsspp_dir {
                if let Ok(Some(c)) = fs.read(&format!("{dir}/controls.ini")) {
                    let mut in_sec = false;
                    let mut sec = String::new();
                    for l in c.lines() {
                        let tl = l.trim();
                        if tl.starts_with('[') {
                            in_sec = tl.eq_ignore_ascii_case("[ControlMapping]");
                        }
                        if in_sec {
                            sec.push_str(l);
                            sec.push('\n');
                        }
                    }
                    if !sec.is_empty() {
                        t.push('\n');
                        t.push_str(&sec);
                    }
                }
            }
            t
        }
        _ => String::new(),
    }
}

/// Apply per-game choices. Emulators involved are closed first.
pub fn apply(fs: &mut dyn DeviceFs, choices: &[Choice]) -> ApplyResult {
    let mut res = ApplyResult::default();
    let pl = places(fs);
    let mut created = manifest(fs);
    let mut stopped = HashSet::new();
    for c in choices {
        let Some(prof) = profile_for(&c.system) else {
            continue;
        };
        let Some(pg) = per_game_of(prof) else {
            continue;
        };
        if pg.kind == "checklist" {
            continue;
        }
        // Only ever touch the file this game's settings belong in.
        let expected_dir_ok = match pg.kind.as_str() {
            "retroarch-opt" => pl
                .retroarch
                .as_ref()
                .is_some_and(|(cfg, _, _)| c.settings_file.starts_with(cfg.as_str())),
            "dolphin" => c
                .settings_file
                .starts_with(&format!("{}/GameSettings/", pl.dolphin_user)),
            "ppsspp" => pl.ppsspp_dir.as_ref().is_some_and(|d| {
                c.settings_file.starts_with(d.as_str()) && c.settings_file.ends_with("_ppsspp.ini")
            }),
            _ => false,
        };
        if !expected_dir_ok || c.settings_file.contains("..") {
            res.failed += 1;
            res.lines.push(format!(
                "FAILED {}: not a settings file RomCrate manages ({})",
                stem(&c.file),
                c.settings_file
            ));
            continue;
        }
        let pkgs: Vec<String> = match pg.kind.as_str() {
            "retroarch-opt" => crate::gamesettings::RETROARCH_PACKAGES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            _ => prof.packages.clone(),
        };
        for p in pkgs {
            if stopped.insert(p.clone()) {
                fs.stop_app(&p);
            }
        }
        let path = c.settings_file.clone();
        let key = pg.key.clone().unwrap_or_default();
        let section = match pg.kind.as_str() {
            "dolphin" => "Video_Settings",
            "ppsspp" => "Graphics",
            _ => "",
        };
        let r = (|| -> anyhow::Result<String> {
            let existing = fs.read(&path)?;
            let ours = created.iter().any(|(k, p)| k == "created" && p == &path);
            match (&c.value, existing) {
                (None, None) => Ok("already uses the console setting".into()),
                (None, Some(_)) if ours => {
                    fs.remove(&path)?;
                    created.retain(|(_, p)| p != &path);
                    Ok("back to the console setting (removed RomCrate's file)".into())
                }
                (None, Some(text)) => {
                    let backup = format!("{path}{BACKUP_SUFFIX}");
                    if let Some(orig) = fs.read(&backup)? {
                        fs.write(&path, &orig)?;
                        fs.remove(&backup)?;
                        created.retain(|(_, p)| p != &path);
                        Ok("back to how it was (restored the backup)".into())
                    } else {
                        // Their own file without a backup: just drop our key.
                        let kept: String = text
                            .lines()
                            .filter(|l| {
                                l.split_once('=')
                                    .map(|(k, _)| k.trim() != key)
                                    .unwrap_or(true)
                            })
                            .map(|l| format!("{l}\n"))
                            .collect();
                        fs.write(&path, &kept)?;
                        Ok("back to the console setting".into())
                    }
                }
                (Some(v), None) => {
                    let text = new_file_text(fs, &pl, prof, &pg, v);
                    fs.write(&path, &text)?;
                    anyhow::ensure!(
                        fs.read(&path)?.as_deref() == Some(text.as_str()),
                        "didn't read back the same"
                    );
                    created.push(("created".into(), path.clone()));
                    Ok(format!("set to {v}"))
                }
                (Some(v), Some(text)) => {
                    let backup = format!("{path}{BACKUP_SUFFIX}");
                    if !ours && fs.read(&backup)?.is_none() {
                        fs.copy(&path, &backup)?;
                        created.push(("changed".into(), path.clone()));
                    }
                    let new = if pg.kind == "retroarch-opt" {
                        cfg_set(&text, &key, v).0
                    } else {
                        ini_set(&text, section, &key, v).0
                    };
                    fs.write(&path, &new)?;
                    anyhow::ensure!(
                        fs.read(&path)?.as_deref() == Some(new.as_str()),
                        "didn't read back the same"
                    );
                    Ok(format!("set to {v}"))
                }
            }
        })();
        match r {
            Ok(what) => {
                res.changed_files += 1;
                res.lines.push(format!(
                    "OK {} ({}): {what} — {path}",
                    stem(&c.file),
                    c.system
                ));
            }
            Err(e) => {
                res.failed += 1;
                res.lines
                    .push(format!("FAILED {} ({}): {e}", stem(&c.file), c.system));
            }
        }
    }
    if let Err(e) = save_manifest(fs, &created) {
        res.lines.push(format!(
            "Couldn't update RomCrate's list of created files: {e}"
        ));
    }
    res
}

/// Undo every per-game change: remove files RomCrate created (from the
/// manifest) and restore backups next to them.
pub fn undo_all(fs: &mut dyn DeviceFs) -> ApplyResult {
    let mut res = ApplyResult::default();
    let mut left = Vec::new();
    for (kind, p) in manifest(fs) {
        let r = (|| -> anyhow::Result<&'static str> {
            if kind == "created" {
                fs.remove(&p)?;
                return Ok("removed (RomCrate had created it)");
            }
            let backup = format!("{p}{BACKUP_SUFFIX}");
            match fs.read(&backup)? {
                Some(orig) => {
                    fs.write(&p, &orig)?;
                    fs.remove(&backup)?;
                    Ok("put back the original")
                }
                None => Ok("no backup found — left as it is"),
            }
        })();
        match r {
            Ok(what) => {
                res.changed_files += 1;
                res.lines.push(format!("OK {what}: {p}"));
            }
            Err(e) => {
                res.failed += 1;
                res.lines.push(format!("FAILED {p}: {e}"));
                left.push((kind, p));
            }
        }
    }
    if let Err(e) = save_manifest(fs, &left) {
        res.lines.push(format!(
            "Couldn't update RomCrate's list of changed files: {e}"
        ));
    }
    res
}
