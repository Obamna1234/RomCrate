//! Where RomCrate keeps its own files: the activity log and downloaded
//! databases/indexes (the "cache"). By default that's the system's usual place
//! for app data (on Windows `%LOCALAPPDATA%` / `%LOCALAPPDATA%\..\Cache` style
//! folders on drive C:). The user can pick another folder; the choice itself is
//! kept in a tiny settings file in the system's config folder, because RomCrate
//! has to find it before it knows where anything else is.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// Custom folder for log + downloads. `None` = system default.
    #[serde(default)]
    pub data_dir: Option<PathBuf>,
    /// Write the activity log to disk (default on).
    #[serde(default = "yes")]
    pub log_enabled: bool,
}

fn yes() -> bool {
    true
}

fn settings_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("RomCrate")
        .join("location.json")
}

/// The current choice (defaults when nothing was chosen or the file is unreadable).
pub fn location() -> Location {
    location_from(&settings_file())
}

fn location_from(file: &Path) -> Location {
    fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Location {
            data_dir: None,
            log_enabled: true,
        })
}

fn save(file: &Path, loc: &Location) -> anyhow::Result<()> {
    if let Some(p) = file.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(file, serde_json::to_vec_pretty(loc)?)?;
    Ok(())
}

pub fn default_log_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("RomCrate")
        .join("logs")
}

pub fn default_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("RomCrate")
}

/// RomCrate's own kept files (pictures you replaced, pictures to compare):
/// next to the log, never in the downloads cache that "Clear cache" empties.
fn keep_dir_of(log: &Path) -> PathBuf {
    log.parent()
        .map(|p| p.join("kept"))
        .unwrap_or_else(|| log.join("kept"))
}

pub fn keep_dir() -> PathBuf {
    keep_dir_of(&log_dir())
}

fn dirs_for(loc: &Location) -> (PathBuf, PathBuf) {
    match &loc.data_dir {
        Some(d) => (d.join("logs"), d.join("downloads")),
        None => (default_log_dir(), default_cache_dir()),
    }
}

pub fn log_dir() -> PathBuf {
    dirs_for(&location()).0
}

pub fn cache_dir() -> PathBuf {
    dirs_for(&location()).1
}

/// Switch to a new folder (or back to the default with `None`), moving the
/// existing log and downloads across. Returns what was moved, for the log.
pub fn set_data_dir(new: Option<PathBuf>) -> anyhow::Result<Vec<String>> {
    set_data_dir_in(&settings_file(), new)
}

fn set_data_dir_in(file: &Path, new: Option<PathBuf>) -> anyhow::Result<Vec<String>> {
    let mut loc = location_from(file);
    let new = match new {
        Some(p) if p.as_os_str().is_empty() => None,
        Some(p) => Some(std::path::absolute(&p)?),
        None => None,
    };
    let (old_log, old_cache) = dirs_for(&loc);
    let target = Location {
        data_dir: new.clone(),
        ..loc.clone()
    };
    let (new_log, new_cache) = dirs_for(&target);
    if let Some(d) = &new {
        fs::create_dir_all(d).map_err(|e| anyhow::anyhow!("can't use {}: {e}", d.display()))?;
        // Writable?
        let probe = d.join(".romcrate-write-test");
        fs::write(&probe, b"ok")
            .map_err(|e| anyhow::anyhow!("can't write to {}: {e}", d.display()))?;
        let _ = fs::remove_file(probe);
    }
    let mut notes = Vec::new();
    let (old_keep, new_keep) = (keep_dir_of(&old_log), keep_dir_of(&new_log));
    // On Windows the default downloads folder also holds the log and kept
    // folders (%LOCALAPPDATA%\RomCrate): those move on their own, never as
    // part of the downloads.
    let ours = [old_log.clone(), old_keep.clone()];
    for (what, from, to, skip) in [
        ("log", &old_log, &new_log, &[][..]),
        ("downloads", &old_cache, &new_cache, &ours[..]),
        ("kept picture", &old_keep, &new_keep, &[][..]),
    ] {
        if from == to || !from.exists() {
            continue;
        }
        if to.starts_with(from) && !skip.iter().any(|s| to.starts_with(s)) {
            anyhow::bail!("the new folder can't be inside the old {what} folder");
        }
        let (n, left) = move_tree(from, to, skip)?;
        notes.push(format!(
            "Moved {n} {what} file(s) from {} to {}",
            from.display(),
            to.display()
        ));
        for l in left {
            notes.push(format!(
                "Left {} where it was: a different file with that name is already in the new folder",
                l.display()
            ));
        }
    }
    loc.data_dir = new;
    save(file, &loc)?;
    Ok(notes)
}

pub fn set_log_enabled(on: bool) -> anyhow::Result<()> {
    let file = settings_file();
    let mut loc = location_from(&file);
    loc.log_enabled = on;
    save(&file, &loc)
}

/// Move a folder's contents (rename when possible, otherwise copy, check, delete).
/// `skip` = RomCrate's other folders that may sit inside `from`; they stay.
/// A file already at the destination is never overwritten: an identical one
/// means the old copy can go; a different one means the old file stays where
/// it is and is returned, so nothing is ever lost.
fn move_tree(from: &Path, to: &Path, skip: &[PathBuf]) -> anyhow::Result<(usize, Vec<PathBuf>)> {
    let nested = skip.iter().any(|s| s.starts_with(from) && s.exists());
    if !to.exists() && !nested {
        if let Some(p) = to.parent() {
            fs::create_dir_all(p)?;
        }
        if fs::rename(from, to).is_ok() {
            return Ok((count(to), vec![]));
        }
    }
    let (mut n, mut left) = (0, Vec::new());
    move_files(from, to, skip, &mut n, &mut left)?;
    Ok((n, left))
}

fn move_files(
    from: &Path,
    to: &Path,
    skip: &[PathBuf],
    n: &mut usize,
    left: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)?.flatten() {
        let p = e.path();
        if skip.contains(&p) {
            continue;
        }
        let ft = e.file_type()?;
        let dest = to.join(e.file_name());
        if ft.is_dir() {
            move_files(&p, &dest, skip, n, left)?;
        } else if ft.is_file() {
            if dest.exists() {
                if crate::paths::same_contents(&p, &dest).unwrap_or(false) {
                    fs::remove_file(&p)?;
                } else {
                    left.push(p);
                }
                continue;
            }
            if fs::rename(&p, &dest).is_err() {
                crate::paths::copy_new_checked(&p, &dest)?;
                fs::remove_file(&p)?;
            }
            *n += 1;
        }
    }
    let _ = fs::remove_dir(from); // only goes if it's now empty
    Ok(())
}

/// Empty the downloads folder ("Clear cache") — never the log or kept folders,
/// which on Windows sit inside it by default.
pub fn clear_downloads() -> anyhow::Result<()> {
    clear_dir_except(&cache_dir(), &[log_dir(), keep_dir()])
}

fn clear_dir_except(dir: &Path, keep: &[PathBuf]) -> anyhow::Result<()> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Ok(());
    };
    for e in rd.flatten() {
        let p = e.path();
        if keep.iter().any(|k| *k == p || k.starts_with(&p)) {
            if p.is_dir() && !keep.contains(&p) {
                clear_dir_except(&p, keep)?;
            }
            continue;
        }
        if e.file_type()?.is_dir() {
            fs::remove_dir_all(&p)?;
        } else {
            fs::remove_file(&p)?;
        }
    }
    Ok(())
}

fn count(p: &Path) -> usize {
    fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .map(|e| {
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        count(&e.path())
                    } else {
                        1
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' default layout: downloads, log and kept all in one folder.
    fn windows_like(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let cache = root.join("RomCrate");
        let (log, keep) = (cache.join("logs"), cache.join("kept"));
        for (p, t) in [
            (cache.join("index/snes.json"), "{}"),
            (cache.join("tools/adb"), "adb"),
            (log.join("romcrate-2026-11-19.log"), "log"),
            (keep.join("replaced-artwork/1/manifest.json"), "{}"),
            (keep.join("import-history/1.jsonl"), "{}"),
        ] {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, t).unwrap();
        }
        (cache, log, keep)
    }

    #[test]
    fn clear_cache_never_touches_the_log_or_kept_pictures() {
        let t = tempfile::tempdir().unwrap();
        let (cache, log, keep) = windows_like(t.path());
        clear_dir_except(&cache, &[log.clone(), keep.clone()]).unwrap();
        assert!(
            !cache.join("index").exists() && !cache.join("tools").exists(),
            "downloads cleared"
        );
        assert!(log.join("romcrate-2026-11-19.log").exists(), "log kept");
        assert!(
            keep.join("replaced-artwork/1/manifest.json").exists(),
            "your old pictures kept"
        );
        assert!(
            keep.join("import-history/1.jsonl").exists(),
            "import undo records kept"
        );
    }

    #[test]
    fn moving_downloads_leaves_our_other_folders_and_never_deletes_unmoved_files() {
        let t = tempfile::tempdir().unwrap();
        let (cache, log, keep) = windows_like(t.path());
        let to = t.path().join("D/downloads");
        // A different file with the same name is already there.
        fs::create_dir_all(to.join("index")).unwrap();
        fs::write(to.join("index/snes.json"), "something else").unwrap();
        let (n, left) = move_tree(&cache, &to, &[log.clone(), keep.clone()]).unwrap();
        assert_eq!(n, 1, "tools/adb moved");
        assert_eq!(
            left,
            vec![cache.join("index/snes.json")],
            "the clashing file stays"
        );
        assert_eq!(
            fs::read_to_string(cache.join("index/snes.json")).unwrap(),
            "{}",
            "not deleted"
        );
        assert_eq!(
            fs::read_to_string(to.join("index/snes.json")).unwrap(),
            "something else",
            "not overwritten"
        );
        assert!(
            keep.join("import-history/1.jsonl").exists() && log.exists(),
            "kept and log not swept along"
        );
        assert!(!to.join("kept").exists() && !to.join("logs").exists());
    }

    #[test]
    fn choosing_a_folder_moves_the_log_and_downloads() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("cfg/location.json");
        let a = t.path().join("A");
        // Start in A (saved directly: moving from the real default folder would
        // pick up this computer's own RomCrate files).
        save(
            &file,
            &Location {
                data_dir: Some(a.clone()),
                log_enabled: true,
            },
        )
        .unwrap();
        fs::create_dir_all(a.join("logs")).unwrap();
        fs::write(a.join("logs/romcrate-2026-11-16.log"), "hello").unwrap();
        fs::create_dir_all(a.join("downloads/libretro")).unwrap();
        fs::write(a.join("downloads/libretro/snes.json"), "{}").unwrap();
        fs::create_dir_all(a.join("kept/replaced-artwork")).unwrap();
        fs::write(a.join("kept/replaced-artwork/old.png"), "png").unwrap();
        // Move to B.
        let b = t.path().join("B");
        let notes = set_data_dir_in(&file, Some(b.clone())).unwrap();
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(
            b.join("kept/replaced-artwork/old.png").exists(),
            "kept pictures move too"
        );
        assert_eq!(
            fs::read_to_string(b.join("logs/romcrate-2026-11-16.log")).unwrap(),
            "hello"
        );
        assert!(b.join("downloads/libretro/snes.json").exists());
        assert!(!a.join("logs").exists() && !a.join("downloads").exists());
        assert_eq!(location_from(&file).data_dir.as_deref(), Some(b.as_path()));
        assert!(location_from(&file).log_enabled);
    }

    #[test]
    fn refuses_a_folder_it_cannot_use() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("location.json");
        let not_a_dir = t.path().join("file.txt");
        fs::write(&not_a_dir, "x").unwrap();
        assert!(set_data_dir_in(&file, Some(not_a_dir)).is_err());
        assert_eq!(
            location_from(&file).data_dir,
            None,
            "nothing saved on failure"
        );
    }
}
