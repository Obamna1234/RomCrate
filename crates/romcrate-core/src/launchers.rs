//! Where each launcher expects artwork, theme presets, and metadata files.

use crate::imaging::Size;
use crate::matcher::{display_title, libretro_sanitize};
use crate::paths::join_safe;
use crate::scan::{Game, ScannedSystem};
use crate::source::ArtKind;
use crate::systems::System;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LauncherKind {
    Pegasus,
    /// Pegasus reading Skraper-style folders (`media/box2dfront/<game>.png`).
    /// Chosen automatically or via [`PegasusLayout`]; not listed separately in the UI.
    PegasusSkraper,
    EsDe,
    EmulationStation,
    RetroArch,
    LaunchBox,
    Folder,
}

/// How Pegasus media is laid out on disk. Pegasus reads both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PegasusLayout {
    /// Use the Skraper layout if the system folder already has one, otherwise per-game folders.
    #[default]
    Auto,
    /// `media/<game>/boxFront.png` (Pegasus' own layout).
    PerGame,
    /// `media/box2dfront/<game>.png` (what Skraper writes; needs "Skraper Assets" enabled in Pegasus, on by default).
    Skraper,
}

/// Skraper folder names Pegasus understands, by art type.
const SKRAPER_DIRS: &[&str] = &[
    "box2dfront",
    "box2dback",
    "box3d",
    "support",
    "screenshot",
    "screenshottitle",
    "wheel",
    "fanart",
    "videos",
];

/// Resolve the layout to use for one system folder.
pub fn effective_launcher(
    launcher: LauncherKind,
    layout: PegasusLayout,
    ss: &ScannedSystem,
) -> LauncherKind {
    if launcher != LauncherKind::Pegasus {
        return launcher;
    }
    let skraper = match layout {
        PegasusLayout::PerGame => false,
        PegasusLayout::Skraper => true,
        PegasusLayout::Auto => ["media", "skraper", ".media"].iter().any(|m| {
            SKRAPER_DIRS
                .iter()
                .any(|d| ss.path.join(m).join(d).is_dir())
        }),
    };
    if skraper {
        LauncherKind::PegasusSkraper
    } else {
        LauncherKind::Pegasus
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ThemeProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Art types this look uses (pre-ticked in the UI).
    pub kinds: Vec<ArtKind>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LauncherInfo {
    pub id: LauncherKind,
    pub name: &'static str,
    pub tagline: &'static str,
    /// Art types this launcher can display.
    pub supported: Vec<ArtKind>,
    /// Whether the user must pick a separate folder (media / RetroArch / LaunchBox / output).
    pub needs_folder: bool,
    pub folder_label: &'static str,
    pub folder_hint: &'static str,
    pub themes: Vec<ThemeProfile>,
    /// Short note on where things go, shown in the UI.
    pub where_note: &'static str,
}

use ArtKind::*;

pub fn launchers() -> Vec<LauncherInfo> {
    vec![
        LauncherInfo {
            id: LauncherKind::Pegasus,
            name: "Pegasus Frontend",
            tagline: "Art goes into a media folder next to your games — Pegasus finds it automatically.",
            supported: vec![Boxart, Box3d, Boxback, Cartridge, Snap, Title, Logo, Fanart, Video],
            needs_folder: false,
            folder_label: "",
            folder_hint: "",
            themes: vec![
                ThemeProfile { id: "pegasus-showcase", name: "Showcase themes", description: "Big screenshot or video, logo and box — gameOS, Neoretro and similar.", kinds: vec![Boxart, Snap, Logo, Video] },
                ThemeProfile { id: "pegasus-3d", name: "3D box shelves", description: "3D boxes used as the cover, plus logos.", kinds: vec![Box3d, Snap, Logo] },
                ThemeProfile { id: "pegasus-grid", name: "Cover grid themes", description: "Shelves or grids of 2D box covers.", kinds: vec![Boxart, Logo] },
                ThemeProfile { id: "pegasus-all", name: "Everything", description: "Every type Pegasus can show.", kinds: vec![Boxart, Boxback, Cartridge, Snap, Title, Logo, Fanart, Video] },
            ],
            where_note: "Matches your existing layout: Skraper-style <system>/media/box2dfront/<game>.png if you already use it, otherwise <system>/media/<game>/boxFront.png. Both work in Pegasus.",
        },
        LauncherInfo {
            id: LauncherKind::EsDe,
            name: "ES-DE (EmulationStation Desktop Edition)",
            tagline: "Writes straight into ES-DE's downloaded_media folder.",
            supported: ArtKind::ALL.to_vec(),
            needs_folder: true,
            folder_label: "ES-DE downloaded_media folder",
            folder_hint: "Usually ~/ES-DE/downloaded_media (Windows: ES-DE\\downloaded_media)",
            themes: vec![
                ThemeProfile { id: "esde-full", name: "Linear / Modern (default themes)", description: "Everything ES-DE's built-in scraper would fetch.", kinds: vec![Boxart, Box3d, Boxback, Cartridge, Snap, Title, Logo, Fanart, Mix, Video, Manual] },
                ThemeProfile { id: "esde-light", name: "Lightweight", description: "Covers, screenshots, marquees and mix images — small and fast.", kinds: vec![Boxart, Snap, Logo, Mix] },
            ],
            where_note: "downloaded_media/<system>/covers, 3dboxes, backcovers, physicalmedia, screenshots, titlescreens, marquees, fanart, miximages, videos, manuals",
        },
        LauncherInfo {
            id: LauncherKind::EmulationStation,
            name: "EmulationStation (Batocera, RetroPie, Recalbox, Knulli, ArkOS…)",
            tagline: "Saves into an images folder and updates gamelist.xml (your original is backed up).",
            supported: ArtKind::ALL.to_vec(),
            needs_folder: false,
            folder_label: "",
            folder_hint: "",
            themes: vec![
                ThemeProfile { id: "es-mix", name: "Mix image (classic look)", description: "Screenshot + box + logo combined; most themes' default.", kinds: vec![Mix, Boxart, Logo, Video] },
                ThemeProfile { id: "es-3d", name: "3D box", description: "3D box as the main picture.", kinds: vec![Box3d, Snap, Logo, Video] },
                ThemeProfile { id: "es-screens", name: "Screenshot", description: "Plain screenshot as the main image.", kinds: vec![Snap, Boxart, Logo, Video] },
                ThemeProfile { id: "es-full", name: "Everything (Batocera)", description: "All media Batocera can show, including manuals.", kinds: vec![Mix, Boxart, Box3d, Boxback, Cartridge, Title, Logo, Fanart, Video, Manual] },
            ],
            where_note: "<system>/images, videos, manuals + gamelist.xml",
        },
        LauncherInfo {
            id: LauncherKind::RetroArch,
            name: "RetroArch",
            tagline: "Fills RetroArch's thumbnails folder and creates playlists if you don't have them.",
            supported: vec![Boxart, Box3d, Snap, Title, Logo],
            needs_folder: true,
            folder_label: "RetroArch folder",
            folder_hint: "The folder that contains RetroArch's 'thumbnails' and 'playlists' folders",
            themes: vec![
                ThemeProfile { id: "ra-all", name: "All thumbnails", description: "Box art, screenshot, title screen and logo.", kinds: vec![Boxart, Snap, Title, Logo] },
                ThemeProfile { id: "ra-3d", name: "3D boxes", description: "3D boxes in place of flat box art.", kinds: vec![Box3d, Snap, Title, Logo] },
            ],
            where_note: "RetroArch/thumbnails/<system>/Named_Boxarts, Named_Snaps, Named_Titles, Named_Logos",
        },
        LauncherInfo {
            id: LauncherKind::LaunchBox,
            name: "LaunchBox / Big Box",
            tagline: "Saves images, videos and manuals into LaunchBox's folders, named by game title.",
            supported: vec![Boxart, Box3d, Boxback, Cartridge, Snap, Title, Logo, Fanart, Video, Manual],
            needs_folder: true,
            folder_label: "LaunchBox folder",
            folder_hint: "The folder that contains LaunchBox.exe",
            themes: vec![ThemeProfile { id: "lb-all", name: "Standard", description: "Everything Big Box themes commonly use.", kinds: vec![Boxart, Box3d, Boxback, Cartridge, Snap, Title, Logo, Fanart, Video, Manual] }],
            where_note: "LaunchBox/Images/<platform>/<type>, LaunchBox/Videos/<platform>, LaunchBox/Manuals/<platform>",
        },
        LauncherInfo {
            id: LauncherKind::Folder,
            name: "Plain folders (any other launcher)",
            tagline: "Simple folders per system and art type — works with Daijishō, Playnite, Steam ROM Manager and more.",
            supported: ArtKind::ALL.to_vec(),
            needs_folder: true,
            folder_label: "Output folder",
            folder_hint: "Any folder you like",
            themes: vec![ThemeProfile { id: "folder-all", name: "Everything", description: "Every art type.", kinds: ArtKind::ALL.to_vec() }],
            where_note: "<output>/<system>/<type>/<game>.png (videos .mp4, manuals .pdf)",
        },
    ]
}

/// Base sizes (the "Standard" preset); aspect ratio is always kept.
pub fn base_size(kind: ArtKind) -> Size {
    match kind {
        Boxart | Boxback => Size { w: 900, h: 1200 },
        Box3d => Size { w: 1000, h: 1200 },
        Cartridge => Size { w: 900, h: 900 },
        Snap | Title | Mix => Size { w: 1280, h: 960 },
        Logo => Size { w: 1000, h: 500 },
        Fanart => Size { w: 1920, h: 1080 },
        Video | Manual => Size { w: 0, h: 0 },
    }
}

/// Size presets offered in the UI.
pub fn size_presets() -> Vec<(&'static str, &'static str, f32)> {
    vec![
        (
            "compact",
            "Compact — handhelds & small screens (≈640p)",
            0.6,
        ),
        ("standard", "Standard — 1080p TVs and monitors", 1.0),
        ("hd", "High — 1440p", 1.5),
        ("uhd", "Ultra — 4K", 2.0),
    ]
}

/// LaunchBox replaces these characters with `_` in image file names.
fn launchbox_sanitize(title: &str) -> String {
    title
        .chars()
        .map(|c| if ":/\\?*\"<>|'".contains(c) { '_' } else { c })
        .collect()
}

/// Subfolder of a game relative to its system folder, e.g. "Europe" for "Europe/Game.zip".
fn rel_dir(game: &Game) -> String {
    match game.rel_path.rfind('/') {
        Some(i) => game.rel_path[..i].to_string(),
        None => String::new(),
    }
}

/// Paths a piece of art should be written to (first = primary).
/// `selected` is every kind the user picked; some launchers have one slot for
/// two kinds (e.g. Pegasus `boxFront` for 2D or 3D boxes) and the 3D box wins.
#[allow(clippy::too_many_arguments)]
pub fn art_targets(
    launcher: LauncherKind,
    folder: Option<&Path>,
    sys: &System,
    ss: &ScannedSystem,
    game: &Game,
    kind: ArtKind,
    matched_name: Option<&str>,
    selected: &[ArtKind],
) -> Vec<PathBuf> {
    let stem = game.stem.as_str();
    let ext = kind.ext();
    let both_boxes = selected.contains(&Boxart) && selected.contains(&Box3d);
    match launcher {
        LauncherKind::Pegasus => {
            let asset = match kind {
                Boxart if both_boxes => return vec![],
                Boxart | Box3d => "boxFront",
                Boxback => "boxBack",
                Cartridge => "cartridge",
                Snap => "screenshot",
                Title => "titlescreen",
                Logo => "logo",
                Fanart => "background",
                Video => "video",
                Mix | Manual => return vec![],
            };
            vec![join_safe(
                &ss.path,
                &["media", stem, &format!("{asset}.{ext}")],
            )]
        }
        LauncherKind::PegasusSkraper => {
            // Pegasus' Skraper support: box2dfront wins over box3d for the cover,
            // so each type gets its own folder and nothing collides.
            let dir = match kind {
                Boxart => "box2dfront",
                Box3d => "box3d",
                Boxback => "box2dback",
                Cartridge => "support",
                Snap => "screenshot",
                Title => "screenshottitle",
                Logo => "wheel",
                Fanart => "fanart",
                Video => "videos",
                Mix | Manual => return vec![],
            };
            vec![join_safe(
                &ss.path,
                &["media", dir, &rel_dir(game), &format!("{stem}.{ext}")],
            )]
        }
        LauncherKind::EsDe => {
            let Some(root) = folder else { return vec![] };
            let dir = match kind {
                Boxart => "covers",
                Box3d => "3dboxes",
                Boxback => "backcovers",
                Cartridge => "physicalmedia",
                Snap => "screenshots",
                Title => "titlescreens",
                Logo => "marquees",
                Fanart => "fanart",
                Mix => "miximages",
                Video => "videos",
                Manual => "manuals",
            };
            vec![join_safe(
                root,
                &[
                    &ss.folder_name,
                    dir,
                    &rel_dir(game),
                    &format!("{stem}.{ext}"),
                ],
            )]
        }
        LauncherKind::EmulationStation => {
            let (dir, suffix) = match kind {
                Boxart => ("images", "thumb"),
                Box3d => ("images", "box3d"),
                Boxback => ("images", "boxback"),
                Cartridge => ("images", "cartridge"),
                Snap => ("images", "snap"),
                Title => ("images", "titleshot"),
                Logo => ("images", "marquee"),
                Fanart => ("images", "fanart"),
                Mix => ("images", "mix"),
                Video => ("videos", "video"),
                Manual => ("manuals", "manual"),
            };
            vec![join_safe(
                &ss.path,
                &[dir, &format!("{stem}-{suffix}.{ext}")],
            )]
        }
        LauncherKind::RetroArch => {
            let Some(root) = folder else { return vec![] };
            let dir = match kind {
                Boxart if both_boxes => return vec![],
                Box3d => "Named_Boxarts",
                k => match k.libretro_dir() {
                    Some(d) => d,
                    None => return vec![],
                },
            };
            let mut v = vec![join_safe(
                root,
                &[
                    "thumbnails",
                    sys.libretro,
                    dir,
                    &format!("{}.png", libretro_sanitize(stem)),
                ],
            )];
            // Playlists made by RetroArch's own scanner use the database name as label.
            if let Some(m) = matched_name {
                if m != stem {
                    v.push(join_safe(
                        root,
                        &["thumbnails", sys.libretro, dir, &format!("{m}.png")],
                    ));
                }
            }
            v
        }
        LauncherKind::LaunchBox => {
            let Some(root) = folder else { return vec![] };
            let title = launchbox_sanitize(&display_title(stem));
            let file = format!("{title}-01.{ext}");
            let sub = match kind {
                Boxart => "Box - Front",
                Box3d => "Box - 3D",
                Boxback => "Box - Back",
                Cartridge => "Cart - Front",
                Snap => "Screenshot - Gameplay",
                Title => "Screenshot - Game Title",
                Logo => "Clear Logo",
                Fanart => "Fanart - Background",
                Video => return vec![join_safe(root, &["Videos", sys.launchbox, &file])],
                Manual => return vec![join_safe(root, &["Manuals", sys.launchbox, &file])],
                Mix => return vec![],
            };
            vec![join_safe(root, &["Images", sys.launchbox, sub, &file])]
        }
        LauncherKind::Folder => {
            let Some(root) = folder else { return vec![] };
            vec![join_safe(
                root,
                &[
                    &ss.folder_name,
                    kind.id(),
                    &rel_dir(game),
                    &format!("{stem}.{ext}"),
                ],
            )]
        }
    }
}

/// Art produced for one game, used to write metadata files.
#[derive(Debug, Clone)]
pub struct GameArt {
    pub game: Game,
    pub files: BTreeMap<ArtKind, PathBuf>,
    /// Description, genre, release date… when a database knew the game.
    pub info: Option<crate::launchbox::GameInfo>,
    /// Database name the game was identified as (No-Intro/Redump/libretro), if any.
    pub matched: Option<String>,
}

impl GameArt {
    /// Best human title: database info, then the identified name, then the file name.
    /// (A badly named "game disc 1.iso" identified by its serial shows its real title.)
    pub fn title(&self) -> String {
        if let Some(i) = &self.info {
            return i.title.clone();
        }
        match &self.matched {
            Some(m) if !m.trim().is_empty() => display_title(m),
            _ => display_title(&self.game.stem),
        }
    }
}

/// Just the Pegasus launch lines, without scraping: creates metadata files that
/// are missing (with the launch block) and, if `choice.fix_existing`, adds or
/// fixes the launch block in existing ones (backup first). Returns notes.
pub fn write_launch_lines(
    lib: &crate::scan::Library,
    choice: &crate::launch::LaunchChoice,
) -> Vec<String> {
    let mut notes = Vec::new();
    for ss in &lib.systems {
        let Some(sys) = crate::systems::by_id(&ss.system_id) else {
            continue;
        };
        match finalize_system_with(LauncherKind::Pegasus, None, sys, ss, &[], false, choice) {
            Ok(n) => notes.extend(n),
            Err(e) => notes.push(format!("{}: {e}", ss.system_name)),
        }
    }
    notes
}

/// Write/update launcher metadata for a system after scraping. Returns notes for the log.
pub fn finalize_system(
    launcher: LauncherKind,
    folder: Option<&Path>,
    sys: &System,
    ss: &ScannedSystem,
    results: &[GameArt],
    overwrite: bool,
) -> anyhow::Result<Vec<String>> {
    finalize_system_with(
        launcher,
        folder,
        sys,
        ss,
        results,
        overwrite,
        &crate::launch::LaunchChoice::default(),
    )
}

/// Like [`finalize_system`], with Pegasus launch lines.
pub fn finalize_system_with(
    launcher: LauncherKind,
    folder: Option<&Path>,
    sys: &System,
    ss: &ScannedSystem,
    results: &[GameArt],
    overwrite: bool,
    launch: &crate::launch::LaunchChoice,
) -> anyhow::Result<Vec<String>> {
    match launcher {
        LauncherKind::EmulationStation => write_gamelist(
            &ss.path.join("gamelist.xml"),
            &ss.path,
            ss,
            results,
            true,
            overwrite,
        ),
        LauncherKind::EsDe => match folder.and_then(es_de_gamelist_dir) {
            // ES-DE keeps game info in <ES-DE>/gamelists/<system>/gamelist.xml.
            Some(dir) => {
                let d = join_safe(&dir, &[&ss.folder_name]);
                std::fs::create_dir_all(&d)?;
                write_gamelist(
                    &d.join("gamelist.xml"),
                    &ss.path,
                    ss,
                    results,
                    false,
                    overwrite,
                )
            }
            None => Ok(vec![]),
        },
        LauncherKind::Pegasus | LauncherKind::PegasusSkraper => {
            write_pegasus_metadata(sys, ss, results, launch)
        }
        LauncherKind::RetroArch => match folder {
            Some(f) => write_retroarch_playlist(f, sys, ss),
            None => Ok(vec![]),
        },
        _ => Ok(vec![]),
    }
}

// ---------------------------------------------------------------- Pegasus

/// "2000-08-11" → "20000811T000000"; "1992" → "19920101T000000".
fn es_date(d: &str) -> Option<String> {
    let digits: String = d.chars().filter(|c| c.is_ascii_digit()).take(8).collect();
    match digits.len() {
        8 => Some(format!("{digits}T000000")),
        6 => Some(format!("{digits}01T000000")),
        4 => Some(format!("{digits}0101T000000")),
        _ => None,
    }
}

/// `<ES-DE>/gamelists` when the media folder is ES-DE's `downloaded_media`.
fn es_de_gamelist_dir(media: &Path) -> Option<PathBuf> {
    let name = media.file_name()?.to_str()?;
    name.eq_ignore_ascii_case("downloaded_media")
        .then(|| media.parent().map(|p| p.join("gamelists")))
        .flatten()
}

fn write_pegasus_metadata(
    sys: &System,
    ss: &ScannedSystem,
    results: &[GameArt],
    launch: &crate::launch::LaunchChoice,
) -> anyhow::Result<Vec<String>> {
    use crate::launch;
    let profile = launch.profile_for(sys.id);
    let existing = ["metadata.pegasus.txt", "metadata.txt"]
        .iter()
        .map(|f| ss.path.join(f))
        .find(|p| p.exists());
    if let Some(path) = existing {
        return update_existing_pegasus(sys, ss, &path, profile, launch.fix_existing);
    }
    let mut exts: Vec<&str> = sys.extensions.to_vec();
    exts.sort();
    exts.dedup();
    let mut s = String::new();
    s.push_str("# Created by RomCrate. Edit freely — RomCrate never overwrites this file.\n");
    s.push_str(&format!("collection: {}\n", sys.name));
    s.push_str(&format!("shortname: {}\n", ss.folder_name));
    s.push_str(&format!("extensions: {}\n", exts.join(", ")));
    match profile {
        Some(p) => {
            s.push_str(&launch::render_block(p));
            s.push('\n');
        }
        None => {
            s.push_str("# Add how Pegasus should start these games, for example:\n");
            s.push_str("# launch: retroarch -L \"/path/to/core\" \"{file.path}\"\n\n");
        }
    }
    for g in &ss.games {
        let art = results.iter().find(|r| r.game.path == g.path);
        let info = art.and_then(|r| r.info.as_ref());
        let title = art
            .map(GameArt::title)
            .unwrap_or_else(|| display_title(&g.stem));
        s.push_str(&format!(
            "game: {}\nfile: {}\n",
            one_line(&title),
            one_line(&g.rel_path)
        ));
        if let Some(i) = info {
            for (k, v) in [
                ("developer", &i.developer),
                ("publisher", &i.publisher),
                ("genre", &i.genre),
                ("players", &i.players),
                ("release", &i.release_date),
            ] {
                if let Some(v) = v {
                    s.push_str(&format!("{k}: {}\n", one_line(v)));
                }
            }
            if let Some(r) = i.rating {
                s.push_str(&format!("rating: {}%\n", (r * 100.0).round() as u32));
            }
            if let Some(d) = &i.description {
                // Multi-line values are indented; blank lines become a lone ".".
                s.push_str("description:\n");
                for line in d.lines() {
                    let l = line.trim();
                    s.push_str(&format!("  {}\n", if l.is_empty() { "." } else { l }));
                }
            }
        }
        s.push('\n');
    }
    std::fs::write(ss.path.join("metadata.pegasus.txt"), s)?;
    let note = match profile {
        Some(p) if p.verified => format!(
            "{}: created metadata.pegasus.txt with the tested launch line for {}.",
            ss.system_name, p.emulator
        ),
        Some(p) => format!(
            "{}: created metadata.pegasus.txt. The launch line for {} hasn't been tested on a device yet, so it's commented out — remove the '# ' to try it.",
            ss.system_name, p.emulator
        ),
        None => format!(
            "{}: created metadata.pegasus.txt — add a 'launch:' line so Pegasus can start the games.",
            ss.system_name
        ),
    };
    Ok(vec![note])
}

/// An existing Pegasus file is never changed unless the user opted in
/// (`fix_existing`); then a backup is made first and only the launch block
/// is added or replaced.
fn update_existing_pegasus(
    sys: &System,
    ss: &ScannedSystem,
    path: &Path,
    profile: Option<&'static crate::launch::LaunchProfile>,
    fix_existing: bool,
) -> anyhow::Result<Vec<String>> {
    use crate::launch;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut notes = vec![format!(
        "{}: kept your existing {name}; art is in the media folder.",
        ss.system_name
    )];
    let text = std::fs::read_to_string(path)?;

    // Disc games stored as .chd but the file doesn't list .chd.
    let has_chd = ss
        .games
        .iter()
        .any(|g| g.file_name.to_lowercase().ends_with(".chd"));
    let header: String = text
        .lines()
        .take_while(|l| !l.trim_start().starts_with("game:"))
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("extension") || l.starts_with(' ')
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if has_chd && header.contains("extension") && !header.contains("chd") {
        notes.push(format!(
            "{}: your {name} doesn't list .chd in 'extensions:', so Pegasus won't show your .chd games. Add chd to that line.",
            ss.system_name
        ));
    }

    let Some(p) = profile else { return Ok(notes) };
    let block = launch::render_block(p);
    let bad = launch::find_known_bad(&text, sys.id);
    let missing = !launch::has_launch(&text);
    if !missing && bad.is_none() {
        return Ok(notes);
    }
    let problem = match bad {
        Some(b) => format!("its launch line won't work: {}", b.why),
        None => "it has no launch line, so Pegasus can't start these games".to_string(),
    };
    if fix_existing && p.verified {
        let new_text = match bad {
            Some(_) => Some(launch::replace_launch(&text, &block)),
            None => launch::insert_launch(&text, &block),
        };
        if let Some(new_text) = new_text {
            let backup = launch::backup_path(path);
            std::fs::copy(path, &backup)?;
            crate::imaging::save_bytes(new_text.as_bytes(), path)?;
            notes.push(format!(
                "{}: {problem}. Fixed with the tested launch line for {} (your original is saved as {}).",
                ss.system_name,
                p.emulator,
                backup.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            ));
            return Ok(notes);
        }
    }
    notes.push(format!(
        "{}: {problem}. {}\n{}",
        ss.system_name,
        if p.verified {
            "Add this to the top of the file (or turn on \"Fix launch lines in existing Pegasus files\"):"
        } else {
            "This launch line hasn't been tested yet; try it by adding it to the top of the file:"
        },
        block.trim_end()
    ));
    Ok(notes)
}

fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

// ---------------------------------------------------------------- RetroArch

fn write_retroarch_playlist(
    root: &Path,
    sys: &System,
    ss: &ScannedSystem,
) -> anyhow::Result<Vec<String>> {
    let path = join_safe(root, &["playlists", &format!("{}.lpl", sys.libretro)]);
    if path.exists() {
        return Ok(vec![]);
    }
    let items: Vec<serde_json::Value> = ss
        .games
        .iter()
        .map(|g| {
            serde_json::json!({
                "path": g.path.to_string_lossy(),
                "label": g.stem,
                "core_path": "DETECT",
                "core_name": "DETECT",
                "crc32": "DETECT",
                "db_name": format!("{}.lpl", sys.libretro),
            })
        })
        .collect();
    let pl = serde_json::json!({
        "version": "1.5",
        "default_core_path": "",
        "default_core_name": "",
        "label_display_mode": 0,
        "right_thumbnail_mode": 0,
        "left_thumbnail_mode": 0,
        "sort_mode": 0,
        "items": items,
    });
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&pl)?)?;
    Ok(vec![format!(
        "{}: created RetroArch playlist {}.lpl",
        ss.system_name, sys.libretro
    )])
}

// ---------------------------------------------------------------- EmulationStation gamelist.xml

#[derive(Debug, Clone)]
struct XmlEntry {
    tag: String,
    attrs: Vec<(String, String)>,
    fields: Vec<(String, String)>,
}

impl XmlEntry {
    fn get(&self, k: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
    fn set(&mut self, k: &str, v: String) {
        match self.fields.iter_mut().find(|(n, _)| n == k) {
            Some(f) => f.1 = v,
            None => self.fields.push((k.to_string(), v)),
        }
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn norm_rel(p: &str) -> String {
    p.trim().trim_start_matches("./").replace('\\', "/")
}

fn parse_gamelist(xml: &str) -> anyhow::Result<Vec<XmlEntry>> {
    let doc = roxmltree::Document::parse(xml)?;
    let mut out = Vec::new();
    for node in doc.root_element().children().filter(|n| n.is_element()) {
        let mut e = XmlEntry {
            tag: node.tag_name().name().to_string(),
            attrs: node
                .attributes()
                .map(|a| (a.name().to_string(), a.value().to_string()))
                .collect(),
            fields: Vec::new(),
        };
        for c in node.children().filter(|c| c.is_element()) {
            e.fields.push((
                c.tag_name().name().to_string(),
                c.text().unwrap_or_default().to_string(),
            ));
        }
        out.push(e);
    }
    Ok(out)
}

fn render_gamelist(entries: &[XmlEntry]) -> String {
    let mut s = String::from("<?xml version=\"1.0\"?>\n<gameList>\n");
    for e in entries {
        s.push_str(&format!("\t<{}", e.tag));
        for (k, v) in &e.attrs {
            s.push_str(&format!(" {}=\"{}\"", k, xml_escape(v)));
        }
        s.push_str(">\n");
        for (k, v) in &e.fields {
            s.push_str(&format!("\t\t<{k}>{}</{k}>\n", xml_escape(v)));
        }
        s.push_str(&format!("\t</{}>\n", e.tag));
    }
    s.push_str("</gameList>\n");
    s
}

/// Create or update a gamelist.xml. `images` controls whether art paths are
/// written (EmulationStation) or not (ES-DE finds media by file name).
/// Game info only fills empty fields unless `overwrite` is on, so hand edits survive.
fn write_gamelist(
    path: &Path,
    rom_dir: &Path,
    ss: &ScannedSystem,
    results: &[GameArt],
    images: bool,
    overwrite: bool,
) -> anyhow::Result<Vec<String>> {
    let useful = |r: &GameArt| (images && !r.files.is_empty()) || r.info.is_some();
    if !results.iter().any(useful) {
        return Ok(vec![]);
    }
    let mut notes = Vec::new();
    let mut entries = if path.exists() {
        let text = std::fs::read_to_string(path)?;
        let backup = path.with_file_name("gamelist.xml.bak");
        if !backup.exists() {
            std::fs::copy(path, &backup)?;
            notes.push(format!(
                "{}: original gamelist.xml backed up to gamelist.xml.bak",
                ss.system_name
            ));
        }
        parse_gamelist(&text).map_err(|e| {
            anyhow::anyhow!("couldn't read existing gamelist.xml ({e}); left it untouched")
        })?
    } else {
        Vec::new()
    };

    for r in results {
        if !useful(r) {
            continue;
        }
        let rel = norm_rel(&r.game.rel_path);
        let idx = entries
            .iter()
            .position(|e| e.get("path").map(norm_rel).as_deref() == Some(rel.as_str()));
        let idx = match idx {
            Some(i) => i,
            None => {
                entries.push(XmlEntry {
                    tag: "game".into(),
                    attrs: vec![],
                    fields: vec![
                        ("path".into(), format!("./{rel}")),
                        ("name".into(), r.title()),
                    ],
                });
                entries.len() - 1
            }
        };
        let e = &mut entries[idx];
        if let Some(i) = &r.info {
            let date = i.release_date.as_deref().and_then(es_date);
            let rating = i.rating.map(|r| format!("{:.2}", r));
            for (field, value) in [
                ("desc", &i.description),
                ("releasedate", &date),
                ("developer", &i.developer),
                ("publisher", &i.publisher),
                ("genre", &i.genre),
                ("players", &i.players),
                ("rating", &rating),
            ] {
                if let Some(v) = value {
                    if overwrite || e.get(field).map(|x| x.trim().is_empty()).unwrap_or(true) {
                        e.set(field, v.clone());
                    }
                }
            }
        }
        if !images {
            continue;
        }
        let relp = |p: &PathBuf| -> String {
            p.strip_prefix(rom_dir)
                .map(|x| format!("./{}", x.to_string_lossy().replace('\\', "/")))
                .unwrap_or_else(|_| p.to_string_lossy().to_string())
        };
        // Main image: mix > screenshot > 3D box > box > title.
        if let Some(p) = [Mix, Snap, Box3d, Boxart, Title]
            .iter()
            .find_map(|k| r.files.get(k))
        {
            e.set("image", relp(p));
        }
        if let Some(p) = r.files.get(&Box3d).or_else(|| r.files.get(&Boxart)) {
            e.set("thumbnail", relp(p));
        }
        for (k, field) in [
            (Logo, "marquee"),
            (Title, "titleshot"),
            (Boxback, "boxback"),
            (Cartridge, "cartridge"),
            (Fanart, "fanart"),
            (Video, "video"),
            (Manual, "manual"),
            (Boxart, "boxart"),
            (Logo, "wheel"),
        ] {
            if let Some(p) = r.files.get(&k) {
                e.set(field, relp(p));
            }
        }
    }
    let tmp = path.with_file_name("gamelist.xml.part");
    std::fs::write(&tmp, render_gamelist(&entries))?;
    std::fs::rename(&tmp, path)?;
    notes.push(format!("{}: {} updated", ss.system_name, path.display()));
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_system_folder;
    use crate::systems::by_id;
    use std::fs;

    #[test]
    fn gamelist_merge_preserves_existing() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("snes");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Chrono Trigger (USA).sfc"), b"x").unwrap();
        fs::write(dir.join("Other & Co (USA).sfc"), b"x").unwrap();
        fs::write(
            dir.join("gamelist.xml"),
            "<?xml version=\"1.0\"?><gameList><game id=\"5\"><path>./Chrono Trigger (USA).sfc</path><name>Chrono Trigger</name><rating>0.9</rating><playcount>12</playcount></game></gameList>",
        )
        .unwrap();
        let sys = by_id("snes").unwrap();
        let ss = scan_system_folder(sys, &dir);
        let results: Vec<GameArt> = ss
            .games
            .iter()
            .map(|g| {
                let mut files = BTreeMap::new();
                for k in [Boxart, Mix] {
                    files.insert(
                        k,
                        art_targets(
                            LauncherKind::EmulationStation,
                            None,
                            sys,
                            &ss,
                            g,
                            k,
                            None,
                            &[Boxart, Mix],
                        )[0]
                        .clone(),
                    );
                }
                GameArt {
                    game: g.clone(),
                    files,
                    info: None,
                    matched: None,
                }
            })
            .collect();
        finalize_system(
            LauncherKind::EmulationStation,
            None,
            sys,
            &ss,
            &results,
            false,
        )
        .unwrap();
        let xml = fs::read_to_string(dir.join("gamelist.xml")).unwrap();
        assert!(dir.join("gamelist.xml.bak").exists());
        assert!(xml.contains("<playcount>12</playcount>"));
        assert!(xml.contains("id=\"5\""));
        assert!(xml.contains("<image>./images/Chrono Trigger (USA)-mix.png</image>"));
        assert!(xml.contains("<thumbnail>./images/Chrono Trigger (USA)-thumb.png</thumbnail>"));
        assert!(xml.contains("<name>Other &amp; Co</name>"));
        roxmltree::Document::parse(&xml).unwrap();
    }

    #[test]
    fn target_paths() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("psx");
        fs::create_dir_all(dir.join("Europe")).unwrap();
        fs::write(dir.join("Europe/Tom & Jerry - Fists (Europe).chd"), b"x").unwrap();
        let sys = by_id("psx").unwrap();
        let ss = scan_system_folder(sys, &dir);
        let g = &ss.games[0];
        let media = t.path().join("media");
        let p = &art_targets(
            LauncherKind::EsDe,
            Some(&media),
            sys,
            &ss,
            g,
            Boxart,
            None,
            &[Boxart],
        )[0];
        assert!(p.starts_with(media.join("psx/covers/Europe")));
        let p = &art_targets(
            LauncherKind::RetroArch,
            Some(&media),
            sys,
            &ss,
            g,
            Snap,
            Some("Tom _ Jerry (Europe)"),
            &[Snap],
        );
        assert_eq!(p.len(), 2);
        assert!(p[0].ends_with(
            "thumbnails/Sony - PlayStation/Named_Snaps/Tom _ Jerry - Fists (Europe).png"
        ));
        let p = &art_targets(
            LauncherKind::Pegasus,
            None,
            sys,
            &ss,
            g,
            Logo,
            None,
            &[Logo],
        )[0];
        assert!(p.ends_with("logo.png"));
        assert!(
            art_targets(LauncherKind::Pegasus, None, sys, &ss, g, Mix, None, &[Mix]).is_empty()
        );
        // 3D box takes the Pegasus boxFront slot when both are chosen.
        assert!(art_targets(
            LauncherKind::Pegasus,
            None,
            sys,
            &ss,
            g,
            Boxart,
            None,
            &[Boxart, Box3d]
        )
        .is_empty());
        assert!(art_targets(
            LauncherKind::Pegasus,
            None,
            sys,
            &ss,
            g,
            Box3d,
            None,
            &[Boxart, Box3d]
        )[0]
        .ends_with("boxFront.png"));
        let v = &art_targets(
            LauncherKind::EsDe,
            Some(&media),
            sys,
            &ss,
            g,
            Video,
            None,
            &[Video],
        )[0];
        assert!(v.ends_with("videos/Europe/Tom & Jerry - Fists (Europe).mp4"));
    }
}
