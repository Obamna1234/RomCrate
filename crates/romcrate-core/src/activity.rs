//! Activity log: a plain-text record of what RomCrate did, kept on disk so you
//! can look back at it (or share it when asking for help). One file per day in
//! the app's data folder; old files are removed after 30 days. Paths in the log
//! are your own folders — nothing is sent anywhere.

use crate::scrape::{Event, GameStatus};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const KEEP_DAYS: i64 = 30;

/// Folder holding the log files.
pub fn log_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("RomCrate")
        .join("logs")
}

/// Today's log file.
pub fn today_file(dir: &Path) -> PathBuf {
    dir.join(format!(
        "romcrate-{}.log",
        chrono::Local::now().format("%Y-%m-%d")
    ))
}

/// Append lines under a task heading. Never fails loudly: logging must not
/// break the task itself.
pub fn append(dir: &Path, task: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(today_file(dir))
    else {
        return;
    };
    let time = chrono::Local::now().format("%H:%M:%S");
    for l in lines {
        for (i, part) in l.lines().enumerate() {
            let _ = if i == 0 {
                writeln!(f, "{time} [{task}] {part}")
            } else {
                writeln!(f, "{time} [{task}]     {part}")
            };
        }
    }
}

/// Remove log files older than 30 days.
pub fn prune(dir: &Path) {
    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(KEEP_DAYS);
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(date) = name
            .strip_prefix("romcrate-")
            .and_then(|n| n.strip_suffix(".log"))
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        else {
            continue;
        };
        if date < cutoff {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// A scrape event as one readable log line (None for progress noise).
pub fn describe(e: &Event) -> Option<String> {
    Some(match e {
        Event::Started { total_games, systems, .. } => {
            format!("Started: {total_games} games in {systems} console folders")
        }
        Event::SystemStarted { system_name, games, .. } => format!("{system_name}: {games} games"),
        Event::SystemIndex { .. } => return None,
        Event::GameDone { game, status, matched, method, saved, missing, message, .. } => {
            let what = match status {
                GameStatus::Done => "OK",
                GameStatus::Partial => "PARTLY",
                GameStatus::Skipped => "already had art",
                GameStatus::Missing => "NOT FOUND",
                GameStatus::Failed => "FAILED",
            };
            let mut s = format!("{what}: {game}");
            if let Some(m) = matched {
                s.push_str(&format!(" → {m}"));
                if let Some(how) = method {
                    s.push_str(&format!(" ({how})"));
                }
            }
            let list = |v: &[crate::source::ArtKind]| v.iter().map(|k| k.label()).collect::<Vec<_>>().join(", ");
            if !saved.is_empty() {
                s.push_str(&format!(" · saved {}", list(saved)));
            }
            if !missing.is_empty() {
                s.push_str(&format!(" · no {} available", list(missing)));
            }
            if let Some(m) = message {
                s.push_str(&format!(" · {m}"));
            }
            s
        }
        Event::Log { level, message } => format!("{}{message}", if level == "info" { "" } else { "⚠ " }),
        Event::Finished { summary } => format!(
            "Finished: {} games — {} done, {} partly, {} already had art, {} not found, {} failed{}",
            summary.games,
            summary.done,
            summary.partial,
            summary.skipped,
            summary.missing,
            summary.failed,
            if summary.cancelled { " (stopped early)" } else { "" }
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_readable_lines_and_prunes_old_files() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("logs");
        append(
            &dir,
            "Import",
            &["Copied 3 games".into(), "Line one\nline two".into()],
        );
        let text = fs::read_to_string(today_file(&dir)).unwrap();
        assert!(text.contains("[Import] Copied 3 games"));
        assert!(text.contains("[Import]     line two"));
        fs::write(dir.join("romcrate-2000-01-01.log"), "old").unwrap();
        fs::write(dir.join("notes.txt"), "keep").unwrap();
        prune(&dir);
        assert!(!dir.join("romcrate-2000-01-01.log").exists());
        assert!(dir.join("notes.txt").exists());
        assert!(today_file(&dir).exists());
    }

    #[test]
    fn game_results_explain_themselves() {
        let e = Event::GameDone {
            system_id: "snes".into(),
            game: "SONIC.SMC".into(),
            status: GameStatus::Missing,
            matched: None,
            method: None,
            saved: vec![],
            missing: vec![crate::source::ArtKind::Boxart],
            preview: None,
            message: Some("not in the official game lists".into()),
        };
        let s = describe(&e).unwrap();
        assert!(s.starts_with("NOT FOUND: SONIC.SMC"), "{s}");
        assert!(s.contains("not in the official game lists"));
    }
}
