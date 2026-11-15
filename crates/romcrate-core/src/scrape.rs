//! The scrape job: identify every game, fetch art from the best source,
//! validate, resize, save, and update launcher metadata.
//!
//! Source order for each piece of art:
//! 1. libretro-thumbnails (free, unlimited) for box art, screenshots, titles, logos
//! 2. ScreenScraper guest mode (optional) for everything else and for gaps
//! 3. RomCrate itself: mix images and 3D boxes rendered from the 2D cover

use crate::identify::{self, Fingerprint, GameDb, Identifier};
use crate::imaging::{self, Size, Upscale};
use crate::launchbox::{GameInfo, LaunchBox, PlatformDb};
use crate::launchers::{self, art_targets, GameArt, LauncherKind};
use crate::matcher::{MatchMethod, MatchPrefs};
use crate::net::{MAX_IMAGE_BYTES, MAX_MANUAL_BYTES, MAX_VIDEO_BYTES};
use crate::scan::{Game, Library, ScannedSystem};
use crate::screenscraper::{self, ScreenScraper, SsGame};
use crate::source::{ArtKind, Libretro, SystemArt};
use crate::systems::{self, System};
use futures::stream::{self, StreamExt};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::OnceCell;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrapeConfig {
    pub launcher: LauncherKind,
    /// Media / RetroArch / LaunchBox / output folder, when the launcher needs one.
    pub folder: Option<PathBuf>,
    pub kinds: Vec<ArtKind>,
    /// Size multiplier from the chosen preset (1.0 = Standard).
    pub size_scale: f32,
    pub upscale: Upscale,
    /// Path to realesrgan-ncnn-vulkan, only used with `Upscale::Ai`.
    pub ai_binary: Option<PathBuf>,
    /// Replace art that already exists.
    pub overwrite: bool,
    pub prefs: MatchPrefs,
    /// Parallel games (clamped to 1..=8 to be polite to free servers).
    pub concurrency: usize,
    /// Re-download artwork lists even if the cache is fresh.
    pub refresh_index: bool,
    /// Write gamelist.xml / Pegasus metadata / RetroArch playlists.
    pub write_metadata: bool,
    /// Identify games by file hash / disc serial (No-Intro & Redump).
    pub identify_by_contents: bool,
    /// Use ScreenScraper guest mode for extra media.
    pub use_screenscraper: bool,
    /// Optional ScreenScraper login (never saved to disk).
    #[serde(skip_serializing)]
    pub ss_user: Option<(String, String)>,
    /// Render 3D boxes locally when no 3D box is available online.
    pub render_3d_fallback: bool,
    /// Pegasus only: which media folder layout to write.
    pub pegasus_layout: launchers::PegasusLayout,
    /// Use the free LaunchBox Games Database (extra art types + game info).
    pub use_launchbox: bool,
    /// Write descriptions, genre, release date… into launcher files.
    pub game_info: bool,
    /// Pegasus launch lines (which emulator starts each system).
    #[serde(default)]
    pub launch: crate::launch::LaunchChoice,
}

impl Default for ScrapeConfig {
    fn default() -> Self {
        Self {
            launcher: LauncherKind::EsDe,
            folder: None,
            kinds: vec![ArtKind::Boxart, ArtKind::Snap, ArtKind::Logo],
            size_scale: 1.0,
            upscale: Upscale::Smart,
            ai_binary: None,
            overwrite: false,
            prefs: MatchPrefs::default(),
            concurrency: 4,
            refresh_index: false,
            write_metadata: true,
            identify_by_contents: true,
            use_screenscraper: true,
            ss_user: None,
            render_3d_fallback: true,
            pegasus_layout: launchers::PegasusLayout::Auto,
            use_launchbox: true,
            game_info: true,
            launch: crate::launch::LaunchChoice::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameStatus {
    /// New art saved.
    Done,
    /// Some requested types were not available.
    Partial,
    /// Everything already existed; nothing to do.
    Skipped,
    /// No artwork found.
    Missing,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    #[serde(rename_all = "camelCase")]
    Started {
        total_games: usize,
        systems: usize,
        screenscraper: bool,
    },
    #[serde(rename_all = "camelCase")]
    SystemStarted {
        system_id: String,
        system_name: String,
        games: usize,
    },
    #[serde(rename_all = "camelCase")]
    SystemIndex {
        system_id: String,
        artwork: usize,
        database: usize,
    },
    #[serde(rename_all = "camelCase")]
    GameDone {
        system_id: String,
        game: String,
        status: GameStatus,
        matched: Option<String>,
        /// How the game was recognised: hash, exact, title, fuzzy, screenscraper.
        method: Option<String>,
        saved: Vec<ArtKind>,
        missing: Vec<ArtKind>,
        preview: Option<PathBuf>,
        message: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Log { level: String, message: String },
    #[serde(rename_all = "camelCase")]
    Finished { summary: Summary },
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub games: usize,
    pub done: usize,
    pub partial: usize,
    pub skipped: usize,
    pub missing: usize,
    pub failed: usize,
    pub files_saved: usize,
    pub cancelled: bool,
    pub missing_games: Vec<String>,
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

fn log(emit: &Emit, level: &str, msg: impl Into<String>) {
    emit(Event::Log {
        level: level.into(),
        message: msg.into(),
    });
}

/// Everything a scrape needs to reach the outside world.
pub struct Sources {
    pub libretro: Libretro,
    pub identifier: Identifier,
    pub screenscraper: Option<ScreenScraper>,
    pub launchbox: Option<LaunchBox>,
}

/// Run a scrape over the selected systems of a scanned library.
pub async fn run(
    lib: Library,
    selected: Vec<String>,
    cfg: ScrapeConfig,
    sources: Arc<Sources>,
    cancel: Arc<AtomicBool>,
    emit: Emit,
) -> Summary {
    let systems: Vec<ScannedSystem> = lib
        .systems
        .into_iter()
        .filter(|s| {
            selected.is_empty()
                || selected.contains(&s.system_id)
                || selected.contains(&s.path.to_string_lossy().to_string())
        })
        .collect();
    let total: usize = systems.iter().map(|s| s.games.len()).sum();
    let ss_on = cfg.use_screenscraper && sources.screenscraper.is_some();
    emit(Event::Started {
        total_games: total,
        systems: systems.len(),
        screenscraper: ss_on,
    });

    let mut summary = Summary {
        games: total,
        ..Default::default()
    };
    let finish = |mut summary: Summary, emit: &Emit| {
        summary.cancelled = cancel.load(Ordering::Relaxed);
        emit(Event::Finished {
            summary: summary.clone(),
        });
        summary
    };
    let launcher_info = launchers::launchers()
        .into_iter()
        .find(|l| l.id == cfg.launcher);
    if let Some(info) = &launcher_info {
        if info.needs_folder && cfg.folder.is_none() {
            log(
                &emit,
                "error",
                format!("Please choose the {} first.", info.folder_label),
            );
            summary.failed = total;
            return finish(summary, &emit);
        }
    }
    let supported: Vec<ArtKind> = launcher_info.map(|l| l.supported).unwrap_or_default();
    let kinds: Vec<ArtKind> = cfg
        .kinds
        .iter()
        .copied()
        .filter(|k| supported.contains(k))
        .collect();
    if kinds.is_empty() {
        log(&emit, "error", "No artwork types selected.");
        return finish(summary, &emit);
    }
    if !ss_on
        && kinds
            .iter()
            .any(|k| matches!(k, ArtKind::Video | ArtKind::Manual))
    {
        log(&emit, "warn", "Videos and manuals need the ScreenScraper extras source, which isn't available in this build — skipping those.");
    }
    let cfg = Arc::new(ScrapeConfig {
        kinds,
        concurrency: cfg.concurrency.clamp(1, 8),
        ..cfg
    });

    // LaunchBox Games Database: download (at most weekly) and index the
    // platforms we need, once, before going through the systems.
    let mut lb_dbs: HashMap<String, Arc<PlatformDb>> = HashMap::new();
    if let (true, Some(lb)) = (cfg.use_launchbox, sources.launchbox.as_ref()) {
        let emit2 = emit.clone();
        let last = std::sync::Mutex::new(0u64);
        let progress = move |done: u64, total: Option<u64>| {
            let mb = done / (1024 * 1024);
            let mut l = last.lock().unwrap();
            if mb >= *l + 10 {
                *l = mb;
                let of = total
                    .map(|t| format!(" of {} MB", t / (1024 * 1024)))
                    .unwrap_or_default();
                log(
                    &emit2,
                    "info",
                    format!("Downloading the free LaunchBox games database (weekly)… {mb} MB{of}"),
                );
            }
        };
        match lb.ensure_database(cfg.refresh_index, &progress).await {
            Ok(_) => {
                let names: Vec<String> = systems
                    .iter()
                    .filter_map(|s| systems::by_id(&s.system_id).map(|x| x.launchbox.to_string()))
                    .collect();
                let lb_dir = lb.dir_for_blocking();
                let parsed = tokio::task::spawn_blocking(move || {
                    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                    lb_dir.platforms(&refs)
                })
                .await;
                match parsed {
                    Ok(Ok(map)) => {
                        for (k, mut v) in map {
                            v.prepare();
                            lb_dbs.insert(k, Arc::new(v));
                        }
                    }
                    Ok(Err(e)) => log(
                        &emit,
                        "warn",
                        format!(
                            "LaunchBox database couldn't be read ({e}); continuing without it."
                        ),
                    ),
                    Err(e) => log(&emit, "warn", format!("LaunchBox database: {e}")),
                }
            }
            Err(e) => log(
                &emit,
                "warn",
                format!("LaunchBox database unavailable ({e}); continuing without it."),
            ),
        }
    }

    for ss in systems {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Some(sys) = systems::by_id(&ss.system_id) else {
            continue;
        };
        emit(Event::SystemStarted {
            system_id: ss.system_id.clone(),
            system_name: ss.system_name.clone(),
            games: ss.games.len(),
        });

        let art = match sources.libretro.system_art(sys, cfg.refresh_index).await {
            Ok(a) => Arc::new(a),
            Err(e) => {
                if ss_on {
                    log(
                        &emit,
                        "warn",
                        format!(
                            "{}: free artwork list unavailable ({e}); using ScreenScraper only.",
                            ss.system_name
                        ),
                    );
                    Arc::new(SystemArt::from_names(BTreeMap::new()))
                } else {
                    log(&emit, "error", format!("{}: couldn't load the artwork list ({e}). Check your internet connection.", ss.system_name));
                    summary.failed += ss.games.len();
                    continue;
                }
            }
        };
        let db = if cfg.identify_by_contents {
            match sources.identifier.load(sys).await {
                Ok(db) => Arc::new(db),
                Err(e) => {
                    log(
                        &emit,
                        "warn",
                        format!(
                            "{}: game database unavailable ({e}); matching by file name.",
                            ss.system_name
                        ),
                    );
                    Arc::new(GameDb::default())
                }
            }
        } else {
            Arc::new(GameDb::default())
        };
        emit(Event::SystemIndex {
            system_id: ss.system_id.clone(),
            artwork: art.total(),
            database: db.len(),
        });
        if art.total() == 0 && !ss_on {
            log(
                &emit,
                "warn",
                format!(
                    "{}: the free collection has no artwork for this system yet.",
                    ss.system_name
                ),
            );
        }

        let launcher = launchers::effective_launcher(cfg.launcher, cfg.pegasus_layout, &ss);
        let ctx = Arc::new(Ctx {
            lb: lb_dbs.get(sys.launchbox).cloned(),
            launcher,
            sys,
            ss: ss.clone(),
            art,
            db,
            cfg: cfg.clone(),
            sources: sources.clone(),
            emit: emit.clone(),
            ss_on,
        });
        let results: Vec<(GameStatus, Option<GameArt>, usize, String)> =
            stream::iter(ss.games.clone())
                .map(|game| {
                    let (ctx, cancel) = (ctx.clone(), cancel.clone());
                    async move {
                        if cancel.load(Ordering::Relaxed) {
                            return (GameStatus::Skipped, None, 0, game.stem.clone());
                        }
                        let (status, ga, saved) = ctx.scrape_game(&game).await;
                        (status, ga, saved, game.stem.clone())
                    }
                })
                .buffer_unordered(cfg.concurrency)
                .collect()
                .await;

        let mut arts = Vec::new();
        for (status, ga, saved, stem) in results {
            summary.files_saved += saved;
            match status {
                GameStatus::Done => summary.done += 1,
                GameStatus::Partial => summary.partial += 1,
                GameStatus::Skipped => summary.skipped += 1,
                GameStatus::Missing => {
                    summary.missing += 1;
                    summary
                        .missing_games
                        .push(format!("{}: {}", ss.system_name, stem));
                }
                GameStatus::Failed => summary.failed += 1,
            }
            if let Some(ga) = ga {
                arts.push(ga);
            }
        }
        if cfg.write_metadata {
            match launchers::finalize_system_with(
                launcher,
                cfg.folder.as_deref(),
                sys,
                &ss,
                &arts,
                cfg.overwrite,
                &cfg.launch,
            ) {
                Ok(notes) => notes.into_iter().for_each(|n| log(&emit, "info", n)),
                Err(e) => log(&emit, "warn", format!("{}: {e}", ss.system_name)),
            }
        }
    }
    if let Some(why) = sources
        .screenscraper
        .as_ref()
        .and_then(|s| s.stopped_reason())
    {
        log(&emit, "warn", why);
    }
    finish(summary, &emit)
}

struct Ctx {
    /// LaunchBox Games Database entries for this platform, if loaded.
    lb: Option<Arc<PlatformDb>>,
    /// Launcher layout for this system (Pegasus may resolve to the Skraper layout).
    launcher: LauncherKind,
    sys: &'static System,
    ss: ScannedSystem,
    art: Arc<SystemArt>,
    db: Arc<GameDb>,
    cfg: Arc<ScrapeConfig>,
    sources: Arc<Sources>,
    emit: Emit,
    ss_on: bool,
}

/// What we fetched for one kind before rendering.
enum Fetched {
    Image(DynamicImage),
    Bytes(Vec<u8>),
}

impl Ctx {
    async fn scrape_game(&self, game: &Game) -> (GameStatus, Option<GameArt>, usize) {
        let cfg = &self.cfg;
        let ss = &self.ss;
        let send = |status: GameStatus,
                    matched: Option<String>,
                    method: Option<String>,
                    saved: Vec<ArtKind>,
                    missing: Vec<ArtKind>,
                    preview: Option<PathBuf>,
                    message: Option<String>| {
            (self.emit)(Event::GameDone {
                system_id: ss.system_id.clone(),
                game: game.stem.clone(),
                status,
                matched,
                method,
                saved,
                missing,
                preview,
                message,
            });
        };

        // 1. Identify by contents.
        let fp: Fingerprint = if cfg.identify_by_contents || self.ss_on {
            let (sys, g) = (self.sys, game.clone());
            tokio::task::spawn_blocking(move || identify::fingerprint(sys, &g))
                .await
                .unwrap_or_default()
        } else {
            Fingerprint::default()
        };
        let canonical: Option<String> = identify::lookup(&self.db, &fp).map(String::from);

        // 2. Match against the free collection.
        let (best, mut method) = match &canonical {
            Some(c) if self.art.all.contains(c) => (Some(c.clone()), Some("hash".to_string())),
            _ => {
                let m = canonical
                    .as_deref()
                    .and_then(|c| self.art.all.best_match(c, &cfg.prefs))
                    .or_else(|| self.art.all.best_match(&game.stem, &cfg.prefs));
                let method = m
                    .as_ref()
                    .map(|m| match (m.method.clone(), canonical.is_some()) {
                        (_, true) => "hash".to_string(),
                        (MatchMethod::Exact, _) => "exact".into(),
                        (MatchMethod::Title, _) => "title".into(),
                        (MatchMethod::Fuzzy, _) => "fuzzy".into(),
                    });
                match m {
                    Some(m) => (Some(m.name), method),
                    // Last resort: the title stored inside the ROM itself.
                    None => match fp
                        .internal_title
                        .as_deref()
                        .and_then(|t| self.art.all.best_match(t, &cfg.prefs))
                    {
                        Some(m) => (Some(m.name), Some("header".to_string())),
                        None => (None, None),
                    },
                }
            }
        };

        // 2b. LaunchBox Games Database: extra art types and game info.
        let lb_id: Option<u64> = self.lb.as_ref().and_then(|db| {
            let mut names: Vec<&str> = Vec::new();
            if let Some(c) = canonical.as_deref() {
                names.push(c);
            }
            if let Some(b) = best.as_deref() {
                names.push(b);
            }
            names.push(&game.stem);
            if let Some(t) = fp.internal_title.as_deref() {
                names.push(t);
            }
            db.find(&names, &cfg.prefs)
        });
        let info: Option<GameInfo> = if cfg.game_info {
            lb_id.and_then(|id| self.lb.as_ref().and_then(|db| db.info(id)))
        } else {
            None
        };
        if best.is_none() && lb_id.is_some() {
            method = Some("launchbox".into());
        }

        // 3. Plan output files, skipping ones that already exist.
        let mut plan: Vec<(ArtKind, Vec<PathBuf>)> = Vec::new();
        let mut existing: BTreeMap<ArtKind, PathBuf> = BTreeMap::new();
        for &k in &cfg.kinds {
            if !self.ss_on && matches!(k, ArtKind::Video | ArtKind::Manual) {
                continue;
            }
            let targets = art_targets(
                self.launcher,
                cfg.folder.as_deref(),
                self.sys,
                ss,
                game,
                k,
                best.as_deref(),
                &cfg.kinds,
            );
            if targets.is_empty() {
                continue;
            }
            if !cfg.overwrite && targets[0].exists() {
                existing.insert(k, targets[0].clone());
            } else {
                plan.push((k, targets));
            }
        }
        if plan.is_empty() {
            let matched = best.clone().or(canonical.clone());
            send(
                GameStatus::Skipped,
                best.or(canonical)
                    .or_else(|| info.as_ref().map(|i| i.title.clone())),
                method,
                vec![],
                vec![],
                preview_of(&existing),
                Some("Already has artwork".into()),
            );
            return (
                GameStatus::Skipped,
                (!existing.is_empty() || info.is_some()).then(|| GameArt {
                    game: game.clone(),
                    files: existing,
                    info,
                    matched,
                }),
                0,
            );
        }

        // 4. Gather sources.
        let planned: Vec<ArtKind> = plan.iter().map(|p| p.0).collect();
        let mut want: Vec<ArtKind> = planned
            .iter()
            .copied()
            .filter(|k| *k != ArtKind::Mix)
            .collect();
        if planned.contains(&ArtKind::Mix) {
            want.extend([ArtKind::Snap, ArtKind::Boxart, ArtKind::Logo]);
        }
        if planned.contains(&ArtKind::Box3d) && cfg.render_3d_fallback {
            want.push(ArtKind::Boxart);
        }
        want.sort();
        want.dedup();

        let ss_game: OnceCell<Option<SsGame>> = OnceCell::new();
        let mut fetched: HashMap<ArtKind, Fetched> = HashMap::new();
        let mut errors: Vec<String> = Vec::new();
        for k in want {
            // Free collection first.
            if let Some(img) = self
                .fetch_libretro(k, best.as_deref(), game, &mut errors)
                .await
            {
                fetched.insert(k, Fetched::Image(img));
                continue;
            }
            // Then the LaunchBox Games Database (free, no account).
            if let (Some(db), Some(id), Some(lb)) =
                (self.lb.as_ref(), lb_id, self.sources.launchbox.as_ref())
            {
                if let Some(url) = db.image_url(id, k, &cfg.prefs.regions) {
                    match lb.fetch_image(&url).await {
                        Ok(bytes) => match tokio::task::spawn_blocking(move || {
                            imaging::decode_validated(&bytes)
                        })
                        .await
                        {
                            Ok(Ok(img)) => {
                                fetched.insert(k, Fetched::Image(img));
                                continue;
                            }
                            Ok(Err(e)) => {
                                errors.push(format!("{}: rejected download ({e})", k.label()))
                            }
                            Err(e) => errors.push(e.to_string()),
                        },
                        Err(e) => errors.push(format!("{}: {e}", k.label())),
                    }
                }
            }
            // Then ScreenScraper.
            if let Some(ssc) = self.sources.screenscraper.as_ref().filter(|_| self.ss_on) {
                let info = ss_game
                    .get_or_init(|| async {
                        match ssc.game(self.sys.id, game, &fp, canonical.as_deref()).await {
                            Ok(g) => g,
                            Err(e) => {
                                log(&self.emit, "debug", format!("{}: {e}", game.stem));
                                None
                            }
                        }
                    })
                    .await;
                if let Some(info) = info {
                    if let Some(media) = info.media_for(k, &cfg.prefs.regions) {
                        let max = match k {
                            ArtKind::Video => MAX_VIDEO_BYTES,
                            ArtKind::Manual => MAX_MANUAL_BYTES,
                            _ => MAX_IMAGE_BYTES,
                        };
                        match ssc.download(media, max).await {
                            Ok(bytes) if k.is_image() => {
                                match tokio::task::spawn_blocking(move || {
                                    imaging::decode_validated(&bytes)
                                })
                                .await
                                {
                                    Ok(Ok(img)) => {
                                        fetched.insert(k, Fetched::Image(img));
                                    }
                                    Ok(Err(e)) => errors
                                        .push(format!("{}: rejected download ({e})", k.label())),
                                    Err(e) => errors.push(e.to_string()),
                                }
                            }
                            Ok(bytes) => match screenscraper::validate_media(k, &bytes) {
                                Ok(()) => {
                                    fetched.insert(k, Fetched::Bytes(bytes));
                                }
                                Err(e) => {
                                    errors.push(format!("{}: rejected download ({e})", k.label()))
                                }
                            },
                            Err(e) => errors.push(format!("{}: {e}", k.label())),
                        }
                    }
                }
            }
        }
        if best.is_none() {
            if let Some(Some(info)) = ss_game.get() {
                method = Some("screenscraper".into());
                let _ = &info.name;
            }
        }
        let matched_id = best.clone().or(canonical.clone());
        let matched_label = best
            .clone()
            .or(canonical.clone())
            .or_else(|| info.as_ref().map(|i| i.title.clone()))
            .or_else(|| ss_game.get().cloned().flatten().and_then(|g| g.name));

        // 5. Produce and save each planned kind.
        let mut saved = Vec::new();
        let mut missing = Vec::new();
        let mut files = existing.clone();
        for (k, targets) in plan {
            let size = launchers::base_size(k).scaled(cfg.size_scale);
            let output: Option<Output> = match k {
                ArtKind::Mix => {
                    let img = |k| match fetched.get(&k) {
                        Some(Fetched::Image(i)) => Some(i.clone()),
                        _ => None,
                    };
                    let (s, b, l) = (img(ArtKind::Snap), img(ArtKind::Boxart), img(ArtKind::Logo));
                    tokio::task::spawn_blocking(move || {
                        imaging::compose_mix(s.as_ref(), b.as_ref(), l.as_ref(), size)
                    })
                    .await
                    .ok()
                    .flatten()
                    .map(Output::Image)
                }
                ArtKind::Box3d => {
                    match (fetched.get(&ArtKind::Box3d), fetched.get(&ArtKind::Boxart)) {
                        (Some(Fetched::Image(i)), _) => {
                            Some(Output::Image(self.render(i, k, size).await))
                        }
                        (_, Some(Fetched::Image(front))) if cfg.render_3d_fallback => {
                            let f = front.clone();
                            tokio::task::spawn_blocking(move || imaging::render_box3d(&f, size))
                                .await
                                .ok()
                                .map(Output::Image)
                        }
                        _ => None,
                    }
                }
                _ => match fetched.get(&k) {
                    Some(Fetched::Image(i)) => Some(Output::Image(self.render(i, k, size).await)),
                    Some(Fetched::Bytes(b)) => Some(Output::Bytes(b.clone())),
                    None => None,
                },
            };
            let Some(output) = output else {
                missing.push(k);
                continue;
            };
            let output = Arc::new(output);
            let mut ok = false;
            for t in &targets {
                let (o, t2) = (output.clone(), t.clone());
                let res = tokio::task::spawn_blocking(move || match &*o {
                    Output::Image(img) => imaging::save_png(img, &t2),
                    Output::Bytes(b) => imaging::save_bytes(b, &t2),
                })
                .await;
                match res {
                    Ok(Ok(())) => ok = true,
                    Ok(Err(e)) => errors.push(format!("couldn't save {}: {e}", t.display())),
                    Err(e) => errors.push(e.to_string()),
                }
            }
            if ok {
                saved.push(k);
                files.insert(k, targets[0].clone());
            }
        }

        let n = saved.len();
        let status = if n == 0 && matched_label.is_none() {
            GameStatus::Missing
        } else if n == 0 && !errors.is_empty() {
            GameStatus::Failed
        } else if n == 0 {
            GameStatus::Missing
        } else if missing.is_empty() {
            GameStatus::Done
        } else {
            GameStatus::Partial
        };
        if !game.missing_files.is_empty() {
            errors.insert(
                0,
                format!(
                    "Disc data missing: {} (the {} file points to it — the download may be incomplete)",
                    game.missing_files.join(", "),
                    game.path.extension().and_then(|e| e.to_str()).unwrap_or("cue")
                ),
            );
        }
        let msg = if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        };
        send(
            status,
            matched_label,
            method,
            saved,
            missing,
            preview_of(&files),
            msg,
        );
        (
            status,
            (!files.is_empty() || info.is_some()).then(|| GameArt {
                game: game.clone(),
                files,
                info,
                matched: matched_id,
            }),
            n,
        )
    }

    async fn fetch_libretro(
        &self,
        k: ArtKind,
        best: Option<&str>,
        game: &Game,
        errors: &mut Vec<String>,
    ) -> Option<DynamicImage> {
        let idx = self.art.per_kind.get(&k)?;
        let best = best?;
        // The game is already identified; for the other art types only accept
        // the same title (any region), never a fuzzy guess — otherwise
        // "Crash Bandicoot" could get the logo of "Crash Bandicoot - Warped".
        let name = if idx.contains(best) {
            best.to_string()
        } else {
            let mut strict = self.cfg.prefs.clone();
            strict.fuzzy = false;
            idx.best_match(best, &strict)
                .or_else(|| idx.best_match(&game.stem, &strict))?
                .name
        };
        match self.sources.libretro.fetch_art(self.sys, k, &name).await {
            Ok(bytes) => {
                match tokio::task::spawn_blocking(move || imaging::decode_validated(&bytes)).await {
                    Ok(Ok(img)) => Some(img),
                    Ok(Err(e)) => {
                        errors.push(format!("{}: rejected download ({e})", k.label()));
                        None
                    }
                    Err(_) => None,
                }
            }
            Err(e) => {
                errors.push(format!("{}: {e}", k.label()));
                None
            }
        }
    }

    async fn render(&self, src: &DynamicImage, k: ArtKind, size: Size) -> image::RgbaImage {
        let cfg = &self.cfg;
        // Real-ESRGAN for photographic art that needs enlarging, if installed.
        if cfg.upscale == Upscale::Ai && !k.is_pixel_art() {
            if let Some(bin) = &cfg.ai_binary {
                if src.width() < size.w && src.height() < size.h {
                    if let Ok(up) = imaging::ai_upscale(bin, src).await {
                        return tokio::task::spawn_blocking(move || {
                            imaging::fit(&up, size, Upscale::Smart, false)
                        })
                        .await
                        .unwrap_or_default();
                    }
                }
            }
        }
        let (src, mode, pixel) = (src.clone(), cfg.upscale, k.is_pixel_art());
        tokio::task::spawn_blocking(move || imaging::fit(&src, size, mode, pixel))
            .await
            .unwrap_or_default()
    }
}

enum Output {
    Image(image::RgbaImage),
    Bytes(Vec<u8>),
}

fn preview_of(files: &BTreeMap<ArtKind, PathBuf>) -> Option<PathBuf> {
    [
        ArtKind::Box3d,
        ArtKind::Boxart,
        ArtKind::Mix,
        ArtKind::Snap,
        ArtKind::Title,
        ArtKind::Logo,
        ArtKind::Fanart,
        ArtKind::Cartridge,
    ]
    .iter()
    .find_map(|k| files.get(k).cloned())
}
