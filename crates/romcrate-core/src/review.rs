//! "Keep yours or use RomCrate's": when a game already has artwork, RomCrate can
//! save the picture it found next to yours (in its own data folder) and let
//! you choose, game by game. Nothing of yours changes until you choose.
//!
//! Replacing always keeps your old picture first, in
//! `<RomCrate data>/replaced-artwork/<when>/`, with a list of where each one
//! came from, so "Put them back" can restore them exactly.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub system_id: String,
    pub game: String,
    /// boxart, snap, logo…
    pub kind: String,
    /// Your current picture (on this PC; for the handheld, a copy pulled from it).
    pub existing: PathBuf,
    /// RomCrate's version, waiting in the review folder.
    pub candidate: PathBuf,
    /// Every place on this PC the picture goes (some launchers use two).
    #[serde(default)]
    pub targets: Vec<PathBuf>,
    /// For artwork on the handheld: where it goes there.
    #[serde(default)]
    pub remote: Vec<String>,
}

/// A file name safe on every OS.
pub fn safe_name(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.()&'".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let t = t.trim().trim_matches('.').to_string();
    if t.is_empty() {
        "_".into()
    } else {
        t
    }
}

/// Where RomCrate keeps candidates while you decide.
pub fn review_dir(data: &Path) -> PathBuf {
    data.join("review")
}

/// Where replaced pictures are kept.
pub fn replaced_dir(data: &Path) -> PathBuf {
    data.join("replaced-artwork")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kept {
    /// Where the picture was (a path on this PC, or on the handheld).
    pub place: String,
    pub on_handheld: bool,
    /// The copy RomCrate kept.
    pub backup: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: String,
    pub kept: Vec<Kept>,
    pub replaced: usize,
    pub failed: Vec<String>,
}

fn new_batch(data: &Path) -> (Batch, PathBuf) {
    let id = chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string();
    let dir = replaced_dir(data).join(&id);
    (
        Batch {
            id,
            ..Default::default()
        },
        dir,
    )
}

fn save_manifest(dir: &Path, b: &Batch) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(b)?)?;
    Ok(())
}

/// Use RomCrate's pictures for these items (artwork on this PC). Your old
/// pictures are kept first; an item whose old picture can't be kept is skipped.
pub fn replace_on_pc(data: &Path, items: &[ReviewItem]) -> Batch {
    let (mut b, dir) = new_batch(data);
    for (i, it) in items.iter().enumerate() {
        let r = (|| -> anyhow::Result<()> {
            anyhow::ensure!(
                it.candidate.is_file(),
                "RomCrate's picture is gone — run artwork again"
            );
            let mut kept = Vec::new();
            for (j, t) in it.targets.iter().enumerate() {
                if t.is_file() {
                    let name = t
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let backup = dir.join(format!("{i:05}-{j}-{}", safe_name(&name)));
                    std::fs::create_dir_all(&dir)?;
                    std::fs::copy(t, &backup)?;
                    anyhow::ensure!(
                        std::fs::metadata(&backup)?.len() == std::fs::metadata(t)?.len(),
                        "couldn't keep a copy of your picture"
                    );
                    kept.push(Kept {
                        place: t.to_string_lossy().into_owned(),
                        on_handheld: false,
                        backup,
                    });
                }
            }
            for t in &it.targets {
                if let Some(p) = t.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::copy(&it.candidate, t)?;
            }
            b.kept.extend(kept);
            Ok(())
        })();
        match r {
            Ok(()) => b.replaced += 1,
            Err(e) => b.failed.push(format!("{} ({}): {e}", it.game, it.kind)),
        }
    }
    let _ = save_manifest(&dir, &b);
    b
}

/// Earlier replacements you can put back, newest first.
pub fn batches(data: &Path) -> Vec<Batch> {
    let mut out: Vec<Batch> = std::fs::read_dir(replaced_dir(data))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            serde_json::from_str(&std::fs::read_to_string(e.path().join("manifest.json")).ok()?)
                .ok()
        })
        .filter(|b: &Batch| !b.kept.is_empty())
        .collect();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

/// Put back the pictures from a batch that were on this PC. (Handheld ones are
/// put back by `handheld_art::put_back`.) The batch is removed once it's done.
pub fn put_back_on_pc(data: &Path, id: &str) -> anyhow::Result<usize> {
    let dir = replaced_dir(data).join(safe_name(id));
    let b: Batch = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json"))?)?;
    let mut n = 0;
    for k in b.kept.iter().filter(|k| !k.on_handheld) {
        std::fs::copy(&k.backup, &k.place)?;
        n += 1;
    }
    if b.kept.iter().all(|k| !k.on_handheld) {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(n)
}

/// Record handheld replacements in a batch (used by `handheld_art`).
pub(crate) fn handheld_batch(data: &Path) -> (Batch, PathBuf) {
    new_batch(data)
}
pub(crate) fn finish_batch(dir: &Path, b: &Batch) {
    let _ = save_manifest(dir, b);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_keeps_old_and_puts_back() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let media = t.path().join("ROMs/snes/media/box2dfront");
        std::fs::create_dir_all(&media).unwrap();
        let mine = media.join("Mario.png");
        std::fs::write(&mine, b"my own cover").unwrap();
        let cand = review_dir(&data).join("snes/Mario/boxart.png");
        std::fs::create_dir_all(cand.parent().unwrap()).unwrap();
        std::fs::write(&cand, b"romcrate cover").unwrap();
        let item = ReviewItem {
            system_id: "snes".into(),
            game: "Mario".into(),
            kind: "boxart".into(),
            existing: mine.clone(),
            candidate: cand,
            targets: vec![mine.clone()],
            remote: vec![],
        };
        let b = replace_on_pc(&data, &[item.clone()]);
        assert_eq!(b.replaced, 1, "{:?}", b.failed);
        assert_eq!(std::fs::read(&mine).unwrap(), b"romcrate cover");
        assert_eq!(batches(&data).len(), 1);
        assert_eq!(put_back_on_pc(&data, &b.id).unwrap(), 1);
        assert_eq!(std::fs::read(&mine).unwrap(), b"my own cover");
        assert!(batches(&data).is_empty());

        // RomCrate's picture gone: nothing of yours is touched.
        let mut gone = item;
        gone.candidate = t.path().join("nope.png");
        let b = replace_on_pc(&data, &[gone]);
        assert_eq!((b.replaced, b.failed.len()), (0, 1));
        assert_eq!(std::fs::read(&mine).unwrap(), b"my own cover");
    }

    #[test]
    fn safe_names() {
        assert_eq!(
            safe_name("Ratchet & Clank: Up Your Arsenal?"),
            "Ratchet & Clank_ Up Your Arsenal_"
        );
        assert_eq!(safe_name("../.."), "_");
    }
}
