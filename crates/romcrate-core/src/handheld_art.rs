//! Artwork for games that are already on the handheld (not on this PC).
//!
//! RomCrate mirrors the handheld's game folders into a hidden staging folder on
//! this PC — tiny stand-ins with the same names, never the games themselves —
//! runs the normal artwork step on that, then copies only the new pictures and
//! launcher files back over USB. Art already on the handheld is stood in for
//! too, so it's kept (or, in "ask me" mode, offered next to RomCrate's version).
//! Nothing on the handheld is overwritten unless you choose RomCrate's picture,
//! and then your old one is kept on this PC first.

use crate::adb::{Adb, PlannedFile, SendPlan, Volume, VolumeNeed};
use crate::review::{self, Batch, Kept, ReviewItem};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Text files RomCrate reads for real (so it never "creates" one that exists).
const REAL_COPY: &[&str] = &["metadata.pegasus.txt", "gamelist.xml"];
/// Stand-in content for a game file. Never sent back.
const STAND_IN: &[u8] = b"R";

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Staging {
    /// The hidden folder on this PC.
    pub root: PathBuf,
    /// (staged ROMs folder, ROMs folder on the handheld, volume path)
    pub roots: Vec<(PathBuf, String, String)>,
    /// Files that came from the handheld (stand-ins and real copies), with
    /// their size on the handheld. Only files not in here are sent back.
    pub from_device: BTreeMap<PathBuf, u64>,
    pub games: usize,
    pub consoles: Vec<String>,
}

impl Staging {
    pub fn local_roots(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|r| r.0.clone()).collect()
    }
    /// The handheld path for a staged file.
    pub fn remote_of(&self, local: &Path) -> Option<(String, String)> {
        let resolved = local
            .parent()
            .and_then(|p| fs::canonicalize(p).ok())
            .zip(local.file_name())
            .map(|(p, n)| p.join(n));
        self.roots.iter().find_map(|(l, r, vol)| {
            let rel = local
                .strip_prefix(l)
                .ok()
                .or_else(|| resolved.as_deref().and_then(|x| x.strip_prefix(l).ok()))?;
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            Some((format!("{r}/{rel}"), vol.clone()))
        })
    }
}

/// Mirror the handheld's game folders (every storage with a ROMs folder).
/// `list(dir)` lists files under a handheld folder; `pull(remote, local)` copies one here.
pub fn stage(
    serial: &str,
    volumes: &[Volume],
    cache: &Path,
    list: &mut dyn FnMut(&str) -> HashMap<String, u64>,
    pull: &mut dyn FnMut(&str, &Path) -> anyhow::Result<()>,
) -> anyhow::Result<Staging> {
    let root = cache.join("handheld").join(review::safe_name(serial));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    let mut st = Staging {
        root: root.clone(),
        roots: vec![],
        from_device: BTreeMap::new(),
        games: 0,
        consoles: vec![],
    };
    for v in volumes {
        let Some(dir) = &v.roms_dir else { continue };
        let remote_root = format!("{}/{}", v.path, dir);
        let local_root = root
            .join(review::safe_name(&v.label))
            .join(review::safe_name(dir));
        fs::create_dir_all(&local_root)?;
        // The resolved path (on macOS /var is a link to /private/var), so it
        // matches the paths the artwork step reports back.
        let local_root = fs::canonicalize(&local_root)?;
        let files = list(&remote_root);
        let mut any = false;
        for (path, size) in files {
            let Some(rel) = path.strip_prefix(&format!("{remote_root}/")) else {
                continue;
            };
            let parts: Vec<&str> = rel.split('/').collect();
            // Names come from the handheld: only mirror ones that are already
            // plain, safe file names here (no "..", "\\", ":", drive letters,
            // control characters, Windows-reserved names). Anything else is
            // skipped, never renamed, so the way back stays exact.
            if parts.len() < 2
                || parts
                    .iter()
                    .any(|p| p.is_empty() || crate::paths::safe_component(p) != *p)
            {
                continue;
            }
            let Some(sys) = crate::systems::detect_from_folder(parts[0]) else {
                continue;
            };
            let name = parts[parts.len() - 1];
            let local = parts.iter().fold(local_root.clone(), |p, c| p.join(c));
            if !crate::paths::is_within(&local_root, &local) {
                continue;
            }
            if let Some(d) = local.parent() {
                fs::create_dir_all(d)?;
            }
            let ext = name
                .rsplit_once('.')
                .map(|(_, e)| e.to_lowercase())
                .unwrap_or_default();
            if REAL_COPY.iter().any(|r| r.eq_ignore_ascii_case(name)) {
                pull(&path, &local)?;
            } else if parts.len() == 2 && sys.extensions.contains(&ext.as_str()) {
                fs::write(&local, STAND_IN)?;
                st.games += 1;
                if !st.consoles.contains(&sys.id.to_string()) {
                    st.consoles.push(sys.id.to_string());
                }
            } else {
                // Existing artwork and anything else: an empty stand-in, so the
                // artwork step sees it's there.
                fs::write(&local, b"")?;
            }
            st.from_device.insert(local, size);
            any = true;
        }
        if any {
            st.roots.push((local_root, remote_root, v.path.clone()));
        }
    }
    Ok(st)
}

/// What to copy back: only files the artwork step made. A launcher file the
/// handheld already has is never replaced.
pub fn plan_back(st: &Staging, volumes: &[Volume]) -> SendPlan {
    let mut plan = SendPlan::default();
    let mut need: BTreeMap<String, u64> = BTreeMap::new();
    let mut files = Vec::new();
    for (l, _, _) in &st.roots {
        walk(l, &mut files);
    }
    for f in files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.ends_with(".bak") || name.ends_with(".part") {
            continue;
        }
        let Some((remote, vol)) = st.remote_of(&f) else {
            continue;
        };
        if let Some(&size) = st.from_device.get(&f) {
            // Came from the handheld. A real launcher file that RomCrate changed
            // stays as it is on the handheld (same rule as Send).
            let now = fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            if REAL_COPY.iter().any(|r| *r == name) && now != size {
                plan.notes.push(format!(
                    "Kept the {name} already on the handheld ({remote})"
                ));
            } else {
                plan.already_there += 1;
            }
            continue;
        }
        let bytes = fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        *need.entry(vol).or_default() += bytes;
        plan.files.push(PlannedFile {
            local: f,
            remote,
            bytes,
        });
    }
    for (path, bytes) in need {
        if let Some(v) = volumes.iter().find(|v| v.path == path) {
            plan.per_volume.push(VolumeNeed {
                label: v.label.clone(),
                fits: bytes + crate::adb::SPACE_MARGIN <= v.free_bytes,
                volume: path,
                bytes,
                free_bytes: v.free_bytes,
            });
        }
    }
    plan
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// For "ask me": point each offered picture at the handheld, and fetch a copy of
/// the handheld's current picture so you can see both. Items whose handheld
/// picture can't be fetched are dropped (nothing is offered blind).
pub fn prepare_review(
    st: &Staging,
    data: &Path,
    items: Vec<ReviewItem>,
    pull: &mut dyn FnMut(&str, &Path) -> anyhow::Result<()>,
) -> Vec<ReviewItem> {
    let dir = review::review_dir(data).join("on-handheld");
    let mut out = Vec::new();
    for (i, mut it) in items.into_iter().enumerate() {
        let remote: Vec<String> = it
            .targets
            .iter()
            .filter_map(|t| st.remote_of(t).map(|r| r.0))
            .collect();
        if remote.is_empty() {
            continue;
        }
        let name = remote[0].rsplit('/').next().unwrap_or("art.png");
        let copy = dir.join(format!("{i:05}-{}", review::safe_name(name)));
        if pull(&remote[0], &copy).is_err() || !copy.is_file() {
            continue;
        }
        let size = st.from_device.get(&it.targets[0]).copied();
        if size.is_some_and(|s| fs::metadata(&copy).map(|m| m.len()).unwrap_or(0) != s) {
            continue;
        }
        // The same picture as RomCrate's? Nothing to choose.
        let same = match (image::open(&copy), image::open(&it.candidate)) {
            (Ok(a), Ok(b)) => a.to_rgba8() == b.to_rgba8(),
            _ => fs::read(&copy).ok() == fs::read(&it.candidate).ok(),
        };
        if same {
            continue;
        }
        it.existing = copy;
        it.remote = remote;
        it.targets.clear();
        out.push(it);
    }
    out
}

/// Use RomCrate's pictures on the handheld for these items. The handheld's
/// current picture (already copied to this PC) is kept first.
pub fn replace_on_handheld(adb: &Adb, serial: &str, data: &Path, items: &[ReviewItem]) -> Batch {
    let (mut b, dir) = review::handheld_batch(data);
    b.serial = Some(serial.to_string());
    for (i, it) in items.iter().enumerate() {
        let r = (|| -> anyhow::Result<()> {
            anyhow::ensure!(
                it.candidate.is_file(),
                "RomCrate's picture is gone — run artwork again"
            );
            anyhow::ensure!(
                it.existing.is_file(),
                "the copy of your picture is missing, so nothing was changed"
            );
            fs::create_dir_all(&dir)?;
            let name = it
                .existing
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let backup = dir.join(format!("{i:05}-{}", review::safe_name(&name)));
            fs::copy(&it.existing, &backup)?;
            let want = fs::metadata(&it.candidate)?.len();
            // Recorded before anything on the handheld changes, so a half-done
            // item can still be put back.
            for r in &it.remote {
                b.kept.push(Kept {
                    place: r.clone(),
                    on_handheld: true,
                    backup: backup.clone(),
                    ours_size: Some(want),
                    ours_crc: review::crc_of(&it.candidate),
                });
            }
            review::finish_batch(&dir, &b);
            for r in &it.remote {
                adb.push(serial, &it.candidate, r)?;
                let d = r.rsplit_once('/').map(|(d, _)| d).unwrap_or("/");
                let got = adb.list_files(serial, d)?.get(r).copied();
                anyhow::ensure!(
                    got == Some(want),
                    "the picture on the handheld didn't arrive whole"
                );
            }
            Ok(())
        })();
        match r {
            Ok(()) => b.replaced += 1,
            Err(e) => b.failed.push(format!("{} ({}): {e}", it.game, it.kind)),
        }
    }
    review::finish_batch(&dir, &b);
    b
}

/// Put back the handheld pictures from a batch.
pub fn put_back_on_handheld(
    adb: &Adb,
    serial: &str,
    data: &Path,
    id: &str,
) -> anyhow::Result<review::PutBack> {
    let dir = review::replaced_dir(data).join(review::safe_name(id));
    let mut b: Batch = serde_json::from_str(&fs::read_to_string(dir.join("manifest.json"))?)?;
    if let Some(s) = b.serial.as_deref().filter(|s| *s != serial) {
        anyhow::bail!(
            "these pictures came from another handheld ({s}) — connect that one to put them back"
        );
    }
    let mut out = review::PutBack::default();
    let mut remaining = Vec::new();
    let mut done = HashSet::new();
    let mut listed: HashMap<String, HashMap<String, u64>> = HashMap::new();
    for k in std::mem::take(&mut b.kept) {
        if !k.on_handheld {
            remaining.push(k);
            continue;
        }
        if !done.insert(k.place.clone()) {
            continue;
        }
        let d = k
            .place
            .rsplit_once('/')
            .map(|(d, _)| d)
            .unwrap_or("/")
            .to_string();
        if !listed.contains_key(&d) {
            listed.insert(d.clone(), adb.list_files(serial, &d)?);
        }
        let now = listed[&d].get(&k.place).copied();
        if !k.backup.is_file() {
            out.left
                .push(format!("{}: the kept copy is missing", k.place));
            remaining.push(k);
        } else if now.is_some() && k.ours_size.is_some() && now != k.ours_size {
            out.left.push(format!(
                "{}: it was changed after RomCrate replaced it, so it's left as it is",
                k.place
            ));
            remaining.push(k);
        } else {
            adb.push(serial, &k.backup, &k.place)?;
            out.restored += 1;
        }
    }
    b.kept = remaining;
    if b.kept.is_empty() {
        fs::remove_dir_all(&dir)?;
    } else {
        review::finish_batch(&dir, &b);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(path: &str, label: &str, roms: &str) -> Volume {
        Volume {
            path: path.into(),
            label: label.into(),
            size_bytes: 1 << 34,
            free_bytes: 1 << 33,
            roms_dir: Some(roms.into()),
            console_folders: vec![],
        }
    }

    #[test]
    fn handheld_names_can_never_write_outside_the_staging_folder() {
        let t = tempfile::tempdir().unwrap();
        let cache = t.path().join("cache");
        let sd = "/storage/1A2B-3C4D/ROMs";
        let bad = [
            "snes/..\\..\\..\\..\\evil.sfc",
            "snes/media/box2dfront/..\\..\\..\\..\\..\\evil.png",
            "snes/C:evil.sfc",
            "snes/C:\\evil.sfc",
            "snes/bad\u{7}name.sfc",
            "snes/CON.sfc",
            "snes/trailing. ",
            "../../evil/x.sfc",
            "snes/../../../evil.sfc",
        ];
        let mut remote: HashMap<String, u64> = HashMap::new();
        for b in bad {
            remote.insert(format!("{sd}/{b}"), 3);
        }
        remote.insert(format!("{sd}/snes/Good Game.sfc"), 3);
        remote.insert(format!("{sd}/snes/metadata.pegasus.txt"), 3);
        let vols = vec![vol("/storage/1A2B-3C4D", "SD", "ROMs")];
        let mut pulled = vec![];
        let st = stage(
            "../../FAKE\\0001",
            &vols,
            &cache,
            &mut |d| {
                remote
                    .iter()
                    .filter(|(k, _)| k.starts_with(d))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect()
            },
            &mut |r, local| {
                pulled.push((r.to_string(), local.to_path_buf()));
                fs::write(local, "x")?;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(st.games, 1, "only the plain-named game");
        let root = fs::canonicalize(&st.root).unwrap();
        for f in st.from_device.keys() {
            assert!(f.starts_with(&root), "{} escaped", f.display());
        }
        for (_, l) in &pulled {
            assert!(l.starts_with(&root));
        }
        // Nothing at all landed outside the cache folder.
        let mut all = vec![];
        walk(t.path(), &mut all);
        for f in all {
            assert!(f.starts_with(&cache), "{} written outside", f.display());
        }
        assert_eq!(st.from_device.len(), 2, "{:?}", st.from_device.keys());
    }

    #[test]
    fn stages_stand_ins_and_sends_back_only_new_files() {
        let t = tempfile::tempdir().unwrap();
        let sd = "/storage/1A2B-3C4D/ROMs";
        let mut remote: HashMap<String, u64> = HashMap::new();
        remote.insert(format!("{sd}/snes/Chrono Trigger (USA).sfc"), 4_194_304);
        remote.insert(
            format!("{sd}/snes/media/box2dfront/Chrono Trigger (USA).png"),
            90_000,
        );
        remote.insert(format!("{sd}/snes/metadata.pegasus.txt"), 17); // "collection: SNES\n"
        remote.insert(format!("{sd}/Notes/readme.txt"), 5);
        let vols = vec![vol("/storage/1A2B-3C4D", "SD card (1A2B-3C4D)", "ROMs")];
        let st = stage(
            "FAKE0001",
            &vols,
            t.path(),
            &mut |d| {
                remote
                    .iter()
                    .filter(|(k, _)| k.starts_with(d))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect()
            },
            &mut |_, local| {
                fs::write(local, "collection: SNES\n")?;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(st.games, 1);
        assert_eq!(st.consoles, vec!["snes"]);
        let game = st.roots[0].0.join("snes/Chrono Trigger (USA).sfc");
        assert_eq!(
            fs::read(&game).unwrap(),
            STAND_IN,
            "a stand-in, not the game"
        );
        assert!(
            !st.roots[0].0.join("Notes").exists(),
            "only console folders"
        );

        // The artwork step adds a screenshot, and edits nothing else.
        let snap = st.roots[0]
            .0
            .join("snes/media/screenshot/Chrono Trigger (USA).png");
        fs::create_dir_all(snap.parent().unwrap()).unwrap();
        fs::write(&snap, b"PNG").unwrap();
        let plan = plan_back(&st, &vols);
        assert_eq!(plan.files.len(), 1, "{:?}", plan.files);
        assert_eq!(
            plan.files[0].remote,
            format!("{sd}/snes/media/screenshot/Chrono Trigger (USA).png")
        );
        assert_eq!(
            plan.already_there, 3,
            "game, cover and metadata stay as they are"
        );
        assert!(plan.per_volume[0].fits);

        // A changed launcher file is never sent back.
        fs::write(
            st.roots[0].0.join("snes/metadata.pegasus.txt"),
            "collection: SNES\nlaunch: x\n",
        )
        .unwrap();
        let plan = plan_back(&st, &vols);
        assert_eq!(plan.files.len(), 1);
        assert!(plan.notes[0].contains("Kept the metadata.pegasus.txt"));
    }
}
