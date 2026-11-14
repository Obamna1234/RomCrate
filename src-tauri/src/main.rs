// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine;
use serde::Serialize;
use shelfart_core::identify::Identifier;
use shelfart_core::launchers::{self, LauncherInfo, LauncherKind};
use shelfart_core::net::SafeClient;
use shelfart_core::scan::{self, Library};
use shelfart_core::screenscraper::{self, ScreenScraper};
use shelfart_core::source::default_cache_dir;
use shelfart_core::systems::{self, System};
use shelfart_core::{ArtKind, Event, Libretro, ScrapeConfig, Sources};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
struct AppState {
    library: Mutex<Library>,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// Files ShelfArt produced or found this session; only these can be previewed.
    previewable: Arc<Mutex<HashSet<PathBuf>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KindInfo {
    id: ArtKind,
    label: &'static str,
    /// "free" (libretro), "extras" (ScreenScraper), "generated" (made by ShelfArt), "free+generated"
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
                ArtKind::Mix => "generated",
                ArtKind::Box3d => "extras+generated",
                _ => "extras",
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
    }
}

#[tauri::command]
async fn scan_library(roots: Vec<String>, state: State<'_, AppState>) -> Result<Library, String> {
    let roots: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();
    let lib = tokio::task::spawn_blocking(move || scan::scan_roots(&roots))
        .await
        .map_err(|e| e.to_string())?;
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
    let mut cancel_slot = state.cancel.lock().unwrap();
    if cancel_slot
        .as_ref()
        .map(|c| !c.load(Ordering::Relaxed))
        .unwrap_or(false)
    {
        return Err("A scrape is already running".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *cancel_slot = Some(cancel.clone());
    drop(cancel_slot);

    let lib = state.library.lock().unwrap().clone();
    let client = SafeClient::new().map_err(|e| e.to_string())?;
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
        libretro: Libretro::new(client.clone(), cache.clone()),
        identifier: Identifier::new(client, cache),
        screenscraper: ss,
    });

    let previewable = state.previewable.clone();
    let app2 = app.clone();
    let emit: shelfart_core::scrape::Emit = Arc::new(move |e: Event| {
        if let Event::GameDone {
            preview: Some(p), ..
        } = &e
        {
            previewable.lock().unwrap().insert(p.clone());
        }
        let _ = app2.emit("scrape-event", &e);
    });
    let done_flag = cancel.clone();
    tauri::async_runtime::spawn(async move {
        shelfart_core::run_scrape(lib, selected, config, sources, cancel.clone(), emit).await;
        // Mark as not running.
        done_flag.store(true, Ordering::Relaxed);
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
        let img = shelfart_core::imaging::decode_validated(&bytes).map_err(|e| e.to_string())?;
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

/// Save a plain-text report to a path the user picked in a save dialog.
#[tauri::command]
fn save_report(path: String, text: String) -> Result<(), String> {
    let p = PathBuf::from(path);
    if p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("txt"))
        != Some(true)
    {
        return Err("Reports are saved as .txt files".into());
    }
    std::fs::write(p, text).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_catalog,
            scan_library,
            add_system_folder,
            start_scrape,
            cancel_scrape,
            thumbnail,
            guess_folder,
            cache_size,
            clear_cache,
            save_report
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShelfArt");
}
