//! Sending games and artwork to an Android handheld over USB, using Android's
//! own USB tool (`adb`, part of Google's "platform-tools").
//!
//! * RomCrate never ships adb. On first use it downloads platform-tools from
//!   Google (dl.google.com) and checks the file against the SHA-1 checksum in
//!   Google's own package list before unpacking it. An adb already installed on
//!   the computer is used if there is one.
//! * Nothing on the handheld is overwritten or deleted. Files already there
//!   with the same size are skipped; a different file with the same name is
//!   left alone and reported; an existing `metadata.pegasus.txt` is kept.
//! * Every game file is checked after copying (MD5 on the PC vs MD5 on the
//!   handheld). A copy that doesn't match is removed again and reported.

use crate::archive::human;
use crate::net::SafeClient;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO_XML: &str = "https://dl.google.com/android/repository/repository2-3.xml";
const REPO_BASE: &str = "https://dl.google.com/android/repository/";
/// Platform-tools is ~7–15 MB; refuse anything absurd.
const MAX_TOOLS_ZIP: usize = 200 * 1024 * 1024;
/// Keep this much free on the handheld after copying.
pub const SPACE_MARGIN: u64 = 200 * 1024 * 1024;
/// Files bigger than this are checked with MD5 after copying (games); smaller
/// ones (artwork, text) by size.
const HASH_FROM: u64 = 1024 * 1024;

pub const TERMS_URL: &str = "https://developer.android.com/tools/releases/platform-tools";

// ---------------------------------------------------------------- finding adb

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "adb.exe"
    } else {
        "adb"
    }
}

/// Where RomCrate keeps its own copy of platform-tools.
pub fn tools_dir() -> PathBuf {
    crate::datadir::cache_dir()
        .join("tools")
        .join("platform-tools")
}

#[derive(Debug, Clone)]
pub struct Adb {
    pub exe: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbStatus {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// "romcrate" (downloaded by RomCrate), "system" (already installed) or "env" (ROMCRATE_ADB).
    pub source: Option<String>,
}

impl Adb {
    /// RomCrate's own copy first, then one already installed on the computer.
    /// `ROMCRATE_ADB` overrides both (used by the tests).
    pub fn find() -> Option<(Adb, &'static str)> {
        if let Some(p) = std::env::var_os("ROMCRATE_ADB") {
            return Some((Adb { exe: p.into() }, "env"));
        }
        let own = tools_dir().join(exe_name());
        if own.is_file() {
            return Some((Adb { exe: own }, "romcrate"));
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|d| d.join(exe_name()))
            .find(|p| p.is_file())
            .map(|exe| (Adb { exe }, "system"))
    }

    pub fn status() -> AdbStatus {
        match Adb::find() {
            None => AdbStatus {
                found: false,
                path: None,
                version: None,
                source: None,
            },
            Some((a, src)) => AdbStatus {
                found: true,
                path: Some(a.exe.to_string_lossy().into()),
                version: a
                    .run(&["version"])
                    .ok()
                    .and_then(|v| v.lines().next().map(str::to_string)),
                source: Some(src.into()),
            },
        }
    }

    fn command(&self) -> Command {
        #[allow(unused_mut)] // only changed on Windows
        let mut c = Command::new(&self.exe);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashing up for every call.
            c.creation_flags(0x0800_0000);
        }
        c
    }

    fn run(&self, args: &[&str]) -> anyhow::Result<String> {
        let out = self.command().args(args).output()?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let msg = if err.trim().is_empty() {
                text.trim()
            } else {
                err.trim()
            };
            anyhow::bail!("{}", friendly(msg));
        }
        Ok(text)
    }

    fn shell(&self, serial: &str, script: &str) -> anyhow::Result<String> {
        self.run(&["-s", serial, "shell", script])
    }

    pub fn devices(&self) -> anyhow::Result<Vec<Device>> {
        Ok(parse_devices(&self.run(&["devices", "-l"])?))
    }

    /// Internal storage and any SD cards, with free space and an existing
    /// ROMs folder if there is one.
    pub fn volumes(&self, serial: &str) -> anyhow::Result<Vec<Volume>> {
        let script = r#"for d in /storage/emulated/0 /storage/*; do case "$d" in /storage/emulated|/storage/self|/storage/sdcard*) continue;; esac; [ -d "$d" ] || continue; df -k "$d" 2>/dev/null | tail -n 1 | while read fs size used avail rest; do echo "V|$d|$size|$avail"; done; for r in ROMs roms Roms ROMS Emulation/roms; do [ -d "$d/$r" ] && echo "R|$d|$r"; done; done"#;
        let mut vols = parse_volumes(&self.shell(serial, script)?);
        for v in &mut vols {
            if let Some(r) = &v.roms_dir {
                v.console_folders = self
                    .shell(
                        serial,
                        &format!("ls -1 {} 2>/dev/null", q(&format!("{}/{r}", v.path))),
                    )
                    .map(|t| {
                        t.lines()
                            .map(|l| l.trim().to_string())
                            .filter(|l| !l.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
            }
        }
        Ok(vols)
    }

    /// Which device and system this is (from Android's build properties).
    pub fn identity(&self, serial: &str) -> anyhow::Result<crate::detect::Identity> {
        let out = self.shell(serial, "getprop")?;
        Ok(crate::detect::identify(&crate::detect::parse_props(&out)))
    }

    /// Copy a file from the handheld to this computer.
    pub fn pull(&self, serial: &str, remote: &str, local: &Path) -> anyhow::Result<()> {
        if let Some(p) = local.parent() {
            fs::create_dir_all(p)?;
        }
        let l = local.to_string_lossy();
        self.run(&["-s", serial, "pull", remote, &l]).map(|_| ())
    }

    /// Every file under a folder on the handheld, with its size.
    pub fn list_files(&self, serial: &str, dir: &str) -> anyhow::Result<HashMap<String, u64>> {
        let out = self.shell(
            serial,
            &format!(
                "[ -d {d} ] && find {d} -type f -exec stat -c '%s|%n' {{}} + 2>/dev/null; true",
                d = q(dir)
            ),
        )?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let (s, n) = l.split_once('|')?;
                Some((n.trim_end_matches('\r').to_string(), s.trim().parse().ok()?))
            })
            .collect())
    }

    pub fn push(&self, serial: &str, local: &Path, remote: &str) -> anyhow::Result<()> {
        if let Some(parent) = remote.rsplit_once('/').map(|(p, _)| p) {
            self.shell(serial, &format!("mkdir -p {}", q(parent)))?;
        }
        let local = local.to_string_lossy();
        self.run(&["-s", serial, "push", &local, remote])
            .map(|_| ())
    }

    pub fn md5(&self, serial: &str, remote: &str) -> anyhow::Result<String> {
        let out = self.shell(serial, &format!("md5sum {}", q(remote)))?;
        out.split_whitespace()
            .next()
            .filter(|h| h.len() == 32)
            .map(|h| h.to_lowercase())
            .ok_or_else(|| anyhow::anyhow!("the handheld couldn't check the file"))
    }

    /// Can this computer write to a storage on the handheld? (Some handhelds
    /// don't allow writing to the SD card over USB debugging.)
    pub fn can_write(&self, serial: &str, volume: &str) -> anyhow::Result<()> {
        let probe = format!("{volume}/.romcrate-write-test");
        let out = self.shell(
            serial,
            &format!(
                "touch {p} 2>&1 && rm -f {p} && echo OK; true",
                p = q(&probe)
            ),
        )?;
        anyhow::ensure!(
            out.trim_end().ends_with("OK"),
            "Android doesn't let this computer write to {volume} over USB ({}). Choose Internal storage for those consoles, or put the SD card in a card reader on your PC.",
            out.trim()
        );
        Ok(())
    }

    /// Is anything at this path on the handheld right now?
    pub fn exists(&self, serial: &str, remote: &str) -> anyhow::Result<bool> {
        let out = self.shell(serial, &format!("[ -e {} ] && echo yes; true", q(remote)))?;
        Ok(out.trim() == "yes")
    }

    pub fn remove(&self, serial: &str, remote: &str) -> anyhow::Result<()> {
        self.shell(serial, &format!("rm -f {}", q(remote)))
            .map(|_| ())
    }
}

/// The handheld's files, for Game settings.
pub struct AdbFs<'a> {
    pub adb: &'a Adb,
    pub serial: &'a str,
}

impl crate::gamesettings::DeviceFs for AdbFs<'_> {
    fn read(&mut self, path: &str) -> anyhow::Result<Option<String>> {
        let out = self.adb.shell(
            self.serial,
            &format!(
                "if [ -f {p} ]; then printf __RC_Y__; cat {p}; else printf __RC_N__; fi",
                p = q(path)
            ),
        )?;
        if let Some(t) = out.strip_prefix("__RC_Y__") {
            Ok(Some(t.to_string()))
        } else if out.starts_with("__RC_N__") {
            Ok(None)
        } else {
            anyhow::bail!("unexpected answer from the handheld")
        }
    }
    fn write(&mut self, path: &str, text: &str) -> anyhow::Result<()> {
        let tmp = tempfile::NamedTempFile::new()?;
        fs::write(tmp.path(), text)?;
        self.adb.push(self.serial, tmp.path(), path)
    }
    fn copy(&mut self, from: &str, to: &str) -> anyhow::Result<()> {
        self.adb
            .shell(self.serial, &format!("cp {} {}", q(from), q(to)))
            .map(|_| ())
    }
    fn remove(&mut self, path: &str) -> anyhow::Result<()> {
        self.adb.remove(self.serial, path)
    }
    fn is_dir(&mut self, path: &str) -> bool {
        self.adb
            .shell(self.serial, &format!("[ -d {} ] && echo Y; true", q(path)))
            .map(|o| o.trim() == "Y")
            .unwrap_or(false)
    }
    fn find_file(&mut self, name: &str) -> Option<String> {
        self.adb
            .shell(
                self.serial,
                &format!("find /storage/emulated/0 -maxdepth 6 -type f -name {} 2>/dev/null | head -n 1; true", q(name)),
            )
            .ok()
            .and_then(|o| o.lines().next().map(|l| l.trim().to_string()))
            .filter(|l| !l.is_empty())
    }
    fn installed_packages(&mut self) -> std::collections::HashSet<String> {
        self.adb
            .shell(self.serial, "pm list packages")
            .map(|o| {
                o.lines()
                    .filter_map(|l| l.trim().strip_prefix("package:").map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn stop_app(&mut self, package: &str) {
        let _ = self
            .adb
            .shell(self.serial, &format!("am force-stop {}", q(package)));
    }
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> anyhow::Result<Vec<u8>> {
        use base64::Engine;
        // dd in 4 KB blocks (offsets here are always 64 KB-aligned), base64 so
        // the bytes survive the trip through the shell.
        anyhow::ensure!(
            offset.is_multiple_of(4096) && len.is_multiple_of(4096),
            "unaligned read"
        );
        let out = self.adb.shell(
            self.serial,
            &format!(
                "dd if={} bs=4096 skip={} count={} 2>/dev/null | base64",
                q(path),
                offset / 4096,
                len / 4096
            ),
        )?;
        let clean: String = out.chars().filter(|c| !c.is_whitespace()).collect();
        Ok(base64::engine::general_purpose::STANDARD.decode(clean)?)
    }
    fn list_files(&mut self, dir: &str) -> Vec<String> {
        self.adb
            .shell(
                self.serial,
                &format!("find {} -maxdepth 1 -type f 2>/dev/null; true", q(dir)),
            )
            .map(|o| {
                o.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Quote for the handheld's shell.
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// adb's own messages, in words a person can act on.
fn friendly(msg: &str) -> String {
    let m = msg.to_lowercase();
    if m.contains("unauthorized") {
        "the handheld hasn't allowed this computer yet — unlock it and tap \"Allow\" on the USB debugging message".into()
    } else if m.contains("no devices") || m.contains("not found") && m.contains("device") {
        "no handheld found — check the cable and that USB debugging is on".into()
    } else if m.contains("offline") {
        "the handheld is connected but not responding — unplug it and plug it back in".into()
    } else if m.contains("no space left") {
        "the handheld is full".into()
    } else if m.contains("read-only") || m.contains("permission denied") {
        format!("Android didn't allow writing there ({})", msg.trim())
    } else {
        msg.trim().to_string()
    }
}

// ---------------------------------------------------------------- parsing

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial: String,
    /// "device" (ready), "unauthorized", "offline", …
    pub state: String,
    pub model: Option<String>,
}

pub fn parse_devices(text: &str) -> Vec<Device> {
    text.lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            let model = parts
                .find_map(|p| p.strip_prefix("model:"))
                .map(|m| m.replace('_', " "));
            Some(Device {
                serial,
                state,
                model,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// e.g. /storage/emulated/0 or /storage/1A2B-3C4D
    pub path: String,
    /// "Internal storage" or "SD card (1A2B-3C4D)"
    pub label: String,
    pub size_bytes: u64,
    pub free_bytes: u64,
    /// Existing ROMs folder name on this volume (e.g. "ROMs"), if any.
    pub roms_dir: Option<String>,
    /// Folders inside that ROMs folder (to reuse e.g. "PS2" rather than make "ps2").
    #[serde(default)]
    pub console_folders: Vec<String>,
}

pub fn parse_volumes(text: &str) -> Vec<Volume> {
    let mut vols: Vec<Volume> = Vec::new();
    for l in text.lines().map(|l| l.trim_end_matches('\r')) {
        let f: Vec<&str> = l.split('|').collect();
        match f.as_slice() {
            ["V", path, size, avail] => {
                if vols.iter().any(|v| v.path == *path) {
                    continue;
                }
                let label = if path.starts_with("/storage/emulated") {
                    "Internal storage".to_string()
                } else {
                    format!("SD card ({})", path.trim_start_matches("/storage/"))
                };
                vols.push(Volume {
                    path: path.to_string(),
                    label,
                    size_bytes: size.parse::<u64>().unwrap_or(0) * 1024,
                    free_bytes: avail.parse::<u64>().unwrap_or(0) * 1024,
                    roms_dir: None,
                    console_folders: vec![],
                });
            }
            ["R", path, dir] => {
                if let Some(v) = vols.iter_mut().find(|v| v.path == *path) {
                    v.roms_dir.get_or_insert_with(|| dir.to_string());
                }
            }
            _ => {}
        }
    }
    vols
}

// ---------------------------------------------------------------- download

/// Download Google's platform-tools for this computer, check it against the
/// checksum Google publishes, and unpack it into RomCrate's folder.
pub async fn install(
    client: &SafeClient,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> anyhow::Result<PathBuf> {
    let xml = client
        .get(&url::Url::parse(REPO_XML)?, 20 * 1024 * 1024)
        .await
        .map_err(|e| anyhow::anyhow!("couldn't read Google's package list: {e}"))?;
    let host = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macosx"
    } else {
        "linux"
    };
    let arch = find_platform_tools(&String::from_utf8_lossy(&xml), host).ok_or_else(|| {
        anyhow::anyhow!("Google's package list has no platform-tools for this computer")
    })?;
    let url = url::Url::parse(REPO_BASE)?.join(&arch.url)?;
    let dir = tools_dir();
    let parent = dir.parent().unwrap_or(&dir).to_path_buf();
    fs::create_dir_all(&parent)?;
    let zip_path = parent.join("platform-tools.zip");
    client
        .download_to_file(&url, &zip_path, MAX_TOOLS_ZIP, progress)
        .await
        .map_err(|e| anyhow::anyhow!("download failed: {e}"))?;
    let got = sha1_file(&zip_path)?;
    if got != arch.sha1.to_lowercase() || fs::metadata(&zip_path)?.len() != arch.size {
        let _ = fs::remove_file(&zip_path);
        anyhow::bail!(
            "the download doesn't match Google's checksum, so it wasn't used — try again"
        );
    }
    let zp = zip_path.clone();
    let parent2 = parent.clone();
    tokio::task::spawn_blocking(move || unpack_tools(&zp, &parent2)).await??;
    let _ = fs::remove_file(&zip_path);
    let exe = dir.join(exe_name());
    anyhow::ensure!(exe.is_file(), "platform-tools unpacked but adb is missing");
    Ok(exe)
}

#[derive(Debug, PartialEq)]
pub struct ToolsArchive {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

/// The platform-tools archive for `host` ("windows", "macosx", "linux") from
/// Google's repository2 XML.
pub fn find_platform_tools(xml: &str, host: &str) -> Option<ToolsArchive> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let pkg = doc.descendants().find(|n| {
        n.tag_name().name() == "remotePackage" && n.attribute("path") == Some("platform-tools")
    })?;
    pkg.descendants()
        .filter(|n| n.tag_name().name() == "archive")
        .find_map(|a| {
            let text = |name: &str| {
                a.descendants()
                    .find(|n| n.tag_name().name() == name)
                    .and_then(|n| n.text())
                    .map(|t| t.trim().to_string())
            };
            let os = text("host-os");
            if os.as_deref() != Some(host) {
                return None;
            }
            let complete = a
                .descendants()
                .find(|n| n.tag_name().name() == "complete")?;
            let get = |name: &str| {
                complete
                    .children()
                    .find(|n| n.tag_name().name() == name)
                    .and_then(|n| n.text())
                    .map(|t| t.trim().to_string())
            };
            Some(ToolsArchive {
                url: get("url")?,
                sha1: get("checksum")?,
                size: get("size")?.parse().ok()?,
            })
        })
}

fn sha1_file(p: &Path) -> anyhow::Result<String> {
    use sha1::Digest;
    let mut h = sha1::Sha1::new();
    let mut f = File::open(p)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

pub fn md5_file(p: &Path) -> anyhow::Result<String> {
    use md5::Digest;
    let mut h = md5::Md5::new();
    let mut f = File::open(p)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Unpack only `platform-tools/…` entries, refusing paths that escape.
fn unpack_tools(zip_path: &Path, into: &Path) -> anyhow::Result<()> {
    let mut ar = zip::ZipArchive::new(File::open(zip_path)?)?;
    for i in 0..ar.len() {
        let mut e = ar.by_index(i)?;
        let Some(rel) = e.enclosed_name() else {
            continue;
        };
        if !rel.starts_with("platform-tools") {
            continue;
        }
        let out = into.join(&rel);
        if e.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p)?;
        }
        let mut f = File::create(&out)?;
        std::io::copy(&mut e, &mut f)?;
        #[cfg(unix)]
        if let Some(mode) = e.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o755))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- plan

/// Where one console folder from the PC goes on the handheld.
#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// The console folder on this PC (e.g. D:\ROMs\snes).
    pub local: PathBuf,
    pub system_id: String,
    /// Volume path on the handheld, e.g. /storage/1A2B-3C4D.
    pub volume: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    pub local: PathBuf,
    pub remote: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendPlan {
    pub files: Vec<PlannedFile>,
    /// Already on the handheld (same name and size).
    pub already_there: usize,
    /// Different file with the same name on the handheld — left alone.
    pub conflicts: Vec<String>,
    /// Bytes to copy per volume, and whether it fits.
    pub per_volume: Vec<VolumeNeed>,
    /// Remote console folder per local folder (for showing the user).
    pub folders: Vec<(String, String)>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeNeed {
    pub volume: String,
    pub label: String,
    pub bytes: u64,
    pub free_bytes: u64,
    pub fits: bool,
}

/// The ROMs folder to use on a volume: the one already there, or "ROMs".
fn roms_root(v: &Volume) -> String {
    format!("{}/{}", v.path, v.roms_dir.as_deref().unwrap_or("ROMs"))
}

/// The console folder on the handheld: reuse an existing folder for the same
/// console (e.g. "PS2"), otherwise the PC folder's name.
fn console_folder(v: &Volume, system_id: &str, local_name: &str) -> String {
    v.console_folders
        .iter()
        .find(|f| crate::systems::detect_from_folder(f).map(|s| s.id) == Some(system_id))
        .cloned()
        .unwrap_or_else(|| local_name.to_string())
}

fn skip_local(name: &str) -> bool {
    let l = name.to_lowercase();
    l.contains(".bak")
        || l == "desktop.ini"
        || l == "thumbs.db"
        || l == ".ds_store"
        || l.starts_with("._")
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<(PathBuf, String, u64)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() || skip_local(&name) {
            continue;
        }
        let p = e.path();
        if ft.is_dir() {
            walk(&p, base, out);
        } else if ft.is_file() {
            let rel = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((p.clone(), rel, e.metadata().map(|m| m.len()).unwrap_or(0)));
        }
    }
}

/// Work out what to copy. `remote_files(dir)` lists what's already on the
/// handheld under a folder (path → size).
pub fn plan(
    targets: &[Target],
    volumes: &[Volume],
    remote_files: &mut dyn FnMut(&str) -> HashMap<String, u64>,
) -> anyhow::Result<SendPlan> {
    let mut plan = SendPlan::default();
    let mut need: BTreeMap<String, u64> = BTreeMap::new();
    for t in targets {
        let v = volumes
            .iter()
            .find(|v| v.path == t.volume)
            .ok_or_else(|| anyhow::anyhow!("{} isn't on the handheld any more", t.volume))?;
        let local_name = t
            .local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| t.system_id.clone());
        let remote_dir = format!(
            "{}/{}",
            roms_root(v),
            console_folder(v, &t.system_id, &local_name)
        );
        plan.folders
            .push((t.local.to_string_lossy().into_owned(), remote_dir.clone()));
        let there = remote_files(&remote_dir);
        let mut files = Vec::new();
        walk(&t.local, &t.local, &mut files);
        for (local, rel, bytes) in files {
            let remote = format!("{remote_dir}/{rel}");
            match there.get(&remote) {
                Some(&s) if s == bytes => plan.already_there += 1,
                Some(_) if rel.eq_ignore_ascii_case("metadata.pegasus.txt") => plan.notes.push(
                    format!("{remote_dir}: kept the metadata.pegasus.txt already on the handheld"),
                ),
                Some(_) => plan.conflicts.push(remote),
                None => {
                    *need.entry(v.path.clone()).or_default() += bytes;
                    plan.files.push(PlannedFile {
                        local,
                        remote,
                        bytes,
                    });
                }
            }
        }
    }
    for (path, bytes) in need {
        let v = volumes.iter().find(|v| v.path == path).expect("volume");
        plan.per_volume.push(VolumeNeed {
            label: v.label.clone(),
            fits: bytes + SPACE_MARGIN <= v.free_bytes,
            volume: path,
            bytes,
            free_bytes: v.free_bytes,
        });
    }
    Ok(plan)
}

// ---------------------------------------------------------------- send

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SendEvent {
    #[serde(rename_all = "camelCase")]
    Started { files: usize, bytes: u64 },
    #[serde(rename_all = "camelCase")]
    File {
        index: usize,
        remote: String,
        bytes: u64,
        ok: bool,
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    Finished { summary: SendSummary },
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendSummary {
    pub copied: usize,
    pub verified: usize,
    pub failed: usize,
    pub already_there: usize,
    pub conflicts: usize,
    pub bytes: u64,
    pub cancelled: bool,
}

/// Copy the planned files, checking each one. Stops (keeping what's done) when
/// `cancel()` returns true.
pub fn send(
    adb: &Adb,
    serial: &str,
    plan: &SendPlan,
    emit: &dyn Fn(SendEvent),
    cancel: &dyn Fn() -> bool,
) -> SendSummary {
    let mut sum = SendSummary {
        already_there: plan.already_there,
        conflicts: plan.conflicts.len(),
        ..Default::default()
    };
    if plan.per_volume.iter().any(|v| !v.fits) {
        for v in plan.per_volume.iter().filter(|v| !v.fits) {
            emit(SendEvent::File {
                index: 0,
                remote: v.volume.clone(),
                bytes: 0,
                ok: false,
                message: format!(
                    "Not enough space on {}: needs {}, {} free — nothing was copied",
                    v.label,
                    human(v.bytes + SPACE_MARGIN),
                    human(v.free_bytes)
                ),
            });
        }
        sum.failed = plan.files.len();
        emit(SendEvent::Finished {
            summary: sum.clone(),
        });
        return sum;
    }
    emit(SendEvent::Started {
        files: plan.files.len(),
        bytes: plan.files.iter().map(|f| f.bytes).sum(),
    });
    for (i, f) in plan.files.iter().enumerate() {
        if cancel() {
            sum.cancelled = true;
            break;
        }
        let result = (|| -> anyhow::Result<&'static str> {
            // Checked again right before copying: the plan may be minutes old,
            // and a file that has appeared since is never replaced (or removed).
            if adb.exists(serial, &f.remote)? {
                return Ok("appeared");
            }
            adb.push(serial, &f.local, &f.remote)?;
            if f.bytes >= HASH_FROM {
                let here = md5_file(&f.local)?;
                let there = adb.md5(serial, &f.remote)?;
                if here != there {
                    let _ = adb.remove(serial, &f.remote);
                    anyhow::bail!("the copy on the handheld didn't match, so it was removed again — try once more, or try another cable or USB port");
                }
                Ok("copied and checked")
            } else {
                let dir = f.remote.rsplit_once('/').map(|(d, _)| d).unwrap_or("/");
                let size = adb.list_files(serial, dir)?.get(&f.remote).copied();
                if size != Some(f.bytes) {
                    let _ = adb.remove(serial, &f.remote);
                    anyhow::bail!(
                        "the copy on the handheld is the wrong size, so it was removed again"
                    );
                }
                Ok("copied")
            }
        })();
        match result {
            Ok("appeared") => {
                sum.conflicts += 1;
                emit(SendEvent::File {
                    index: i,
                    remote: f.remote.clone(),
                    bytes: 0,
                    ok: true,
                    message: "a file with this name is on the handheld now — left as it is"
                        .to_string(),
                });
            }
            Ok(how) => {
                sum.copied += 1;
                if how == "copied and checked" {
                    sum.verified += 1;
                }
                sum.bytes += f.bytes;
                emit(SendEvent::File {
                    index: i,
                    remote: f.remote.clone(),
                    bytes: f.bytes,
                    ok: true,
                    message: how.to_string(),
                });
            }
            Err(e) => {
                sum.failed += 1;
                emit(SendEvent::File {
                    index: i,
                    remote: f.remote.clone(),
                    bytes: f.bytes,
                    ok: false,
                    message: e.to_string(),
                });
            }
        }
    }
    emit(SendEvent::Finished {
        summary: sum.clone(),
    });
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_devices() {
        let t = "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\nABC123   device usb:1-1 product:odin2 model:AYN_Odin2_Portal device:odin2 transport_id:1\nXYZ   unauthorized usb:1-2 transport_id:2\n\n";
        let d = parse_devices(t);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].model.as_deref(), Some("AYN Odin2 Portal"));
        assert_eq!(d[0].state, "device");
        assert_eq!(d[1].state, "unauthorized");
    }

    #[test]
    fn parses_volumes_and_roms_folders() {
        let t = "V|/storage/emulated/0|229000000|120000000\nR|/storage/emulated/0|ROMs\nV|/storage/1A2B-3C4D|500000000|400000000\nR|/storage/1A2B-3C4D|roms\nR|/storage/1A2B-3C4D|Emulation/roms\n";
        let v = parse_volumes(t);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].label, "Internal storage");
        assert_eq!(v[0].roms_dir.as_deref(), Some("ROMs"));
        assert_eq!(v[1].label, "SD card (1A2B-3C4D)");
        assert_eq!(v[1].roms_dir.as_deref(), Some("roms"), "first found wins");
        assert_eq!(v[1].free_bytes, 400_000_000 * 1024);
    }

    #[test]
    fn finds_platform_tools_in_googles_list() {
        let xml = r#"<?xml version="1.0"?><sdk:sdk-repository xmlns:sdk="http://schemas.android.com/sdk/android/repo/repository2/03">
<remotePackage path="build-tools;35.0.0"><archives><archive><complete><size>1</size><checksum type="sha1">aa</checksum><url>bt.zip</url></complete><host-os>windows</host-os></archive></archives></remotePackage>
<remotePackage path="platform-tools"><revision><major>36</major></revision><archives>
<archive><complete><size>6982025</size><checksum type="sha1">0123456789abcdef0123456789abcdef01234567</checksum><url>platform-tools_r36.0.0-win.zip</url></complete><host-os>windows</host-os></archive>
<archive><complete><size>7111111</size><checksum type="sha1">fedcba9876543210fedcba9876543210fedcba98</checksum><url>platform-tools_r36.0.0-linux.zip</url></complete><host-os>linux</host-os></archive>
</archives></remotePackage></sdk:sdk-repository>"#;
        assert_eq!(
            find_platform_tools(xml, "linux"),
            Some(ToolsArchive {
                url: "platform-tools_r36.0.0-linux.zip".into(),
                sha1: "fedcba9876543210fedcba9876543210fedcba98".into(),
                size: 7111111
            })
        );
        assert_eq!(find_platform_tools(xml, "macosx"), None);
    }

    #[test]
    fn quotes_awkward_names() {
        assert_eq!(q("Itchy & Scratchy's.smc"), r"'Itchy & Scratchy'\''s.smc'");
    }

    #[test]
    fn plan_skips_same_reuses_folders_and_never_overwrites() {
        let t = tempfile::tempdir().unwrap();
        let snes = t.path().join("snes");
        fs::create_dir_all(snes.join("media/box2dfront")).unwrap();
        fs::write(snes.join("A.sfc"), vec![1u8; 100]).unwrap();
        fs::write(snes.join("B.sfc"), vec![2u8; 100]).unwrap();
        fs::write(snes.join("C.sfc"), vec![3u8; 100]).unwrap();
        fs::write(snes.join("media/box2dfront/A.png"), vec![4u8; 10]).unwrap();
        fs::write(snes.join("metadata.pegasus.txt"), "collection: SNES\n").unwrap();
        fs::write(snes.join("metadata.pegasus.txt.bak"), "old").unwrap();
        let vols = vec![Volume {
            path: "/storage/1A2B-3C4D".into(),
            label: "SD card (1A2B-3C4D)".into(),
            size_bytes: 1 << 40,
            free_bytes: 1 << 40,
            roms_dir: Some("roms".into()),
            console_folders: vec!["SNES".into()],
        }];
        let targets = vec![Target {
            local: snes.clone(),
            system_id: "snes".into(),
            volume: "/storage/1A2B-3C4D".into(),
        }];
        let mut lister = |dir: &str| {
            assert_eq!(
                dir, "/storage/1A2B-3C4D/roms/SNES",
                "reuses the existing SNES folder"
            );
            HashMap::from([
                (format!("{dir}/A.sfc"), 100u64),              // same → skip
                (format!("{dir}/B.sfc"), 999u64),              // different → leave alone
                (format!("{dir}/metadata.pegasus.txt"), 5u64), // keep the device's
            ])
        };
        let p = plan(&targets, &vols, &mut lister).unwrap();
        let remotes: Vec<_> = p.files.iter().map(|f| f.remote.as_str()).collect();
        assert_eq!(
            remotes,
            [
                "/storage/1A2B-3C4D/roms/SNES/C.sfc",
                "/storage/1A2B-3C4D/roms/SNES/media/box2dfront/A.png"
            ]
        );
        assert_eq!(p.already_there, 1);
        assert_eq!(p.conflicts, ["/storage/1A2B-3C4D/roms/SNES/B.sfc"]);
        assert!(p.notes[0].contains("kept the metadata.pegasus.txt"));
        assert!(p.per_volume[0].fits);
    }

    #[test]
    fn refuses_when_the_card_is_full() {
        let t = tempfile::tempdir().unwrap();
        let n64 = t.path().join("n64");
        fs::create_dir_all(&n64).unwrap();
        fs::write(n64.join("A.z64"), vec![0u8; 4096]).unwrap();
        let vols = vec![Volume {
            path: "/storage/emulated/0".into(),
            label: "Internal storage".into(),
            size_bytes: 1 << 30,
            free_bytes: 100 * 1024 * 1024,
            roms_dir: None,
            console_folders: vec![],
        }];
        let targets = vec![Target {
            local: n64,
            system_id: "n64".into(),
            volume: "/storage/emulated/0".into(),
        }];
        let p = plan(&targets, &vols, &mut |_| HashMap::new()).unwrap();
        assert_eq!(
            p.files[0].remote, "/storage/emulated/0/ROMs/n64/A.z64",
            "makes ROMs/n64 when there's none"
        );
        assert!(!p.per_volume[0].fits, "keeps 200 MB spare");
        let adb = Adb {
            exe: "/nonexistent/adb".into(),
        };
        let events = std::cell::RefCell::new(vec![]);
        let s = send(&adb, "X", &p, &|e| events.borrow_mut().push(e), &|| false);
        assert_eq!(s.copied, 0);
        assert!(
            matches!(&events.borrow()[0], SendEvent::File { ok: false, message, .. } if message.contains("Not enough space"))
        );
    }
}
