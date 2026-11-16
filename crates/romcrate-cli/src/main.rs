//! `romcrate` command-line interface — handy for scripts, servers and handhelds.

use anyhow::{bail, Context};
use romcrate_core::imaging::Upscale;
use romcrate_core::launchers::{self, LauncherKind};
use romcrate_core::net::SafeClient;
use romcrate_core::scan;
use romcrate_core::screenscraper::{self, ScreenScraper};
use romcrate_core::source::default_cache_dir;
use romcrate_core::{identify::Identifier, ArtKind, Event, Libretro, ScrapeConfig, Sources};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const HELP: &str = "RomCrate — get your games ready for your handheld (sorted, checked, with artwork)

USAGE:
  romcrate-cli check <folder or file>...
  romcrate-cli scan <ROM folder>...
  romcrate-cli scrape --roms <folder> --launcher <name> [options]
  romcrate-cli launchers
  romcrate-cli emulators            (Pegasus launch lines RomCrate knows)
  romcrate-cli devices              (devices RomCrate has defaults for)
  romcrate-cli pegasus --roms <dir>... [--pegasus-on android] [--emulator s=id] [--fix-launch]
                                    (write just the Pegasus launch lines, no artwork)
  romcrate-cli import --from <folder|zip> --to <ROMs folder> [options]

CHECK MY FILES:
  Finds unfinished downloads, damaged zips, PlayStation Store packages, damaged N64 ROMs,
  disc formats that need converting, .bin files without their .cue, PS3 games, and PC games
  whose disc copy protection won't run under Wine/GameNative. Exits with code 1 if any
  file has a serious problem.

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
  --pegasus-on <where>  android   Write Pegasus launch lines for an Android handheld
  --emulator <s>=<id>   Choose the emulator for a system, e.g. ps2=armsx2-playstore (see 'emulators')
  --fix-launch          Also add/fix launch lines in Pegasus files you already have
                        (a .bak backup is made first; only the launch block changes)
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
        Some("check") => {
            let roots: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
            if roots.is_empty() {
                bail!("check needs at least one folder or file\n\n{HELP}");
            }
            for r in &roots {
                if !r.exists() {
                    bail!("{} doesn't exist", r.display());
                }
            }
            let report = romcrate_core::preflight::check(&roots, &|_| {});
            use romcrate_core::preflight::Severity;
            for f in &report.findings {
                let tag = match f.severity {
                    Severity::Error => "PROBLEM",
                    Severity::Warn => "WARNING",
                    Severity::Info => "info   ",
                };
                println!(
                    "{tag}  {}\n         {}\n         {}",
                    f.path.display(),
                    f.title,
                    f.advice
                );
            }
            println!(
                "\nChecked {} files: {} problem{}, {} warning{}.",
                report.files_checked,
                report.errors,
                if report.errors == 1 { "" } else { "s" },
                report.warnings,
                if report.warnings == 1 { "" } else { "s" }
            );
            if report.errors > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        Some("devices") => {
            for d in romcrate_core::devices::all() {
                println!(
                    "{:<18} {:<46} {}x{}  {}",
                    d.id,
                    d.name,
                    d.screen.width,
                    d.screen.height,
                    if d.too_demanding.is_empty() {
                        String::new()
                    } else {
                        format!("too demanding: {}", d.too_demanding.join(", "))
                    }
                );
            }
            Ok(())
        }
        Some("pegasus") => {
            let mut roots = Vec::new();
            let mut choice = romcrate_core::launch::LaunchChoice {
                platform: Some("android".into()),
                ..Default::default()
            };
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                let mut val = || {
                    it.next()
                        .cloned()
                        .with_context(|| format!("{a} needs a value"))
                };
                match a.as_str() {
                    "--roms" => roots.push(PathBuf::from(val()?)),
                    "--pegasus-on" => {
                        let v = val()?;
                        if v != "android" {
                            bail!("--pegasus-on supports: android");
                        }
                    }
                    "--emulator" => {
                        let v = val()?;
                        let (sys, id) = v
                            .split_once('=')
                            .context("use --emulator <system>=<profile id>")?;
                        let p = romcrate_core::launch::profile(id)
                            .with_context(|| format!("unknown emulator profile '{id}'"))?;
                        if !p.systems.iter().any(|s| s == sys) {
                            bail!("{} doesn't run {sys} games", p.emulator);
                        }
                        choice.per_system.insert(sys.to_string(), id.to_string());
                    }
                    "--fix-launch" => choice.fix_existing = true,
                    other => bail!("unknown option {other}\n\n{HELP}"),
                }
            }
            if roots.is_empty() {
                bail!("--roms <your ROMs folder> is required");
            }
            let lib = scan::scan_roots(&unzip_roots(&roots));
            for n in romcrate_core::launchers::write_launch_lines(&lib, &choice) {
                println!("{n}");
            }
            Ok(())
        }
        Some("emulators") => {
            for p in &romcrate_core::launch::db().profiles {
                println!(
                    "{:<18} {:<44} {:<8} {}",
                    p.id,
                    p.emulator,
                    p.systems.join(","),
                    if p.verified {
                        "tested on a device"
                    } else {
                        "not tested yet (written commented out)"
                    }
                );
            }
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
    let (out, errors) = romcrate_core::archive::prepare_roots(roots, &|p| {
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
    use romcrate_core::archive::human;
    use romcrate_core::import::{self, ImportOptions, ItemState, Mode};
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
                    .context("unknown launcher (see `romcrate-cli launchers`)")?
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
            "--pegasus-on" => {
                let v = val()?;
                if v != "android" {
                    bail!("--pegasus-on supports: android");
                }
                cfg.launch.platform = Some(v);
            }
            "--emulator" => {
                let v = val()?;
                let (sys, id) = v
                    .split_once('=')
                    .context("use --emulator <system>=<profile id>, e.g. ps2=armsx2-playstore")?;
                let p = romcrate_core::launch::profile(id).with_context(|| {
                    format!("unknown emulator profile '{id}' — see 'romcrate-cli emulators'")
                })?;
                if !p.systems.iter().any(|s| s == sys) {
                    bail!("{} doesn't run {sys} games", p.emulator);
                }
                cfg.launch
                    .per_system
                    .insert(sys.to_string(), id.to_string());
            }
            "--fix-launch" => cfg.launch.fix_existing = true,
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
        launchbox: Some(romcrate_core::launchbox::LaunchBox::new(
            client.clone(),
            &cache,
        )),
        libretro: Libretro::new(client.clone(), cache.clone()),
        identifier: Identifier::new(client, cache),
        screenscraper: ss,
    });
    let emit: romcrate_core::scrape::Emit = Arc::new(|e: Event| {
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
    romcrate_core::run_scrape(
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
