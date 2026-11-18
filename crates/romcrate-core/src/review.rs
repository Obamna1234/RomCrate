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
    // Also no Windows-reserved names (CON, NUL, COM1...) and a sane length.
    crate::paths::safe_component(t.trim())
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
    /// Size and CRC of the picture RomCrate put there, so putting back never
    /// replaces a picture that has been changed since. (Older records: none.)
    #[serde(default)]
    pub ours_size: Option<u64>,
    #[serde(default)]
    pub ours_crc: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: String,
    pub kept: Vec<Kept>,
    pub replaced: usize,
    pub failed: Vec<String>,
    /// The handheld these pictures were on (handheld batches only).
    #[serde(default)]
    pub serial: Option<String>,
}

/// What putting back did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutBack {
    pub restored: usize,
    /// Pictures left as they are, and why.
    pub left: Vec<String>,
}

pub(crate) fn crc_of(p: &Path) -> Option<u32> {
    std::fs::read(p).ok().map(|b| crc32fast::hash(&b))
}

fn new_batch(data: &Path) -> (Batch, PathBuf) {
    // A new folder every time: two batches in the same second never share one
    // (that would overwrite the first batch's kept pictures).
    let base = chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string();
    let root = replaced_dir(data);
    let _ = std::fs::create_dir_all(&root);
    let mut id = base.clone();
    for n in 2.. {
        match std::fs::create_dir(root.join(&id)) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => id = format!("{base}-{n}"),
            _ => break,
        }
    }
    let dir = root.join(&id);
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
                        ours_size: std::fs::metadata(&it.candidate).ok().map(|m| m.len()),
                        ours_crc: crc_of(&it.candidate),
                    });
                }
            }
            // Recorded before anything is replaced, so even a half-done item
            // can be put back.
            b.kept.extend(kept);
            let _ = save_manifest(&dir, &b);
            for t in &it.targets {
                if let Some(p) = t.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::copy(&it.candidate, t)?;
            }
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
pub fn put_back_on_pc(data: &Path, id: &str) -> anyhow::Result<PutBack> {
    let dir = replaced_dir(data).join(safe_name(id));
    let mut b: Batch = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json"))?)?;
    let mut out = PutBack::default();
    let mut remaining = Vec::new();
    for k in std::mem::take(&mut b.kept) {
        if k.on_handheld {
            remaining.push(k);
            continue;
        }
        let place = PathBuf::from(&k.place);
        let changed = place.is_file()
            && k.ours_crc.is_some()
            && (std::fs::metadata(&place).ok().map(|m| m.len()) != k.ours_size
                || crc_of(&place) != k.ours_crc);
        if !k.backup.is_file() {
            out.left
                .push(format!("{}: the kept copy is missing", k.place));
            remaining.push(k);
        } else if changed {
            out.left.push(format!(
                "{}: it was changed after RomCrate replaced it, so it's left as it is",
                k.place
            ));
            remaining.push(k);
        } else {
            if let Some(p) = place.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&k.backup, &place)?;
            out.restored += 1;
        }
    }
    b.kept = remaining;
    if b.kept.is_empty() {
        std::fs::remove_dir_all(&dir)?;
    } else {
        save_manifest(&dir, &b)?;
    }
    Ok(out)
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
        let b = replace_on_pc(&data, std::slice::from_ref(&item));
        assert_eq!(b.replaced, 1, "{:?}", b.failed);
        assert_eq!(std::fs::read(&mine).unwrap(), b"romcrate cover");
        assert_eq!(batches(&data).len(), 1);
        assert_eq!(put_back_on_pc(&data, &b.id).unwrap().restored, 1);
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
    fn put_back_leaves_a_picture_you_changed_since_and_batches_never_share_a_folder() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let media = t.path().join("ROMs/snes/media");
        std::fs::create_dir_all(&media).unwrap();
        let cand = t.path().join("cand.png");
        std::fs::write(&cand, b"romcrate cover").unwrap();
        let item = |name: &str| {
            let p = media.join(name);
            std::fs::write(&p, format!("my {name}")).unwrap();
            ReviewItem {
                system_id: "snes".into(),
                game: name.into(),
                kind: "boxart".into(),
                existing: p.clone(),
                candidate: cand.clone(),
                targets: vec![p],
                remote: vec![],
            }
        };
        // Two replacements in the same second: separate records.
        let b1 = replace_on_pc(&data, &[item("A.png")]);
        let b2 = replace_on_pc(&data, &[item("B.png")]);
        assert_ne!(b1.id, b2.id);
        assert_eq!(batches(&data).len(), 2);
        // You draw your own new A afterwards: putting back leaves it alone.
        std::fs::write(media.join("A.png"), b"newer art I made").unwrap();
        let r = put_back_on_pc(&data, &b1.id).unwrap();
        assert_eq!((r.restored, r.left.len()), (0, 1), "{r:?}");
        assert_eq!(
            std::fs::read(media.join("A.png")).unwrap(),
            b"newer art I made"
        );
        assert_eq!(batches(&data).len(), 2, "still there to try again");
        let r = put_back_on_pc(&data, &b2.id).unwrap();
        assert_eq!(r.restored, 1);
        assert_eq!(std::fs::read(media.join("B.png")).unwrap(), b"my B.png");
    }

    #[test]
    fn a_half_done_replacement_can_still_be_put_back() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let media = t.path().join("ROMs/snes/media");
        std::fs::create_dir_all(&media).unwrap();
        let cand = t.path().join("cand.png");
        std::fs::write(&cand, b"romcrate cover").unwrap();
        let first = media.join("Game.png");
        std::fs::write(&first, b"mine").unwrap();
        // The second target can't be written (its folder is a file).
        std::fs::write(media.join("blocked"), b"").unwrap();
        let item = ReviewItem {
            system_id: "snes".into(),
            game: "Game".into(),
            kind: "boxart".into(),
            existing: first.clone(),
            candidate: cand,
            targets: vec![first.clone(), media.join("blocked/Game.png")],
            remote: vec![],
        };
        let b = replace_on_pc(&data, &[item]);
        assert_eq!(b.failed.len(), 1);
        assert_eq!(std::fs::read(&first).unwrap(), b"romcrate cover");
        assert_eq!(b.kept.len(), 1, "your picture's copy is on record");
        assert_eq!(put_back_on_pc(&data, &b.id).unwrap().restored, 1);
        assert_eq!(std::fs::read(&first).unwrap(), b"mine");
    }

    #[test]
    fn safe_names() {
        assert_eq!(
            safe_name("Ratchet & Clank: Up Your Arsenal?"),
            "Ratchet & Clank_ Up Your Arsenal_"
        );
        assert_eq!(safe_name("../.."), "_");
        assert_eq!(safe_name("CON"), "_CON");
        assert_eq!(safe_name("nul.png"), "_nul.png");
    }
}
