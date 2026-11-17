//! Graphics and performance settings per console, applied to the emulators on
//! an Android handheld over USB (see `adb.rs`).
//!
//! Safety rules:
//! * Only the keys in `data/game_settings.json` are changed; every other line
//!   of a settings file stays exactly as it was.
//! * Before RomCrate first changes a file it keeps the original as
//!   `<file>.romcrate-backup` (never overwritten later), so "Undo" can always
//!   put things back. Files RomCrate creates start with a "Written by RomCrate"
//!   line, and Undo deletes them.
//! * Every written file is read back and compared.
//! * Saves, BIOS files and controller settings are never touched.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

pub const BACKUP_SUFFIX: &str = ".romcrate-backup";
pub const MARKER: &str = "# Written by RomCrate (Game settings). Delete this file to undo.";

// ---------------------------------------------------------------- data

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Setting {
    #[serde(default)]
    pub section: Option<String>,
    pub key: String,
    pub value: String,
    pub label: String,
    pub why: String,
    #[serde(default)]
    pub tested: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Shader {
    pub preset: String,
    pub label: String,
    pub why: String,
    #[serde(default)]
    pub tested: bool,
}

/// One picture style for a RetroArch console: a shader preset per video driver.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderStyle {
    pub id: String,
    pub label: String,
    /// Preset under `shaders_slang/` (Vulkan), without `.slangp`.
    #[serde(default)]
    pub slang: Option<String>,
    /// Preset under `shaders_glsl/` (OpenGL), without `.glslp`.
    #[serde(default)]
    pub glsl: Option<String>,
    pub why: String,
    /// A short "great for …" line.
    #[serde(default)]
    pub hint: Option<String>,
    /// May slow down weaker devices.
    #[serde(default)]
    pub heavy: bool,
    #[serde(default)]
    pub tested: bool,
    #[serde(default)]
    pub default: bool,
}

/// The built-in "no shader" style: RomCrate's preset is removed.
pub const STYLE_NONE: &str = "none";
/// Styles where RetroArch's own bilinear filter stays as it is.
const STYLES_KEEP_FILTER: &[&str] = &["sharp", STYLE_NONE];

impl ShaderStyle {
    /// The preset for this video driver ("vulkan" → slang, anything else → glsl).
    pub fn preset_for(&self, driver: &str) -> Option<(&'static str, &'static str, &str)> {
        if driver == "vulkan" {
            self.slang.as_deref().map(|p| ("shaders_slang", "slangp", p))
        } else {
            self.glsl.as_deref().map(|p| ("shaders_glsl", "glslp", p))
        }
    }
}

/// A style as offered for one console on one handheld.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleOption {
    pub id: String,
    pub label: String,
    pub why: String,
    pub hint: Option<String>,
    pub heavy: bool,
    pub tested: bool,
    /// False when the handheld's video driver can't use it.
    pub available: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IniFile {
    pub find: Vec<String>,
    pub set: Vec<Setting>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub systems: Vec<String>,
    pub emulator: String,
    /// "retroarch-core", "ini" or "checklist"
    pub kind: String,
    #[serde(default)]
    pub core: Option<String>,
    #[serde(default)]
    pub r#override: Vec<Setting>,
    #[serde(default)]
    pub options: Vec<Setting>,
    #[serde(default)]
    pub options_if_driver: BTreeMap<String, Vec<Setting>>,
    #[serde(default)]
    pub note_if_other_driver: Option<String>,
    /// Old single shader (read as the "sharp" style).
    #[serde(default)]
    pub shader: Option<Shader>,
    /// Picture styles to choose from (RetroArch consoles).
    #[serde(default)]
    pub shader_styles: Vec<ShaderStyle>,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub files: Vec<IniFile>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub why_checklist: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub tested: bool,
    pub source: String,
    #[serde(default)]
    pub per_game: Option<crate::pergame::PerGame>,
}

#[derive(Debug, Deserialize)]
pub struct Db {
    pub screens: String,
    pub profiles: Vec<Profile>,
}

pub fn db() -> &'static Db {
    static DB: OnceLock<Db> = OnceLock::new();
    DB.get_or_init(|| {
        serde_json::from_str(include_str!("../data/game_settings.json"))
            .expect("data/game_settings.json is valid")
    })
}

// ---------------------------------------------------------------- text editing

fn unquote(v: &str) -> String {
    let v = v.trim();
    v.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(v)
        .to_string()
}

/// Set `key = "value"` in a RetroArch .cfg/.opt file. Returns the new text
/// and the old value. Other lines are kept exactly.
pub fn cfg_set(text: &str, key: &str, value: &str) -> (String, Option<String>) {
    let mut old = None;
    let mut out = Vec::new();
    let mut done = false;
    for line in text.lines() {
        let t = line.trim_start();
        if !done && !t.starts_with('#') {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == key {
                    old = Some(unquote(v));
                    out.push(format!("{key} = \"{value}\""));
                    done = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    if !done {
        out.push(format!("{key} = \"{value}\""));
    }
    (out.join("\n") + "\n", old)
}

pub fn cfg_get(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let t = l.trim_start();
        if t.starts_with('#') {
            return None;
        }
        let (k, v) = t.split_once('=')?;
        (k.trim() == key).then(|| unquote(v))
    })
}

/// Set `key = value` inside `[section]` of an .ini file (Dolphin, PPSSPP),
/// adding the section or key if missing. Other lines are kept exactly.
pub fn ini_set(text: &str, section: &str, key: &str, value: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let is_header = |l: &str| {
        let t = l.trim();
        t.starts_with('[') && t.ends_with(']')
    };
    let start = lines.iter().position(|l| {
        is_header(l) && l.trim()[1..l.trim().len() - 1].eq_ignore_ascii_case(section)
    });
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let Some(start) = start else {
        while out.last().is_some_and(|l| l.trim().is_empty()) {
            out.pop();
        }
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(format!("[{section}]"));
        out.push(format!("{key} = {value}"));
        return (out.join("\n") + "\n", None);
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| is_header(l))
        .map(|p| start + 1 + p)
        .unwrap_or(lines.len());
    for i in start + 1..end {
        let t = lines[i].trim_start();
        if t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == key {
                let old = v.trim().to_string();
                out[i] = format!("{key} = {value}");
                return (out.join("\n") + "\n", Some(old));
            }
        }
    }
    // Insert after the section's last non-empty line.
    let mut at = end;
    while at > start + 1 && lines[at - 1].trim().is_empty() {
        at -= 1;
    }
    out.insert(at, format!("{key} = {value}"));
    (out.join("\n") + "\n", None)
}

// ---------------------------------------------------------------- device access

/// What RomCrate needs from the handheld. Implemented over adb; tests use a folder.
pub trait DeviceFs {
    /// File contents, or `None` if there's no such file.
    fn read(&mut self, path: &str) -> anyhow::Result<Option<String>>;
    fn write(&mut self, path: &str, text: &str) -> anyhow::Result<()>;
    fn copy(&mut self, from: &str, to: &str) -> anyhow::Result<()>;
    fn remove(&mut self, path: &str) -> anyhow::Result<()>;
    fn is_dir(&mut self, path: &str) -> bool;
    /// Search shared storage for a file name (shallow, for e.g. ppsspp.ini).
    fn find_file(&mut self, name: &str) -> Option<String>;
    fn installed_packages(&mut self) -> HashSet<String>;
    fn stop_app(&mut self, package: &str);
    /// `len` bytes from `offset` of a (possibly huge) file, without copying it all.
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> anyhow::Result<Vec<u8>>;
    /// Files (not folders) directly inside a folder, as full paths.
    fn list_files(&mut self, dir: &str) -> Vec<String>;
}

/// Read+Seek over a file on the handheld, fetching 64 KB blocks as needed.
pub struct DeviceReader<'a> {
    pub fs: &'a mut dyn DeviceFs,
    pub path: String,
    pos: u64,
    cache: std::collections::HashMap<u64, Vec<u8>>,
}

const BLOCK: u64 = 64 * 1024;

impl<'a> DeviceReader<'a> {
    pub fn new(fs: &'a mut dyn DeviceFs, path: &str) -> Self {
        DeviceReader {
            fs,
            path: path.to_string(),
            pos: 0,
            cache: Default::default(),
        }
    }
}

impl std::io::Read for DeviceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let b = self.pos / BLOCK;
        if !self.cache.contains_key(&b) {
            if self.cache.len() > 64 {
                self.cache.clear();
            }
            let data = self
                .fs
                .read_range(&self.path, b * BLOCK, BLOCK as usize)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            self.cache.insert(b, data);
        }
        let data = &self.cache[&b];
        let off = (self.pos - b * BLOCK) as usize;
        if off >= data.len() {
            return Ok(0);
        }
        let n = buf.len().min(data.len() - off);
        buf[..n].copy_from_slice(&data[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for DeviceReader<'_> {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.pos = match to {
            std::io::SeekFrom::Start(p) => p,
            std::io::SeekFrom::Current(d) => self.pos.saturating_add_signed(d),
            std::io::SeekFrom::End(_) => {
                return Err(std::io::Error::other(
                    "seeking from the end isn't supported",
                ))
            }
        };
        Ok(self.pos)
    }
}

// ---------------------------------------------------------------- plan

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyChange {
    pub section: Option<String>,
    pub key: String,
    pub label: String,
    pub why: String,
    pub old: Option<String>,
    pub new: String,
    pub tested: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// False when RomCrate creates the file.
    pub exists: bool,
    /// Remove RomCrate's own file instead of writing (e.g. "No shader").
    #[serde(default)]
    pub remove: bool,
    pub keys: Vec<KeyChange>,
    #[serde(skip)]
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsolePlan {
    pub id: String,
    pub systems: Vec<String>,
    pub emulator: String,
    /// "ready", "already", "checklist", "not-installed", "problem"
    pub status: String,
    pub message: Option<String>,
    pub changes: Vec<FileChange>,
    pub steps: Vec<String>,
    pub tested: bool,
    pub source: String,
    /// Apps to close before writing.
    pub packages: Vec<String>,
    /// Picture styles you can pick for this console (RetroArch consoles).
    pub styles: Vec<StyleOption>,
    /// The style this plan is for.
    pub style: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPlan {
    pub screens: String,
    pub consoles: Vec<ConsolePlan>,
}

pub const RETROARCH_PACKAGES: &[&str] = &[
    "com.retroarch",
    "com.retroarch.aarch64",
    "com.retroarch.ra32",
];

fn dir_of(p: &str) -> &str {
    p.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

struct RetroArch {
    package: String,
    config_dir: String,
    driver: String,
    shader_dir: Option<String>,
    core_options_file: String,
}

fn find_retroarch(
    fs: &mut dyn DeviceFs,
    installed: &HashSet<String>,
) -> Result<RetroArch, (String, String)> {
    let Some(pkg) = RETROARCH_PACKAGES.iter().find(|p| installed.contains(**p)) else {
        return Err((
            "not-installed".into(),
            "RetroArch isn't installed on the handheld.".into(),
        ));
    };
    let cfg_path = format!("/storage/emulated/0/Android/data/{pkg}/files/retroarch.cfg");
    let cfg = match fs.read(&cfg_path) {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err((
                "problem".into(),
                format!("Couldn't find RetroArch's settings ({cfg_path}). Open RetroArch once, then try again."),
            ))
        }
        Err(e) => return Err(("problem".into(), format!("Couldn't read RetroArch's settings: {e}"))),
    };
    let base = dir_of(&cfg_path).to_string();
    let usable = |v: Option<String>| v.filter(|d| d.starts_with('/'));
    let mut config_dir = usable(cfg_get(&cfg, "rgui_config_directory"));
    if config_dir.is_none() {
        for cand in [
            format!("{base}/config"),
            "/storage/emulated/0/RetroArch/config".to_string(),
        ] {
            if fs.is_dir(&cand) {
                config_dir = Some(cand);
                break;
            }
        }
    }
    let Some(config_dir) = config_dir else {
        return Err((
            "problem".into(),
            "Couldn't find RetroArch's config folder. In RetroArch: Settings → Directory → Config, pick a folder, then Main Menu → Configuration File → Save Current Configuration, and try again.".into(),
        ));
    };
    let core_options_file = usable(cfg_get(&cfg, "core_options_path"))
        .unwrap_or_else(|| format!("{base}/retroarch-core-options.cfg"));
    Ok(RetroArch {
        package: pkg.to_string(),
        config_dir,
        driver: cfg_get(&cfg, "video_driver")
            .unwrap_or_else(|| "gl".into())
            .to_lowercase(),
        shader_dir: usable(cfg_get(&cfg, "video_shader_dir")),
        core_options_file,
    })
}

/// (config folder, shared core options file, video driver) of the RetroArch
/// installed on the handheld, if found.
impl Profile {
    /// The picture styles for this console. Old data with a single `shader` is
    /// read as one "sharp" style.
    pub fn styles(&self) -> Vec<ShaderStyle> {
        if !self.shader_styles.is_empty() {
            return self.shader_styles.clone();
        }
        self.shader
            .iter()
            .map(|sh| ShaderStyle {
                id: "sharp".into(),
                label: sh.label.clone(),
                slang: Some(sh.preset.clone()),
                glsl: Some(sh.preset.clone()),
                why: sh.why.clone(),
                hint: None,
                heavy: false,
                tested: sh.tested,
                default: true,
            })
            .collect()
    }

    pub fn default_style(&self) -> Option<String> {
        let st = self.styles();
        st.iter().find(|s| s.default).or(st.first()).map(|s| s.id.clone())
    }
}

/// Which style a preset file RomCrate wrote points at (by its `#reference`).
pub fn style_of_preset(p: &Profile, text: &str) -> Option<String> {
    let r = text.lines().find_map(|l| l.trim().strip_prefix("#reference"))?;
    let r = r.trim().trim_matches('"');
    p.styles().into_iter().find_map(|s| {
        [("shaders_slang", "slangp", &s.slang), ("shaders_glsl", "glslp", &s.glsl)]
            .iter()
            .any(|(f, e, pr)| pr.as_ref().is_some_and(|pr| r.ends_with(&format!("{f}/{pr}.{e}"))))
            .then(|| s.id.clone())
    })
}

/// Where the preset for `style` is on the handheld, or why it can't be used.
pub enum PresetFound {
    At(String),
    NotForDriver,
    Missing { updater: &'static str },
}

pub fn find_preset(fs: &mut dyn DeviceFs, driver: &str, shader_dir: Option<&str>, style: &ShaderStyle) -> PresetFound {
    let Some((folder, ext, preset)) = style.preset_for(driver) else {
        return PresetFound::NotForDriver;
    };
    let updater = if driver == "vulkan" { "Slang" } else { "GLSL" };
    match shader_dir.and_then(|d| {
        let f = format!("{d}/{folder}/{preset}.{ext}");
        fs.read(&f).ok().flatten().map(|_| f)
    }) {
        Some(f) => PresetFound::At(f),
        None => PresetFound::Missing { updater },
    }
}

pub fn preset_text(found: &str) -> String {
    format!("{MARKER}\n#reference \"{found}\"\n")
}

/// RetroArch's config folder, video driver and shader folder on the handheld.
pub fn retroarch_shader_info(fs: &mut dyn DeviceFs) -> Option<(String, String, Option<String>)> {
    let installed = fs.installed_packages();
    find_retroarch(fs, &installed)
        .ok()
        .map(|r| (r.config_dir, r.driver, r.shader_dir))
}

pub fn retroarch_paths(
    fs: &mut dyn DeviceFs,
    installed: &HashSet<String>,
) -> Option<(String, String, String)> {
    find_retroarch(fs, installed)
        .ok()
        .map(|r| (r.config_dir, r.core_options_file, r.driver))
}

fn change_from(
    fs: &mut dyn DeviceFs,
    path: &str,
    settings: &[Setting],
    cfg_style: bool,
    seed: impl FnOnce(&mut dyn DeviceFs) -> String,
) -> anyhow::Result<Option<FileChange>> {
    let existing = fs.read(path)?;
    let exists = existing.is_some();
    let mut text = existing.unwrap_or_else(|| seed(fs));
    let mut keys = Vec::new();
    for s in settings {
        let (t, old) = if cfg_style {
            cfg_set(&text, &s.key, &s.value)
        } else {
            ini_set(&text, s.section.as_deref().unwrap_or(""), &s.key, &s.value)
        };
        text = t;
        if old.as_deref() == Some(s.value.as_str()) {
            continue;
        }
        keys.push(KeyChange {
            section: s.section.clone(),
            key: s.key.clone(),
            label: s.label.clone(),
            why: s.why.clone(),
            old,
            new: s.value.clone(),
            tested: s.tested,
        });
    }
    if keys.is_empty() {
        return Ok(None);
    }
    Ok(Some(FileChange {
        remove: false,
        path: path.to_string(),
        exists,
        keys,
        new_text: text,
    }))
}

/// Work out what would change for the consoles in `systems`.
pub fn plan(fs: &mut dyn DeviceFs, systems: &[String]) -> SettingsPlan {
    plan_profiles(fs, systems, &db().profiles, &BTreeMap::new())
}

/// Like `plan`, with a picture style chosen per console (profile id → style id;
/// consoles not listed get their default style).
pub fn plan_styled(fs: &mut dyn DeviceFs, systems: &[String], styles: &BTreeMap<String, String>) -> SettingsPlan {
    plan_profiles(fs, systems, &db().profiles, styles)
}

/// Like `plan`, for any list of profiles (play modes build their own).
pub fn plan_profiles(
    fs: &mut dyn DeviceFs,
    systems: &[String],
    profiles: &[Profile],
    chosen_styles: &BTreeMap<String, String>,
) -> SettingsPlan {
    let want: HashSet<&str> = systems.iter().map(String::as_str).collect();
    let installed = fs.installed_packages();
    let mut ra: Option<Result<RetroArch, (String, String)>> = None;
    let mut consoles = Vec::new();
    for p in profiles
        .iter()
        .filter(|p| p.systems.iter().any(|s| want.contains(s.as_str())))
    {
        let mut c = ConsolePlan {
            id: p.id.clone(),
            systems: p
                .systems
                .iter()
                .filter(|s| want.contains(s.as_str()))
                .cloned()
                .collect(),
            emulator: p.emulator.clone(),
            status: "ready".into(),
            message: p.note.clone(),
            changes: vec![],
            steps: p.steps.clone(),
            tested: p.tested,
            source: p.source.clone(),
            packages: p.packages.clone(),
            styles: vec![],
            style: None,
        };
        let result: Result<(), (String, String)> = (|| {
            match p.kind.as_str() {
                "checklist" => {
                    return Err((
                        "checklist".into(),
                        p.why_checklist.clone().unwrap_or_default(),
                    ));
                }
                "retroarch-core" => {
                    let r = ra.get_or_insert_with(|| find_retroarch(fs, &installed));
                    let r = r.as_ref().map_err(|e| e.clone())?;
                    c.packages = vec![r.package.clone()];
                    let core = p.core.as_deref().unwrap_or_default();
                    let dir = format!("{}/{core}", r.config_dir);
                    let err = |e: anyhow::Error| {
                        (
                            "problem".to_string(),
                            format!("Couldn't read the handheld: {e}"),
                        )
                    };
                    // Picture style: which one, and what this handheld can use.
                    let styles = p.styles();
                    let style = (!styles.is_empty()).then(|| {
                        chosen_styles
                            .get(&p.id)
                            .filter(|id| *id == STYLE_NONE || styles.iter().any(|s| &s.id == *id))
                            .cloned()
                            .or_else(|| p.default_style())
                            .unwrap_or_else(|| STYLE_NONE.into())
                    });
                    c.styles = styles
                        .iter()
                        .map(|s| StyleOption {
                            id: s.id.clone(),
                            label: s.label.clone(),
                            why: s.why.clone(),
                            hint: s.hint.clone(),
                            heavy: s.heavy,
                            tested: s.tested,
                            available: s.preset_for(&r.driver).is_some(),
                        })
                        .collect();
                    if !c.styles.is_empty() {
                        c.styles.push(StyleOption {
                            id: STYLE_NONE.into(),
                            label: "No shader".into(),
                            why: "A plain picture, as RetroArch shows it by default.".into(),
                            hint: None,
                            heavy: false,
                            tested: true,
                            available: true,
                        });
                    }
                    c.style = style.clone();
                    let mut over = p.r#override.clone();
                    if let Some(st) = style.as_deref().filter(|s| !STYLES_KEEP_FILTER.contains(s)) {
                        let usable = styles.iter().find(|x| x.id == st).is_some_and(|x| x.preset_for(&r.driver).is_some());
                        if usable {
                            over.push(Setting {
                                section: None,
                                key: "video_smooth".into(),
                                value: "false".into(),
                                label: "RetroArch's own smoothing off".into(),
                                why: "The picture style does the smoothing; RetroArch's blur on top would make it soft.".into(),
                                tested: false,
                            });
                        }
                    }
                    if !over.is_empty() {
                        let path = format!("{dir}/{core}.cfg");
                        if let Some(ch) =
                            change_from(fs, &path, &over, true, |_| format!("{MARKER}\n"))
                                .map_err(err)?
                        {
                            c.changes.push(ch);
                        }
                    }
                    let mut opts = p.options.clone();
                    match p.options_if_driver.get(&r.driver) {
                        Some(extra) => opts.extend(extra.iter().cloned()),
                        None if !p.options_if_driver.is_empty() => {
                            c.message = p.note_if_other_driver.clone();
                        }
                        None => {}
                    }
                    if !opts.is_empty() {
                        let path = format!("{dir}/{core}.opt");
                        // A new per-core options file replaces the shared one for this
                        // core, so start it with this core's current shared options.
                        let prefixes: Vec<String> = opts
                            .iter()
                            .filter_map(|o| o.key.find(['-', '_']).map(|i| o.key[..=i].to_string()))
                            .collect();
                        let shared = r.core_options_file.clone();
                        let seed = move |fs: &mut dyn DeviceFs| {
                            let mut t = format!("{MARKER}\n");
                            if let Ok(Some(g)) = fs.read(&shared) {
                                for l in g.lines() {
                                    if prefixes
                                        .iter()
                                        .any(|p| l.trim_start().starts_with(p.as_str()))
                                    {
                                        t.push_str(l);
                                        t.push('\n');
                                    }
                                }
                            }
                            t
                        };
                        if let Some(ch) = change_from(fs, &path, &opts, true, seed).map_err(err)? {
                            c.changes.push(ch);
                        }
                    }
                    if let Some(style) = style.as_deref() {
                        let ext = if r.driver == "vulkan" { "slangp" } else { "glslp" };
                        let path = format!("{dir}/{core}.{ext}");
                        let existing = fs.read(&path).map_err(err)?;
                        let ours = existing.as_deref().is_none_or(|t| t.starts_with(MARKER));
                        let old_label = |t: &str| {
                            style_of_preset(p, t)
                                .and_then(|id| styles.iter().find(|s| s.id == id).map(|s| s.label.clone()))
                                .unwrap_or_else(|| "another shader".into())
                        };
                        if !ours {
                            // A preset you made yourself is never replaced.
                            c.steps.push(format!(
                                "Your own shader preset is kept ({path}). To use a RomCrate picture style for this console, remove that preset in RetroArch (Quick Menu → Shaders → Remove Core Preset), then apply again."
                            ));
                        } else if style == STYLE_NONE {
                            if let Some(t) = existing {
                                c.changes.push(FileChange {
                                    path,
                                    exists: true,
                                    remove: true,
                                    keys: vec![KeyChange {
                                        section: None,
                                        key: "shader preset".into(),
                                        label: "Picture style".into(),
                                        why: "No shader: a plain picture.".into(),
                                        old: Some(old_label(&t)),
                                        new: "No shader".into(),
                                        tested: true,
                                    }],
                                    new_text: String::new(),
                                });
                            }
                        } else if let Some(sh) = styles.iter().find(|s| s.id == style) {
                            match find_preset(fs, &r.driver, r.shader_dir.as_deref(), sh) {
                                PresetFound::At(f) => {
                                    let want_text = preset_text(&f);
                                    if existing.as_deref() != Some(want_text.as_str()) {
                                        c.changes.push(FileChange {
                                            path,
                                            exists: existing.is_some(),
                                            remove: false,
                                            keys: vec![KeyChange {
                                                section: None,
                                                key: "shader preset".into(),
                                                label: format!("Picture style: {}", sh.label),
                                                why: sh.why.clone(),
                                                old: existing.as_deref().map(old_label),
                                                new: sh.label.clone(),
                                                tested: sh.tested,
                                            }],
                                            new_text: want_text,
                                        });
                                    }
                                }
                                PresetFound::NotForDriver => c.steps.push(format!(
                                    "“{}” needs RetroArch's Vulkan video driver, and this handheld uses {}. The picture style was left as it is.",
                                    sh.label, r.driver
                                )),
                                PresetFound::Missing { updater } => c.steps.push(format!(
                                    "For the “{}” look: in RetroArch, Main Menu → Online Updater → Update {updater} Shaders, then apply Game settings again.",
                                    sh.label
                                )),
                            }
                        }
                    }
                }
                "ini" => {
                    if !p.packages.iter().any(|x| installed.contains(x)) {
                        return Err((
                            "not-installed".into(),
                            format!("{} isn't installed on the handheld.", p.emulator),
                        ));
                    }
                    for f in &p.files {
                        let mut path = None;
                        for cand in &f.find {
                            if let Some(name) = cand.strip_prefix("search:") {
                                path = fs.find_file(name);
                            } else if fs.read(cand).ok().flatten().is_some() {
                                path = Some(cand.clone());
                            }
                            if path.is_some() {
                                break;
                            }
                        }
                        let Some(path) = path else {
                            return Err((
                                "problem".into(),
                                format!("Couldn't find {}'s settings file. Open {} once (and start any game), then try again.", p.emulator, p.emulator),
                            ));
                        };
                        let ch = change_from(fs, &path, &f.set, false, |_| String::new()).map_err(
                            |e| {
                                (
                                    "problem".to_string(),
                                    format!("Couldn't read the handheld: {e}"),
                                )
                            },
                        )?;
                        if let Some(ch) = ch {
                            c.changes.push(ch);
                        }
                    }
                }
                other => return Err(("problem".into(), format!("unknown kind {other}"))),
            }
            Ok(())
        })();
        if let Err((status, msg)) = result {
            c.status = status;
            c.message = Some(msg);
        } else if c.changes.is_empty() {
            c.status = "already".into();
        }
        consoles.push(c);
    }
    SettingsPlan {
        screens: db().screens.clone(),
        consoles,
    }
}

// ---------------------------------------------------------------- apply / undo

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub changed_files: usize,
    pub failed: usize,
    pub lines: Vec<String>,
}

/// Write the changes for the chosen consoles (ids), closing the emulators first.
pub fn apply(fs: &mut dyn DeviceFs, plan: &SettingsPlan, ids: &[String]) -> ApplyResult {
    let mut res = ApplyResult::default();
    let chosen: Vec<&ConsolePlan> = plan
        .consoles
        .iter()
        .filter(|c| c.status == "ready" && ids.contains(&c.id))
        .collect();
    let pkgs: HashSet<&String> = chosen.iter().flat_map(|c| c.packages.iter()).collect();
    for p in pkgs {
        fs.stop_app(p);
        res.lines
            .push(format!("Closed {p} so it can't overwrite the new settings"));
    }
    for c in chosen {
        for ch in &c.changes {
            let r = (|| -> anyhow::Result<()> {
                // Back up the owner's original once. A file RomCrate created itself
                // (it starts with MARKER) is never "backed up": Undo deletes it.
                let ours = ch.exists && fs.read(&ch.path)?.is_some_and(|t| t.starts_with(MARKER));
                if ch.exists && !ours {
                    let backup = format!("{}{BACKUP_SUFFIX}", ch.path);
                    if fs.read(&backup)?.is_none() {
                        fs.copy(&ch.path, &backup)?;
                    }
                }
                if ch.remove {
                    // Only ever RomCrate's own file.
                    anyhow::ensure!(ours, "it isn't RomCrate's file, so it was left alone");
                    fs.remove(&ch.path)?;
                    anyhow::ensure!(fs.read(&ch.path)?.is_none(), "the file is still there");
                    return Ok(());
                }
                fs.write(&ch.path, &ch.new_text)?;
                anyhow::ensure!(
                    fs.read(&ch.path)?.as_deref() == Some(ch.new_text.as_str()),
                    "the file didn't read back the same"
                );
                Ok(())
            })();
            let what = ch
                .keys
                .iter()
                .map(|k| k.label.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            match r {
                Ok(()) => {
                    res.changed_files += 1;
                    res.lines.push(format!(
                        "OK {} — {}: {what} ({})",
                        c.emulator,
                        if ch.remove { "removed" } else if ch.exists { "changed" } else { "created" },
                        ch.path
                    ));
                }
                Err(e) => {
                    res.failed += 1;
                    res.lines
                        .push(format!("FAILED {} — {}: {e}", c.emulator, ch.path));
                }
            }
        }
    }
    res
}

/// Put back everything RomCrate changed for these consoles: restore backups,
/// delete files RomCrate created.
pub fn undo(fs: &mut dyn DeviceFs, systems: &[String]) -> ApplyResult {
    let mut res = ApplyResult::default();
    let installed = fs.installed_packages();
    let want: HashSet<&str> = systems.iter().map(String::as_str).collect();
    let mut paths: Vec<(String, String)> = Vec::new();
    let mut ra = None;
    for p in db()
        .profiles
        .iter()
        .filter(|p| p.systems.iter().any(|s| want.contains(s.as_str())))
    {
        match p.kind.as_str() {
            "retroarch-core" => {
                let r = ra.get_or_insert_with(|| find_retroarch(fs, &installed));
                if let Ok(r) = r {
                    let core = p.core.clone().unwrap_or_default();
                    for ext in ["cfg", "opt", "slangp", "glslp"] {
                        paths.push((
                            p.emulator.clone(),
                            format!("{}/{core}/{core}.{ext}", r.config_dir),
                        ));
                    }
                }
            }
            "ini" => {
                for f in &p.files {
                    for cand in &f.find {
                        if let Some(name) = cand.strip_prefix("search:") {
                            if let Some(found) = fs.find_file(name) {
                                paths.push((p.emulator.clone(), found));
                            }
                        } else {
                            paths.push((p.emulator.clone(), cand.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    paths.dedup();
    let pg = crate::pergame::undo_all(fs);
    res.changed_files += pg.changed_files;
    res.failed += pg.failed;
    res.lines.extend(pg.lines);
    for (emu, path) in paths {
        let backup = format!("{path}{BACKUP_SUFFIX}");
        let r = (|| -> anyhow::Result<Option<&'static str>> {
            if fs.read(&backup)?.is_some() {
                fs.copy(&backup, &path)?;
                fs.remove(&backup)?;
                return Ok(Some("put back the original"));
            }
            if fs.read(&path)?.is_some_and(|t| t.starts_with(MARKER)) {
                fs.remove(&path)?;
                return Ok(Some("removed (RomCrate had created it)"));
            }
            Ok(None)
        })();
        match r {
            Ok(Some(what)) => {
                res.changed_files += 1;
                res.lines.push(format!("OK {emu} — {what}: {path}"));
            }
            Ok(None) => {}
            Err(e) => {
                res.failed += 1;
                res.lines.push(format!("FAILED {emu} — {path}: {e}"));
            }
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_is_valid() {
        let d = db();
        let mut ids = HashSet::new();
        for p in &d.profiles {
            assert!(ids.insert(p.id.clone()), "duplicate {}", p.id);
            assert!(!p.source.is_empty());
            for s in &p.systems {
                assert!(crate::systems::by_id(s).is_some(), "unknown system {s}");
            }
            let styles = p.styles();
            if !styles.is_empty() {
                assert_eq!(p.kind, "retroarch-core", "{}: styles are RetroArch shaders", p.id);
                assert_eq!(styles.iter().filter(|s| s.default).count(), 1, "{}: exactly one default style", p.id);
                let mut sid = HashSet::new();
                for s in &styles {
                    assert!(sid.insert(s.id.clone()) && s.id != STYLE_NONE, "{}: style id {}", p.id, s.id);
                    assert!(!s.label.is_empty() && !s.why.is_empty(), "{}: {} needs a label and a why", p.id, s.id);
                    assert!(s.slang.is_some() || s.glsl.is_some(), "{}: {} has no preset", p.id, s.id);
                }
            }
            match p.kind.as_str() {
                "retroarch-core" => assert!(p.core.is_some()),
                "ini" => assert!(!p.files.is_empty() && !p.packages.is_empty()),
                "checklist" => assert!(!p.steps.is_empty()),
                k => panic!("kind {k}"),
            }
        }
    }

    #[test]
    fn cfg_set_changes_only_that_key() {
        let t = "# hi\nvideo_vsync = \"false\"\nvideo_driver = \"vulkan\"\n";
        let (n, old) = cfg_set(t, "video_vsync", "true");
        assert_eq!(
            n,
            "# hi\nvideo_vsync = \"true\"\nvideo_driver = \"vulkan\"\n"
        );
        assert_eq!(old.as_deref(), Some("false"));
        let (n, old) = cfg_set(&n, "aspect_ratio_index", "0");
        assert!(n.ends_with("aspect_ratio_index = \"0\"\n") && old.is_none());
        assert_eq!(cfg_get(&n, "video_driver").as_deref(), Some("vulkan"));
    }

    #[test]
    fn ini_set_keeps_everything_else() {
        let t = "[Core]\nCPUThread = True\nGFXBackend = OGL\n\n[Display]\nFullscreen = False\n";
        let (n, old) = ini_set(t, "Core", "GFXBackend", "Vulkan");
        assert_eq!(
            n,
            "[Core]\nCPUThread = True\nGFXBackend = Vulkan\n\n[Display]\nFullscreen = False\n"
        );
        assert_eq!(old.as_deref(), Some("OGL"));
        let (n2, old) = ini_set(&n, "Display", "AspectRatio", "0");
        assert_eq!(n2, "[Core]\nCPUThread = True\nGFXBackend = Vulkan\n\n[Display]\nFullscreen = False\nAspectRatio = 0\n");
        assert!(old.is_none());
        let (n3, _) = ini_set("[Core]\nA = 1\n", "Settings", "InternalResolution", "2");
        assert_eq!(n3, "[Core]\nA = 1\n\n[Settings]\nInternalResolution = 2\n");
        // Same key in another section isn't touched.
        let (n4, _) = ini_set("[A]\nX = 1\n[B]\nX = 2\n", "B", "X", "9");
        assert_eq!(n4, "[A]\nX = 1\n[B]\nX = 9\n");
    }
}
