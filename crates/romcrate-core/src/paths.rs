//! Safe file naming: nothing from the network or from file names can make
//! RomCrate write outside the folder the user chose.

use std::path::{Component, Path, PathBuf};

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Make a single path component that is valid on Windows, macOS and Linux.
pub fn safe_component(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // No trailing dots/spaces (Windows), no leading dots (hidden / "..").
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    while s.starts_with('.') {
        s.remove(0);
    }
    if s.is_empty() {
        s = "_".into();
    }
    let stem = s.split('.').next().unwrap_or("").to_lowercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        s.insert(0, '_');
    }
    // Keep names a sane length (most filesystems allow 255 bytes).
    if s.len() > 200 {
        let mut cut = 200;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

/// Join `parts` under `root`, sanitising each part. The result is guaranteed
/// to be inside `root`.
pub fn join_safe(root: &Path, parts: &[&str]) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in parts {
        // A part may intentionally contain sub-folders (e.g. "subdir/name"), so
        // split and sanitise each segment.
        for seg in part.split(['/', '\\']) {
            if seg.is_empty() || seg == "." || seg == ".." {
                continue;
            }
            p.push(safe_component(seg));
        }
    }
    debug_assert!(is_within(root, &p));
    p
}

/// Lexical check that `p` is inside `root`.
pub fn is_within(root: &Path, p: &Path) -> bool {
    let norm = |x: &Path| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for c in x.components() {
            match c {
                Component::ParentDir => {
                    v.pop();
                }
                Component::CurDir => {}
                other => v.push(other.as_os_str().to_string_lossy().to_string()),
            }
        }
        v
    };
    let r = norm(root);
    let q = norm(p);
    q.len() >= r.len() && q[..r.len()] == r[..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes() {
        assert_eq!(safe_component("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_component("Game: The Sequel?"), "Game_ The Sequel_");
        assert_eq!(safe_component("CON"), "_CON");
        assert_eq!(safe_component("..."), "_");
    }

    #[test]
    fn join_stays_inside() {
        let root = Path::new("/media/roms");
        let p = join_safe(root, &["../../etc", "x/../../y.png"]);
        assert!(is_within(root, &p));
        assert_eq!(p, Path::new("/media/roms/etc/x/y.png"));
    }
}
