//! A record of every Import & sort run, so it can be undone — and so a run that
//! was cut short (crash, power cut, closed app) can be cleaned up next time.
//!
//! One JSON-lines file per run in RomCrate's own folder. Before a game's files
//! are copied or moved, a `begin` line lists them; once they're all done and
//! checked, a `done` line records what happened to each (moved or copied, with
//! size and modified time). Appending a line per game stays cheap for huge
//! libraries, and the file is flushed to disk after each line.
//!
//! Undo never overwrites anything and never deletes a file that has changed
//! since the import: those are listed as kept, with the reason.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileOp {
    pub from: PathBuf,
    pub to: PathBuf,
    /// Moved (the original is gone from `from`) or copied (it's still there).
    pub moved: bool,
    pub size: u64,
    /// Modified time of `to` right after the import (seconds), to spot later changes.
    pub mtime: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Line {
    #[serde(rename_all = "camelCase")]
    Start {
        id: String,
        moving: bool,
        dest: PathBuf,
    },
    /// About to copy/move these (from, to) pairs for one game.
    #[serde(rename_all = "camelCase")]
    Begin {
        game: String,
        pairs: Vec<(PathBuf, PathBuf)>,
    },
    #[serde(rename_all = "camelCase")]
    Done {
        game: String,
        files: Vec<FileOp>,
    },
    /// The game failed and was rolled back by the import itself.
    #[serde(rename_all = "camelCase")]
    Failed {
        game: String,
    },
    Finished,
    Undone,
}

pub fn mtime(p: &Path) -> u64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Writes one run's record as it happens.
pub struct Recorder {
    file: fs::File,
    pub path: PathBuf,
}

impl Recorder {
    pub fn start(dir: &Path, moving: bool, dest: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(dir)?;
        let id = chrono::Local::now()
            .format("%Y-%m-%d_%H%M%S%.3f")
            .to_string();
        let path = dir.join(format!("{id}.jsonl"));
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)?;
        let mut r = Recorder { file, path };
        r.line(&Line::Start {
            id,
            moving,
            dest: dest.to_path_buf(),
        })?;
        Ok(r)
    }
    fn line(&mut self, l: &Line) -> anyhow::Result<()> {
        let mut s = serde_json::to_string(l)?;
        s.push('\n');
        self.file.write_all(s.as_bytes())?;
        self.file.sync_data()?;
        Ok(())
    }
    pub fn begin(&mut self, game: &str, pairs: &[(PathBuf, PathBuf)]) -> anyhow::Result<()> {
        self.line(&Line::Begin {
            game: game.into(),
            pairs: pairs.to_vec(),
        })
    }
    pub fn done(&mut self, game: &str, files: Vec<FileOp>) -> anyhow::Result<()> {
        self.line(&Line::Done {
            game: game.into(),
            files,
        })
    }
    pub fn failed(&mut self, game: &str) -> anyhow::Result<()> {
        self.line(&Line::Failed { game: game.into() })
    }
    pub fn finish(mut self) -> anyhow::Result<()> {
        self.line(&Line::Finished)
    }
}

/// One past run, as shown in "Earlier imports".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub moving: bool,
    pub dest: PathBuf,
    pub games: usize,
    pub files: usize,
    pub finished: bool,
    pub undone: bool,
    /// A game was cut off midway (no "done" or "failed" line).
    pub interrupted: Option<String>,
}

struct Parsed {
    run: Run,
    done: Vec<FileOp>,
    open: Option<(String, Vec<(PathBuf, PathBuf)>)>,
}

fn parse(path: &Path) -> Option<Parsed> {
    let text = fs::read_to_string(path).ok()?;
    let mut run = Run {
        id: String::new(),
        moving: false,
        dest: PathBuf::new(),
        games: 0,
        files: 0,
        finished: false,
        undone: false,
        interrupted: None,
    };
    let mut done = Vec::new();
    let mut open = None;
    // A torn last line (power cut while writing it) is ignored.
    for l in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Line>(l).ok())
    {
        match l {
            Line::Start { id, moving, dest } => {
                run.id = id;
                run.moving = moving;
                run.dest = dest;
            }
            Line::Begin { game, pairs } => open = Some((game, pairs)),
            Line::Done { files, .. } => {
                open = None;
                run.games += 1;
                run.files += files.len();
                done.extend(files);
            }
            Line::Failed { .. } => open = None,
            Line::Finished => run.finished = true,
            Line::Undone => run.undone = true,
        }
    }
    if run.id.is_empty() {
        return None;
    }
    run.interrupted = open.as_ref().map(|(g, _)| g.clone());
    Some(Parsed { run, done, open })
}

/// Where RomCrate keeps these records (moves with the data folder).
pub fn default_dir() -> PathBuf {
    crate::datadir::keep_dir().join("import-history")
}

/// Past runs, newest first.
pub fn runs(dir: &Path) -> Vec<Run> {
    let mut v: Vec<Run> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| parse(&e.path()).map(|p| p.run))
        .collect();
    v.sort_by(|a, b| b.id.cmp(&a.id));
    v
}

fn run_path(dir: &Path, id: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            && !id.contains(".."),
        "not a RomCrate import"
    );
    let p = dir.join(format!("{id}.jsonl"));
    anyhow::ensure!(
        p.is_file(),
        "that import isn't in RomCrate's history any more"
    );
    Ok(p)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    /// Moved back to where they were.
    pub moved_back: usize,
    /// Copies removed (the originals were never touched).
    pub copies_removed: usize,
    /// Left alone, with the reason.
    pub kept: Vec<String>,
}

fn unchanged(op: &FileOp) -> bool {
    fs::metadata(&op.to).is_ok_and(|m| m.len() == op.size) && mtime(&op.to) == op.mtime
}

/// Put one file back. Never overwrites, never deletes a changed file.
fn put_back(op: &FileOp, r: &mut UndoResult) {
    if !op.to.exists() {
        r.kept.push(format!(
            "{}: not there any more — nothing to do",
            op.to.display()
        ));
        return;
    }
    if !unchanged(op) {
        r.kept.push(format!(
            "{}: changed since the import, so it was left as it is",
            op.to.display()
        ));
        return;
    }
    if op.moved {
        if op.from.exists() {
            r.kept.push(format!(
                "{}: there's already a file where it came from ({}), so it was left as it is",
                op.to.display(),
                op.from.display()
            ));
            return;
        }
        let res = (|| -> anyhow::Result<()> {
            if let Some(d) = op.from.parent() {
                fs::create_dir_all(d)?;
            }
            if fs::rename(&op.to, &op.from).is_err() {
                // Another drive: copy, check, then remove.
                crate::paths::copy_new_checked(&op.to, &op.from)?;
                fs::remove_file(&op.to)?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => r.moved_back += 1,
            Err(e) => r
                .kept
                .push(format!("{}: couldn't move it back ({e})", op.to.display())),
        }
    } else {
        // A copy: removed only while the original is still there.
        if !op.from.exists() {
            r.kept.push(format!(
                "{}: the original ({}) is gone, so this copy is kept",
                op.to.display(),
                op.from.display()
            ));
            return;
        }
        match fs::remove_file(&op.to) {
            Ok(()) => r.copies_removed += 1,
            Err(e) => r
                .kept
                .push(format!("{}: couldn't remove it ({e})", op.to.display())),
        }
    }
}

/// Remove folders the import left empty (only inside its destination).
fn tidy_dirs(dest: &Path, ops: &[FileOp]) {
    let mut dirs: Vec<PathBuf> = ops
        .iter()
        .filter_map(|o| o.to.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    for mut d in dirs {
        while d.starts_with(dest) && d != dest {
            if fs::remove_dir(&d).is_err() {
                break; // not empty (or not ours to remove)
            }
            match d.parent() {
                Some(p) => d = p.to_path_buf(),
                None => break,
            }
        }
    }
}

/// Undo a past run.
pub fn undo(dir: &Path, id: &str) -> anyhow::Result<UndoResult> {
    let path = run_path(dir, id)?;
    let p = parse(&path).ok_or_else(|| anyhow::anyhow!("that import's record can't be read"))?;
    anyhow::ensure!(!p.run.undone, "this import was already undone");
    let mut r = UndoResult::default();
    if p.open.is_some() {
        recover_one(&path, &mut r)?;
    }
    for op in p.done.iter().rev() {
        put_back(op, &mut r);
    }
    tidy_dirs(&p.run.dest, &p.done);
    append(&path, &Line::Undone)?;
    Ok(r)
}

/// Add a line to a finished record. A torn last line (power cut while it was
/// written) is closed first, so the new line can always be read.
fn append(path: &Path, l: &Line) -> anyhow::Result<()> {
    let ends_clean = fs::read(path)?.last().is_none_or(|b| *b == b'\n');
    let mut f = OpenOptions::new().append(true).open(path)?;
    if !ends_clean {
        f.write_all(b"\n")?;
    }
    writeln!(f, "{}", serde_json::to_string(l)?)?;
    f.sync_data()?;
    Ok(())
}

/// Roll back the game that was cut off in a run: remove half-written `.part`
/// files, move back files that were already moved, remove verified copies.
fn recover_one(path: &Path, r: &mut UndoResult) -> anyhow::Result<Option<String>> {
    let Some(p) = parse(path) else {
        return Ok(None);
    };
    let Some((game, pairs)) = p.open else {
        return Ok(None);
    };
    for (from, to) in &pairs {
        let mut part = to.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        if part.is_file() && fs::remove_file(&part).is_ok() {
            r.copies_removed += 1;
        }
        if !to.exists() {
            continue;
        }
        let size = fs::metadata(to).map(|m| m.len()).unwrap_or(0);
        if p.run.moving && !from.exists() {
            // Moved before the cut-off: put it back.
            let op = FileOp {
                from: from.clone(),
                to: to.clone(),
                moved: true,
                size,
                mtime: mtime(to),
            };
            put_back(&op, r);
        } else if from.exists()
            && fs::metadata(from).is_ok_and(|m| m.len() == size)
            && crate::paths::same_contents(from, to).unwrap_or(false)
        {
            // A finished copy of a game that didn't finish: remove it.
            let op = FileOp {
                from: from.clone(),
                to: to.clone(),
                moved: false,
                size,
                mtime: mtime(to),
            };
            put_back(&op, r);
        }
    }
    append(path, &Line::Failed { game: game.clone() })?;
    Ok(Some(game))
}

/// After a crash or power cut: roll back any game that was cut off midway, in
/// every run. Returns what was done, for the log and a message.
pub fn recover(dir: &Path) -> Vec<(String, UndoResult)> {
    let mut out = Vec::new();
    for run in runs(dir)
        .into_iter()
        .filter(|r| r.interrupted.is_some() && !r.undone)
    {
        let Ok(path) = run_path(dir, &run.id) else {
            continue;
        };
        let mut r = UndoResult::default();
        if let Ok(Some(game)) = recover_one(&path, &mut r) {
            out.push((game, r));
        }
    }
    out
}
