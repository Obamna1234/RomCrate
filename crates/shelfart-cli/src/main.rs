//! `shelfart` command-line interface — handy for scripts, servers and handhelds.

use anyhow::{bail, Context};
use shelfart_core::imaging::Upscale;
use shelfart_core::launchers::{self, LauncherKind};
use shelfart_core::net::SafeClient;
use shelfart_core::scan;
use shelfart_core::screenscraper::{self, ScreenScraper};
use shelfart_core::source::default_cache_dir;
use shelfart_core::{identify::Identifier, ArtKind, Event, Libretro, ScrapeConfig, Sources};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const HELP: &str = "ShelfArt — account-free artwork scraper for emulated games

USAGE:
  shelfart-cli scan <ROM folder>...
  shelfart-cli scrape --roms <folder> --launcher <name> [options]
  shelfart-cli launchers
  shelfart-cli import --from <folder|zip> --to <ROMs folder> [options]

IMPORT & SORT (shows a preview first; nothing changes without --yes):
  --from <dir|zip>      Messy folder (or .zip) to sort (repeatable)
  --to <dir>            Your ROMs folder; games go into <dir>/<console>/
  --move                Move instead of copy (originals removed only after a verified copy)
  --rename              Rename single-file games to their official name when identified
  --assign <file>=<id>  Choose the console for a file it couldn't work out (e.g. \"Game.cue=psx\")
  --skip <file>         Leave this file out (repeatable)
  --yes                 Actually do it (after reviewing the preview)

SCRAPE OPTIONS:
  --roms <dir|zip>      ROM folder or .zip of one (repeatable). A zip is unpacked next to itself first.
  --launcher <name>     pegasus | es-de | emulation-station | retro-arch | launch-box | folder
  --folder <dir>        Media / RetroArch / LaunchBox / output folder (when the launcher needs one)
  --kinds <list>        Comma list: boxart,box3d,boxback,cartridge,snap,title,logo,fanart,mix,video,manual
  --systems <list>      Only these system ids (e.g. snes,psx)
  --size <preset>       compact | standard | hd | uhd   (default standard)
  --upscale <mode>      off | smooth | smart | ai        (default smart)
  --esrgan <path>       realesrgan-ncnn-vulkan executable (for --upscale ai)
  --regions <list>      Region preference, e.g. usa,europe,japan
  --overwrite           Replace existing art
  --no-hash             Match by file name only
  --no-extras           Don't use ScreenScraper guest mode
  --no-launchbox        Don't use the free LaunchBox games database
  --no-game-info        Don't write descriptions/genre/release dates
  --no-metadata         Don't touch gamelist.xml / Pegasus metadata / playlists
  --pegasus-layout <l>  auto | per-game | skraper   (default auto: match what's already there)
  --threads <n>         Parallel games, 1-8 (default 4)
";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = tokio::runtime::Runtime::new()?;
    match args.first().map(String::as_str) {
        Some("scan") => {
            let roots: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
            let roots = unzip_roots(&roots);
            let lib = scan::scan_roots(&roots);
            for s in &lib.systems {
                println!(
                    "{:<32} {:>5} games   {}",
                    s.system_name,
                    s.games.len(),
                    s.path.display()
                );
            }
            println!(
                "Total: {} games in {} systems",
                lib.game_count(),
                lib.systems.len()
            );
            Ok(())
        }
        Some("launchers") => {
            for l in launchers::launchers() {
                println!(
                    "{}  —  {}",
                    serde_json::to_string(&l.id)?.trim_matches('"'),
                    l.name
                );
            }
            Ok(())
        }
        Some("scrape") => rt.block_on(scrape(&args[1..])),
        Some("import") => rt.block_on(import(&args[1..])),
        _ => {
            print!("{HELP}");
            Ok(())
        }
    }
}

/// Unpack any .zip roots (next to the zip) and return folders to scan.
fn unzip_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let last = std::sync::Mutex::new(0u64);
    let (out, errors) = shelfart_core::archive::prepare_roots(roots, &|p| {
        let pct = p.done_bytes * 100 / p.total_bytes.max(1);
        let mut l = last.lock().unwrap();
        if pct >= *l + 10 || pct == 100 {
            *l = pct;
            eprintln!("unzipping {}: {pct}%", p.zip.display());
        }
    });
    for e in errors {
        eprintln!("couldn't unzip {e}");
    }
    out
}

async fn import(args: &[String]) -> anyhow::Result<()> {
    use shelfart_core::archive::human;
    use shelfart_core::import::{self, ImportOptions, ItemState, Mode};
    use std::collections::{HashMap, HashSet};

    let mut sources = Vec::new();
    let mut dest: Option<PathBuf> = None;
    let mut opts = ImportOptions::default();
    let mut yes = false;
    let mut assign_names: Vec<(String, String)> = Vec::new();
    let mut skip_names: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || {
            it.next()
                .cloned()
                .with_context(|| format!("{a} needs a value"))
        };
        match a.as_str() {
            "--from" => sources.push(PathBuf::from(val()?)),
            "--to" => dest = Some(PathBuf::from(val()?)),
            "--move" => opts.mode = Mode::Move,
            "--rename" => opts.rename = true,
            "--yes" | "-y" => yes = true,
            "--assign" => {
                let v = val()?;
                let (f, s) = v
                    .rsplit_once('=')
                    .context("use --assign \"file name=console id\"")?;
                assign_names.push((f.to_string(), s.trim().to_string()));
            }
            "--skip" => skip_names.push(val()?),
            other => bail!("unknown option {other}\n\n{HELP}"),
        }
    }
    let dest = dest.context("--to <your ROMs folder> is required")?;
    if sources.is_empty() {
        bail!("--from <folder> is required\n\n{HELP}");
    }
    let sources = unzip_roots(&sources);
    let client = SafeClient::new()?;
    let identifier = Identifier::new(client, default_cache_dir());
    let plan = import::plan(&sources, &dest, &opts, Some(&identifier)).await?;

    println!("Import into {}", dest.display());
    for s in &plan.systems {
        println!(
            "  {:<34} {:>4} games  {:>9}   → {}/",
            s.name,
            s.games,
            human(s.bytes),
            s.folder
        );
    }
    println!();
    let mut assign = HashMap::new();
    let mut exclude = HashSet::new();
    for item in &plan.items {
        if let Some((_, sys)) = assign_names.iter().find(|(f, _)| *f == item.name) {
            assign.insert(item.id, sys.clone());
        }
        if skip_names.contains(&item.name) {
            exclude.insert(item.id);
        }
        let to = item
            .dest_folder
            .as_deref()
            .map(|f| format!("{f}/{}", item.dest_name))
            .unwrap_or_default();
        let state = match &item.state {
            ItemState::Ready => format!("→ {to}"),
            ItemState::AlreadyThere => "already in your ROMs folder".into(),
            ItemState::Conflict(m) => format!("NOT imported: {m}"),
            ItemState::NeedsSystem => {
                "? console unknown — use --assign \"name=console\" or --skip".into()
            }
            ItemState::Skipped(m) => format!("skipped: {m}"),
        };
        println!("  {}  [{}]", item.name, item.reason);
        println!("      {state}");
        for w in &item.warnings {
            println!("      ! {w}");
        }
    }
    for n in &plan.notes {
        println!("  note: {n}");
    }
    if plan.ignored_files > 0 {
        println!(
            "  note: {} non-game files (text, pictures, installers…) left alone.",
            plan.ignored_files
        );
    }
    println!(
        "\nNeeds {} on the destination drive; {} free.{}",
        human(plan.needed_bytes),
        plan.free_bytes
            .map(human)
            .unwrap_or_else(|| "unknown".into()),
        if plan.enough_space {
            ""
        } else {
            "  NOT ENOUGH SPACE."
        }
    );
    if !yes {
        println!("Nothing was changed. Run again with --yes to import.");
        return Ok(());
    }
    if !plan.enough_space {
        bail!("not enough free space — nothing was changed");
    }
    let last = std::sync::Mutex::new(0u64);
    let summary = import::execute(
        &plan,
        &assign,
        &exclude,
        &AtomicBool::new(false),
        &|e| match e {
            import::ImportEvent::Progress {
                done_bytes,
                total_bytes,
                ..
            } => {
                let pct = done_bytes * 100 / total_bytes.max(1);
                let mut l = last.lock().unwrap();
                if pct >= *l + 10 {
                    *l = pct;
                    eprintln!("  {pct}%");
                }
            }
            import::ImportEvent::ItemDone { ok, message, .. } => {
                println!("  {} {message}", if ok { "✓" } else { "✗" })
            }
            _ => {}
        },
    );
    println!(
        "\nDone: {} imported, {} already there, {} skipped, {} failed ({}).",
        summary.imported,
        summary.already_there,
        summary.skipped,
        summary.failed,
        human(summary.bytes)
    );
    for e in &summary.errors {
        println!("  ✗ {e}");
    }
    if summary.failed > 0 {
        std::process::exit(2);
    }
    Ok(())
}

async fn scrape(args: &[String]) -> anyhow::Result<()> {
    let mut cfg = ScrapeConfig::default();
    let mut roots = Vec::new();
    let mut systems = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || {
            it.next()
                .cloned()
                .with_context(|| format!("{a} needs a value"))
        };
        match a.as_str() {
            "--roms" => roots.push(PathBuf::from(val()?)),
            "--launcher" => {
                cfg.launcher = serde_json::from_str(&format!("\"{}\"", val()?))
                    .context("unknown launcher (see `shelfart-cli launchers`)")?
            }
            "--folder" => cfg.folder = Some(PathBuf::from(val()?)),
            "--kinds" => {
                cfg.kinds = val()?
                    .split(',')
                    .map(|k| {
                        serde_json::from_str::<ArtKind>(&format!("\"{}\"", k.trim()))
                            .with_context(|| format!("unknown art type {k}"))
                    })
                    .collect::<Result<_, _>>()?
            }
            "--systems" => systems = val()?.split(',').map(|s| s.trim().to_string()).collect(),
            "--size" => {
                let v = val()?;
                cfg.size_scale = launchers::size_presets()
                    .into_iter()
                    .find(|p| p.0 == v)
                    .map(|p| p.2)
                    .context("unknown size")?;
            }
            "--upscale" => {
                cfg.upscale = serde_json::from_str::<Upscale>(&format!("\"{}\"", val()?))
                    .context("unknown upscale mode")?
            }
            "--esrgan" => cfg.ai_binary = Some(PathBuf::from(val()?)),
            "--regions" => {
                cfg.prefs.regions = val()?.split(',').map(|s| s.trim().to_lowercase()).collect()
            }
            "--overwrite" => cfg.overwrite = true,
            "--no-hash" => cfg.identify_by_contents = false,
            "--no-extras" => cfg.use_screenscraper = false,
            "--no-launchbox" => cfg.use_launchbox = false,
            "--no-game-info" => cfg.game_info = false,
            "--no-metadata" => cfg.write_metadata = false,
            "--pegasus-layout" => {
                cfg.pegasus_layout = serde_json::from_str(&format!("\"{}\"", val()?))
                    .context("use auto, per-game or skraper")?
            }
            "--threads" => cfg.concurrency = val()?.parse()?,
            other => bail!("unknown option {other}\n\n{HELP}"),
        }
    }
    if roots.is_empty() {
        bail!("--roms is required\n\n{HELP}");
    }
    if cfg.launcher != LauncherKind::Pegasus
        && cfg.launcher != LauncherKind::EmulationStation
        && cfg.folder.is_none()
    {
        bail!("this launcher needs --folder");
    }
    let roots = unzip_roots(&roots);
    let lib = scan::scan_roots(&roots);
    if lib.systems.is_empty() {
        bail!("no systems found — make sure your ROMs are in folders named after the system (snes, psx, gba…)");
    }
    let client = SafeClient::new()?;
    let cache = default_cache_dir();
    let ss = if cfg.use_screenscraper {
        ScreenScraper::new(client.clone(), None)
    } else {
        None
    };
    if cfg.use_screenscraper && !screenscraper::available() {
        eprintln!("note: this build has no ScreenScraper access; using the free collection only");
    }
    let sources = Arc::new(Sources {
        launchbox: Some(shelfart_core::launchbox::LaunchBox::new(
            client.clone(),
            &cache,
        )),
        libretro: Libretro::new(client.clone(), cache.clone()),
        identifier: Identifier::new(client, cache),
        screenscraper: ss,
    });
    let emit: shelfart_core::scrape::Emit = Arc::new(|e: Event| {
        match e {
        Event::SystemStarted { system_name, games, .. } => println!("\n== {system_name} ({games} games)"),
        Event::SystemIndex { artwork, database, .. } => println!("   {artwork} artwork entries, {database} database entries"),
        Event::GameDone { game, status, matched, method, saved, missing, message, .. } => {
            let s: Vec<_> = saved.iter().map(|k| k.id()).collect();
            let m: Vec<_> = missing.iter().map(|k| k.id()).collect();
            println!(
                "   [{:?}] {game}{}{}{}{}",
                status,
                matched.map(|m| format!("  →  {m}")).unwrap_or_default(),
                method.map(|m| format!(" ({m})")).unwrap_or_default(),
                if s.is_empty() { String::new() } else { format!("  saved: {}", s.join(",")) },
                if m.is_empty() { String::new() } else { format!("  missing: {}", m.join(",")) },
            );
            if let Some(msg) = message {
                println!("        {msg}");
            }
        }
        Event::Log { level, message } if level != "debug" => println!("   [{level}] {message}"),
        Event::Finished { summary } => println!(
            "\nDone: {} games — {} complete, {} partial, {} already had art, {} not found, {} failed. {} files saved.",
            summary.games, summary.done, summary.partial, summary.skipped, summary.missing, summary.failed, summary.files_saved
        ),
        _ => {}
    }
    });
    shelfart_core::run_scrape(
        lib,
        systems,
        cfg,
        sources,
        Arc::new(AtomicBool::new(false)),
        emit,
    )
    .await;
    Ok(())
}
