// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine;
use romcrate_core::identify::Identifier;
use romcrate_core::import::{self, ImportOptions, ImportPlan, Mode};
use romcrate_core::launchers::{self, LauncherInfo, LauncherKind};
use romcrate_core::net::SafeClient;
use romcrate_core::scan::{self, Library};
use romcrate_core::screenscraper::{self, ScreenScraper};
use romcrate_core::source::default_cache_dir;
use romcrate_core::systems::{self, System};
use romcrate_core::{ArtKind, Event, Libretro, ScrapeConfig, Sources};
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
struct AppState {
    library: Mutex<Library>,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
    running: Arc<AtomicBool>,
    /// Files RomCrate produced or found this session; only these can be previewed.
    previewable: Arc<Mutex<HashSet<PathBuf>>>,
    /// Folders the user chose this session; only these can be opened in the file manager.
    openable: Mutex<HashSet<PathBuf>>,
    /// The last import preview. Importing only ever acts on this plan, so the
    /// interface can't ask for arbitrary files to be copied or moved.
    import_plan: Mutex<Option<ImportPlan>>,
    /// The last "send to handheld" plan (serial + files). Sending only acts on this.
    send_plan: Mutex<Option<(String, romcrate_core::adb::SendPlan)>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KindInfo {
    id: ArtKind,
    label: &'static str,
    /// "free" (libretro), "extras" (ScreenScraper), "generated" (made by RomCrate), "free+generated"
    source: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    version: &'static str,
    launchers: Vec<LauncherInfo>,
    systems: Vec<System>,
    kinds: Vec<KindInfo>,
    size_presets: Vec<(&'static str, &'static str, f32)>,
    screenscraper_available: bool,
    cache_dir: String,
    launch_profiles: Vec<romcrate_core::launch::LaunchProfile>,
    devices: Vec<DeviceInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceInfo {
    #[serde(flatten)]
    device: romcrate_core::devices::Device,
    size_preset: &'static str,
    launch_platform: Option<&'static str>,
}

#[tauri::command]
fn get_catalog() -> Catalog {
    let kinds = ArtKind::ALL
        .iter()
        .map(|&k| KindInfo {
            id: k,
            label: k.label(),
            source: match k {
                ArtKind::Boxart | ArtKind::Snap | ArtKind::Title | ArtKind::Logo => "free",
                ArtKind::Box3d => "launchbox+generated",
                ArtKind::Boxback | ArtKind::Cartridge | ArtKind::Fanart => "launchbox",
                ArtKind::Mix => "generated",
                ArtKind::Video | ArtKind::Manual => "extras",
            },
        })
        .collect();
    Catalog {
        version: env!("CARGO_PKG_VERSION"),
        launchers: launchers::launchers(),
        systems: systems::SYSTEMS.to_vec(),
        kinds,
        size_presets: launchers::size_presets(),
        screenscraper_available: screenscraper::available(),
        cache_dir: default_cache_dir().to_string_lossy().to_string(),
        launch_profiles: romcrate_core::launch::db().profiles.clone(),
        devices: romcrate_core::devices::all()
            .iter()
            .map(|d| DeviceInfo {
                device: d.clone(),
                size_preset: d.size_preset(),
                launch_platform: d.launch_platform(),
            })
            .collect(),
    }
}

#[tauri::command]
async fn scan_library(
    app: AppHandle,
    roots: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Library, String> {
    let roots: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();
    // Zipped folders are unpacked next to the zip first.
    let app2 = app.clone();
    let (roots, errors) = tokio::task::spawn_blocking(move || {
        let last = std::sync::Mutex::new(std::time::Instant::now());
        romcrate_core::archive::prepare_roots(&roots, &|p| {
            let mut l = last.lock().unwrap();
            if l.elapsed().as_millis() > 150 || p.done_bytes == p.total_bytes {
                *l = std::time::Instant::now();
                let _ = app2.emit("unzip-progress", &p);
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    for e in errors {
        let _ = app.emit("scan-note", format!("Couldn't unzip {e}"));
    }
    state.openable.lock().unwrap().extend(roots.iter().cloned());
    let lib = tokio::task::spawn_blocking(move || scan::scan_roots(&roots))
        .await
        .map_err(|e| e.to_string())?;
    state
        .openable
        .lock()
        .unwrap()
        .extend(lib.systems.iter().map(|s| s.path.clone()));
    *state.library.lock().unwrap() = lib.clone();
    Ok(lib)
}

/// Assign a folder that wasn't auto-detected to a system.
#[tauri::command]
async fn add_system_folder(
    system_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<Library, String> {
    let sys = systems::by_id(&system_id).ok_or("unknown system")?;
    let dir = PathBuf::from(path);
    let scanned = tokio::task::spawn_blocking(move || scan::scan_system_folder(sys, &dir))
        .await
        .map_err(|e| e.to_string())?;
    state.openable.lock().unwrap().insert(scanned.path.clone());
    let mut lib = state.library.lock().unwrap();
    lib.systems.retain(|s| s.path != scanned.path);
    lib.systems.push(scanned);
    Ok(lib.clone())
}

#[tauri::command]
async fn start_scrape(
    app: AppHandle,
    selected: Vec<String>,
    config: ScrapeConfig,
    ss_user: Option<String>,
    ss_password: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("A scrape is already running".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *state.cancel.lock().unwrap() = Some(cancel.clone());
    let mut config = config;
    // Only ever run the Real-ESRGAN program, never an arbitrary executable.
    if let Some(bin) = &config.ai_binary {
        if !romcrate_core::imaging::is_esrgan_binary(bin) {
            config.ai_binary = None;
        }
    }
    if let Some(f) = &config.folder {
        state.openable.lock().unwrap().insert(f.clone());
    }

    let lib = state.library.lock().unwrap().clone();
    let client = match SafeClient::new() {
        Ok(c) => c,
        Err(e) => {
            state.running.store(false, Ordering::SeqCst);
            return Err(e.to_string());
        }
    };
    let cache = default_cache_dir();
    let user = match (ss_user, ss_password) {
        (Some(u), Some(p)) if !u.trim().is_empty() && !p.is_empty() => {
            Some((u.trim().to_string(), p))
        }
        _ => None,
    };
    let ss = if config.use_screenscraper {
        ScreenScraper::new(client.clone(), user)
    } else {
        None
    };
    let sources = Arc::new(Sources {
        launchbox: Some(romcrate_core::launchbox::LaunchBox::new(
            client.clone(),
            &cache,
        )),
        libretro: Libretro::new(client.clone(), cache.clone()),
        identifier: Identifier::new(client, cache),
        screenscraper: ss,
    });

    let previewable = state.previewable.clone();
    let app2 = app.clone();
    let emit: romcrate_core::scrape::Emit = Arc::new(move |e: Event| {
        if let Event::GameDone {
            preview: Some(p), ..
        } = &e
        {
            previewable.lock().unwrap().insert(p.clone());
        }
        if let Some(line) = romcrate_core::activity::describe(&e) {
            romcrate_core::activity::append(
                &romcrate_core::activity::log_dir(),
                "Artwork",
                &[line],
            );
        }
        let _ = app2.emit("scrape-event", &e);
    });
    let running = state.running.clone();
    tauri::async_runtime::spawn(async move {
        romcrate_core::run_scrape(lib, selected, config, sources, cancel, emit).await;
        running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Just the Pegasus launch lines for these ROM folders (no artwork).
#[tauri::command]
async fn write_launch_lines(
    roots: Vec<String>,
    launch: romcrate_core::launch::LaunchChoice,
) -> Result<Vec<String>, String> {
    let roots: Vec<PathBuf> = roots
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect();
    if roots.is_empty() {
        return Err("Add your ROM folders first".into());
    }
    if launch.platform.is_none() {
        return Err("Choose where you run Pegasus first".into());
    }
    tokio::task::spawn_blocking(move || {
        let lib = romcrate_core::scan::scan_roots(&roots);
        let notes = romcrate_core::launchers::write_launch_lines(&lib, &launch);
        romcrate_core::activity::append(
            &romcrate_core::activity::log_dir(),
            "Pegasus launch lines",
            &notes,
        );
        notes
    })
    .await
    .map_err(|e| e.to_string())
}

/// "Check my files": find unfinished downloads, damaged archives, store
/// packages, damaged N64 ROMs, disc formats that need converting and PC copy
/// protection. Reads only.
#[tauri::command]
async fn check_files(paths: Vec<String>) -> Result<romcrate_core::preflight::Report, String> {
    let paths: Vec<PathBuf> = paths
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();
    if paths.is_empty() {
        return Err("Choose a folder to check first".into());
    }
    let report =
        tokio::task::spawn_blocking(move || romcrate_core::preflight::check(&paths, &|_| {}))
            .await
            .map_err(|e| e.to_string())?;
    let mut lines: Vec<String> = report
        .findings
        .iter()
        .filter(|f| f.severity != romcrate_core::preflight::Severity::Info)
        .map(|f| {
            format!(
                "{:?}: {} — {}\n{}",
                f.severity,
                f.title,
                f.path.display(),
                f.advice
            )
        })
        .collect();
    lines.push(format!(
        "Checked {} files: {} problems, {} warnings",
        report.files_checked, report.errors, report.warnings
    ));
    romcrate_core::activity::append(
        &romcrate_core::activity::log_dir(),
        "Check my files",
        &lines,
    );
    Ok(report)
}

/// Look at a messy folder (or .zip) and work out where every game should go.
/// Reads only; nothing is copied or moved.
#[tauri::command]
async fn plan_import(
    app: AppHandle,
    sources: Vec<String>,
    dest: String,
    mode: Mode,
    rename: bool,
    state: State<'_, AppState>,
) -> Result<ImportPlan, String> {
    let dest = PathBuf::from(dest);
    if dest.as_os_str().is_empty() {
        return Err("Choose your ROMs folder first".into());
    }
    let sources: Vec<PathBuf> = sources.into_iter().map(PathBuf::from).collect();
    let app2 = app.clone();
    let (sources, errors) = tokio::task::spawn_blocking(move || {
        let last = std::sync::Mutex::new(std::time::Instant::now());
        romcrate_core::archive::prepare_roots(&sources, &|p| {
            let mut l = last.lock().unwrap();
            if l.elapsed().as_millis() > 150 || p.done_bytes == p.total_bytes {
                *l = std::time::Instant::now();
                let _ = app2.emit("unzip-progress", &p);
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    if !errors.is_empty() {
        return Err(format!("Couldn't unzip {}", errors.join("; ")));
    }
    let client = SafeClient::new().map_err(|e| e.to_string())?;
    let identifier = Identifier::new(client, default_cache_dir());
    let opts = ImportOptions { mode, rename };
    let plan = import::plan(&sources, &dest, &opts, Some(&identifier))
        .await
        .map_err(|e| e.to_string())?;
    {
        let mut open = state.openable.lock().unwrap();
        open.extend(sources.iter().cloned());
        if dest.is_dir() {
            open.insert(dest.clone());
        }
    }
    *state.import_plan.lock().unwrap() = Some(plan.clone());
    Ok(plan)
}

/// Carry out the last preview. `assign` = consoles the user picked for unknown
/// items, `exclude` = items they unticked. Progress arrives as "import-event".
#[tauri::command]
async fn start_import(
    app: AppHandle,
    assign: HashMap<usize, String>,
    exclude: Vec<usize>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let plan = state
        .import_plan
        .lock()
        .unwrap()
        .clone()
        .ok_or("Preview the import first")?;
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("Something is already running".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *state.cancel.lock().unwrap() = Some(cancel.clone());
    state.openable.lock().unwrap().insert(plan.dest.clone());
    let running = state.running.clone();
    let exclude: HashSet<usize> = exclude.into_iter().collect();
    tauri::async_runtime::spawn_blocking(move || {
        let last = Mutex::new(std::time::Instant::now());
        let summary = import::execute(&plan, &assign, &exclude, &cancel, &|e| {
            if let import::ImportEvent::Progress {
                done_bytes,
                total_bytes,
                ..
            } = &e
            {
                let mut l = last.lock().unwrap();
                if l.elapsed().as_millis() < 120 && done_bytes != total_bytes {
                    return;
                }
                *l = std::time::Instant::now();
            }
            match &e {
                import::ImportEvent::ItemDone { ok, message, .. } => {
                    romcrate_core::activity::append(
                        &romcrate_core::activity::log_dir(),
                        "Import",
                        &[format!("{} {message}", if *ok { "OK" } else { "FAILED" })],
                    )
                }
                import::ImportEvent::Finished { summary } => {
                    let mut l = vec![format!(
                        "Finished: {} imported, {} already there, {} left out, {} failed{}",
                        summary.imported,
                        summary.already_there,
                        summary.skipped,
                        summary.failed,
                        if summary.cancelled {
                            " (stopped early)"
                        } else {
                            ""
                        }
                    )];
                    l.extend(summary.errors.iter().cloned());
                    romcrate_core::activity::append(
                        &romcrate_core::activity::log_dir(),
                        "Import",
                        &l,
                    );
                }
                _ => {}
            }
            let _ = app.emit("import-event", &e);
        });
        drop(summary);
        running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
fn cancel_scrape(state: State<'_, AppState>) {
    if let Some(c) = state.cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::Relaxed);
    }
}

/// Small preview image as a data URL. Only for files produced in this session.
#[tauri::command]
async fn thumbnail(path: String, state: State<'_, AppState>) -> Result<String, String> {
    let p = PathBuf::from(&path);
    if !state.previewable.lock().unwrap().contains(&p) {
        return Err("not allowed".into());
    }
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
        let img = romcrate_core::imaging::decode_validated(&bytes).map_err(|e| e.to_string())?;
        let t = img.thumbnail(220, 220);
        let mut out = Vec::new();
        t.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(out)
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Best guess for the launcher's folder on this computer.
#[tauri::command]
fn guess_folder(launcher: LauncherKind) -> Option<String> {
    let home = dirs::home_dir()?;
    let candidates: Vec<PathBuf> = match launcher {
        LauncherKind::EsDe => vec![
            home.join("ES-DE/downloaded_media"),
            home.join(".emulationstation/downloaded_media"),
            home.join("Emulation/tools/downloaded_media"),
        ],
        LauncherKind::RetroArch => vec![
            home.join(".config/retroarch"),
            home.join("Library/Application Support/RetroArch"),
            home.join("AppData/Roaming/RetroArch"),
            PathBuf::from("C:/RetroArch-Win64"),
            home.join(".var/app/org.libretro.RetroArch/config/retroarch"),
        ],
        LauncherKind::LaunchBox => vec![
            home.join("LaunchBox"),
            PathBuf::from("C:/LaunchBox"),
            PathBuf::from("D:/LaunchBox"),
        ],
        _ => vec![],
    };
    candidates
        .into_iter()
        .find(|p| p.is_dir())
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn cache_size() -> u64 {
    fn walk(p: &std::path::Path) -> u64 {
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| {
                let m = e.metadata();
                match m {
                    Ok(m) if m.is_dir() => walk(&e.path()),
                    Ok(m) => m.len(),
                    _ => 0,
                }
            })
            .sum()
    }
    walk(&default_cache_dir())
}

#[tauri::command]
fn clear_cache() -> Result<(), String> {
    let dir = default_cache_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Ask where to save the "missing games" report and write it. The path comes
/// from the native save dialog, never from the web page.
#[tauri::command]
async fn save_missing_report(app: AppHandle, text: String) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let Some(choice) = app
        .dialog()
        .file()
        .set_title("Save missing games list")
        .set_file_name("romcrate-missing.txt")
        .add_filter("Text", &["txt"])
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let mut path = choice.into_path().map_err(|e| e.to_string())?;
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("txt"))
        != Some(true)
    {
        path.set_extension("txt");
    }
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Open a folder in the system file manager — only folders the user chose this session.
#[tauri::command]
fn open_folder(app: AppHandle, path: String, state: State<'_, AppState>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = PathBuf::from(&path);
    if !p.is_dir() || !state.openable.lock().unwrap().contains(&p) {
        return Err("That folder can't be opened from here".into());
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Where the activity log lives (shown in Settings).
#[tauri::command]
fn log_info() -> serde_json::Value {
    use romcrate_core::datadir;
    let dir = romcrate_core::activity::log_dir();
    let loc = datadir::location();
    serde_json::json!({
        "dir": dir.to_string_lossy(),
        "today": romcrate_core::activity::today_file(&dir).to_string_lossy(),
        "dataDir": loc.data_dir.as_ref().map(|d| d.to_string_lossy().to_string()),
        "defaultLogDir": datadir::default_log_dir().to_string_lossy(),
        "defaultCacheDir": datadir::default_cache_dir().to_string_lossy(),
        "cacheDir": datadir::cache_dir().to_string_lossy(),
        "logEnabled": loc.log_enabled,
    })
}

/// Use another folder for the log + downloads (`None` = back to the default).
#[tauri::command]
async fn set_data_dir(path: Option<String>) -> Result<Vec<String>, String> {
    let notes = tokio::task::spawn_blocking(move || {
        romcrate_core::datadir::set_data_dir(path.map(PathBuf::from))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    romcrate_core::activity::append(
        &romcrate_core::activity::log_dir(),
        "Settings",
        &[
            vec![format!(
                "RomCrate's folder is now {}",
                romcrate_core::datadir::log_dir()
                    .parent()
                    .unwrap_or(Path::new(""))
                    .display()
            )],
            notes.clone(),
        ]
        .concat(),
    );
    Ok(notes)
}

#[tauri::command]
fn set_log_enabled(on: bool) -> Result<(), String> {
    romcrate_core::datadir::set_log_enabled(on).map_err(|e| e.to_string())
}

/// Open today's log as a text file (an empty one with a heading if nothing
/// was logged yet today).
#[tauri::command]
fn open_log_file(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = romcrate_core::activity::log_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let f = romcrate_core::activity::today_file(&dir);
    if !f.exists() {
        std::fs::write(&f, "RomCrate activity log — nothing logged yet today.\n")
            .map_err(|e| e.to_string())?;
    }
    app.opener()
        .open_path(f.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Write a problem report (problems from the last week, your user name taken
/// out of paths) and open it.
#[tauri::command]
fn save_problem_report(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = romcrate_core::activity::log_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = romcrate_core::activity::problem_report(&dir, env!("CARGO_PKG_VERSION"));
    let f = romcrate_core::activity::report_file(&dir);
    std::fs::write(&f, text).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(f.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(f.to_string_lossy().to_string())
}

// ---------------------------------------------------------------- send to handheld

fn find_adb() -> Result<romcrate_core::adb::Adb, String> {
    romcrate_core::adb::Adb::find()
        .map(|(a, _)| a)
        .ok_or_else(|| "Android's USB tool isn't installed yet".to_string())
}

#[tauri::command]
async fn adb_status() -> serde_json::Value {
    let st = tokio::task::spawn_blocking(romcrate_core::adb::Adb::status)
        .await
        .ok();
    serde_json::json!({ "status": st, "termsUrl": romcrate_core::adb::TERMS_URL })
}

/// Download Google's platform-tools (checked against Google's checksum).
#[tauri::command]
async fn adb_install(app: AppHandle) -> Result<String, String> {
    let client = romcrate_core::net::SafeClient::new().map_err(|e| e.to_string())?;
    let app2 = app.clone();
    let exe = romcrate_core::adb::install(&client, &move |done, total| {
        let _ = app2.emit(
            "adb-install-progress",
            serde_json::json!({ "done": done, "total": total }),
        );
    })
    .await
    .map_err(|e| e.to_string())?;
    romcrate_core::activity::append(
        &romcrate_core::activity::log_dir(),
        "Send to handheld",
        &[format!(
            "Downloaded Android's USB tool (platform-tools) from Google, checksum OK: {}",
            exe.display()
        )],
    );
    Ok(exe.to_string_lossy().into())
}

#[tauri::command]
async fn adb_devices() -> Result<Vec<romcrate_core::adb::Device>, String> {
    let adb = find_adb()?;
    tokio::task::spawn_blocking(move || adb.devices())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn adb_volumes(serial: String) -> Result<Vec<romcrate_core::adb::Volume>, String> {
    let adb = find_adb()?;
    tokio::task::spawn_blocking(move || adb.volumes(&serial))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Work out what to copy. Only console folders from "Your games" can be sent.
#[tauri::command]
async fn send_plan(
    serial: String,
    targets: Vec<romcrate_core::adb::Target>,
    state: State<'_, AppState>,
) -> Result<romcrate_core::adb::SendPlan, String> {
    let known: HashSet<PathBuf> = state
        .library
        .lock()
        .unwrap()
        .systems
        .iter()
        .map(|s| s.path.clone())
        .collect();
    if let Some(t) = targets.iter().find(|t| !known.contains(&t.local)) {
        return Err(format!(
            "{} isn't one of your game folders",
            t.local.display()
        ));
    }
    let adb = find_adb()?;
    let serial2 = serial.clone();
    let plan = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let vols = adb.volumes(&serial2)?;
        let mut err = None;
        let plan = romcrate_core::adb::plan(&targets, &vols, &mut |d| {
            adb.list_files(&serial2, d).unwrap_or_else(|e| {
                err.get_or_insert(e.to_string());
                Default::default()
            })
        })?;
        if let Some(e) = err {
            anyhow::bail!("couldn't look at the handheld: {e}");
        }
        Ok(plan)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    *state.send_plan.lock().unwrap() = Some((serial, plan.clone()));
    Ok(plan)
}

#[tauri::command]
async fn start_send(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let (serial, plan) = state
        .send_plan
        .lock()
        .unwrap()
        .clone()
        .ok_or("Check what will be copied first")?;
    let adb = find_adb()?;
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("Something is already running".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *state.cancel.lock().unwrap() = Some(cancel.clone());
    let running = state.running.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let log = |l: String| {
            romcrate_core::activity::append(
                &romcrate_core::activity::log_dir(),
                "Send to handheld",
                &[l],
            )
        };
        log(format!(
            "Started: {} files ({}) to {serial}; {} already there, {} left alone (different file with the same name on the handheld)",
            plan.files.len(),
            romcrate_core::archive::human(plan.files.iter().map(|f| f.bytes).sum()),
            plan.already_there,
            plan.conflicts.len()
        ));
        for c in &plan.conflicts {
            log(format!(
                "Left alone (a different file is already there): {c}"
            ));
        }
        for n in &plan.notes {
            log(n.clone());
        }
        romcrate_core::adb::send(
            &adb,
            &serial,
            &plan,
            &|e| {
                match &e {
                    romcrate_core::adb::SendEvent::File { remote, ok, message, .. } => log(format!(
                        "{} {remote} — {message}",
                        if *ok { "OK" } else { "FAILED" }
                    )),
                    romcrate_core::adb::SendEvent::Finished { summary } => log(format!(
                        "Finished: {} copied ({} checked by checksum), {} failed, {} already there{}",
                        summary.copied,
                        summary.verified,
                        summary.failed,
                        summary.already_there,
                        if summary.cancelled { " — stopped early" } else { "" }
                    )),
                    _ => {}
                }
                let _ = app.emit("send-event", &e);
            },
            &|| cancel.load(Ordering::Relaxed),
        );
        running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Open the activity log folder in the file manager.
#[tauri::command]
fn open_log_folder(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = romcrate_core::activity::log_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// On small screens (e.g. 1024×768, or big display scaling) shrink the window so it
/// isn't opened partly off-screen.
fn fit_to_screen(app: &tauri::App) {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Ok(Some(mon)) = win.current_monitor() else {
        return;
    };
    let scale = mon.scale_factor();
    let screen = mon.size().to_logical::<f64>(scale);
    let Ok(size) = win.outer_size() else { return };
    let size = size.to_logical::<f64>(scale);
    let w = size.width.min(screen.width * 0.95);
    let h = size.height.min(screen.height * 0.9);
    if w < size.width || h < size.height {
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        // Place it explicitly: center() can use the old size on some platforms (macOS).
        let origin = mon.position().to_logical::<f64>(scale);
        let _ = win.set_position(tauri::LogicalPosition::new(
            origin.x + (screen.width - w) / 2.0,
            origin.y + (screen.height - h) / 2.0,
        ));
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            fit_to_screen(app);
            romcrate_core::activity::prune(&romcrate_core::activity::log_dir());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_catalog,
            scan_library,
            add_system_folder,
            start_scrape,
            cancel_scrape,
            plan_import,
            start_import,
            check_files,
            write_launch_lines,
            log_info,
            open_log_folder,
            open_log_file,
            adb_status,
            adb_install,
            adb_devices,
            adb_volumes,
            send_plan,
            start_send,
            save_problem_report,
            set_data_dir,
            set_log_enabled,
            thumbnail,
            guess_folder,
            cache_size,
            clear_cache,
            save_missing_report,
            open_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running RomCrate");
}
