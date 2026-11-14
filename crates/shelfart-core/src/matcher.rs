//! Matches ROM file names to the names used by the artwork collection.
//!
//! libretro artwork is named after the No-Intro / Redump title, e.g.
//! `Super Mario World (USA).png`. Users' ROMs are often named the same way, but
//! may also be renamed, have different region tags or use scene-style names.
//! We try, in order: exact name, same title with the best region/tag fit, and
//! finally a fuzzy title comparison.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Characters libretro replaces with `_` in thumbnail file names.
const LIBRETRO_BAD_CHARS: &[char] = &['&', '*', '/', ':', '`', '<', '>', '?', '\\', '|', '"'];

/// Tags that indicate a non-final release; avoided unless the ROM has them too.
const UNWANTED_TAGS: &[&str] = &[
    "beta",
    "proto",
    "prototype",
    "demo",
    "sample",
    "pirate",
    "unl",
    "kiosk",
    "hack",
    "aftermarket",
    "debug",
];

const KNOWN_REGIONS: &[&str] = &[
    "usa",
    "world",
    "europe",
    "japan",
    "australia",
    "uk",
    "france",
    "germany",
    "spain",
    "italy",
    "korea",
    "china",
    "brazil",
    "canada",
    "netherlands",
    "sweden",
    "asia",
    "taiwan",
    "hong kong",
    "russia",
    "scandinavia",
];

/// Apply libretro's file-name sanitisation.
pub fn libretro_sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if LIBRETRO_BAD_CHARS.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Split `Title (Tag) [Tag]` into the title and the lower-cased tags.
pub fn split_title(stem: &str) -> (String, Vec<String>) {
    let cut = stem.find(['(', '[']).unwrap_or(stem.len());
    let title = stem[..cut].trim().to_string();
    let mut tags = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in stem[cut..].chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                if depth == 1 {
                    cur.clear();
                    continue;
                }
            }
            ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    for part in cur.split(',') {
                        let t = part.trim().to_lowercase();
                        // GoodTools short region codes.
                        let t = match t.as_str() {
                            "u" | "us" => "usa".to_string(),
                            "e" | "eu" => "europe".to_string(),
                            "j" | "jp" => "japan".to_string(),
                            "w" => "world".to_string(),
                            "ue" | "ju" | "jue" => {
                                for c in t.chars() {
                                    tags.push(
                                        match c {
                                            'u' => "usa",
                                            'e' => "europe",
                                            _ => "japan",
                                        }
                                        .to_string(),
                                    );
                                }
                                continue;
                            }
                            _ => t,
                        };
                        if !t.is_empty() {
                            tags.push(t);
                        }
                    }
                    continue;
                }
            }
            _ => {}
        }
        if depth > 0 {
            cur.push(c);
        }
    }
    (title, tags)
}

/// A comparable form of a title: ascii, lower-case, punctuation-free, articles
/// moved, roman numerals turned into digits.
pub fn normalize_title(title: &str) -> String {
    let mut s = deunicode::deunicode(title).to_lowercase();
    // libretro artwork names have "&" replaced by "_" ("Ren _ Stimpy").
    s = s.replace(" _ ", " and ");
    s = s.replace('&', " and ").replace('+', " plus ");
    // "Legend of Zelda, The - A Link..." -> "Legend of Zelda - A Link..."
    for art in [", the", ", a", ", an"] {
        while let Some(i) = s.find(art) {
            let end = i + art.len();
            let next = s[end..].chars().next();
            if next.is_none() || next == Some(' ') || next == Some('-') || next == Some(':') {
                s.replace_range(i..end, "");
            } else {
                break;
            }
        }
    }
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    let words: Vec<String> = cleaned
        .split_whitespace()
        .map(|w| {
            match w {
                "brothers" => "bros",
                "ii" => "2",
                "iii" => "3",
                "iv" => "4",
                "v" => "5",
                "vi" => "6",
                "vii" => "7",
                "viii" => "8",
                "ix" => "9",
                "x" => "10",
                "xi" => "11",
                "xii" => "12",
                "xiii" => "13",
                other => other,
            }
            .to_string()
        })
        .collect();
    let mut words = words;
    if words.first().map(|w| w == "the").unwrap_or(false) && words.len() > 1 {
        words.remove(0);
    }
    words.join(" ")
}

#[derive(Debug, Clone)]
struct Candidate {
    name: String,
    title_norm: String,
    tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatchMethod {
    Exact,
    Title,
    Fuzzy,
}

#[derive(Debug, Clone, Serialize)]
pub struct Match {
    /// Name of the artwork file (without `.png`).
    pub name: String,
    pub method: MatchMethod,
    /// 0.0 – 1.0 confidence.
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchPrefs {
    /// Preferred regions, best first (lower-case), e.g. ["usa","world","europe","japan"].
    pub regions: Vec<String>,
    /// Allow fuzzy title matching.
    pub fuzzy: bool,
    /// Minimum similarity (0-1) for fuzzy matches.
    pub fuzzy_threshold: f32,
}

impl Default for MatchPrefs {
    fn default() -> Self {
        Self {
            regions: ["usa", "world", "europe", "australia", "uk", "japan"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            fuzzy: true,
            fuzzy_threshold: 0.92,
        }
    }
}

/// Index of all artwork names available for one system.
#[derive(Debug, Default, Clone)]
pub struct NameIndex {
    cands: Vec<Candidate>,
    exact: HashMap<String, usize>,
    by_title: HashMap<String, Vec<usize>>,
    /// Title with spaces removed: "Duck Tales" and "DuckTales" land together.
    by_squash: HashMap<String, Vec<usize>>,
    by_base: HashMap<String, Vec<usize>>,
}

fn squash(norm: &str) -> String {
    norm.chars().filter(|c| !c.is_whitespace()).collect()
}

impl NameIndex {
    pub fn new<I: IntoIterator<Item = String>>(names: I) -> Self {
        let mut idx = NameIndex::default();
        for name in names {
            if idx.exact.contains_key(&name.to_lowercase()) {
                continue;
            }
            let (title, tags) = split_title(&name);
            let title_norm = normalize_title(&title);
            let base_norm = normalize_title(title.split(" - ").next().unwrap_or(&title));
            let i = idx.cands.len();
            idx.exact.insert(name.to_lowercase(), i);
            idx.by_title.entry(title_norm.clone()).or_default().push(i);
            idx.by_squash
                .entry(squash(&title_norm))
                .or_default()
                .push(i);
            idx.by_base.entry(base_norm).or_default().push(i);
            idx.cands.push(Candidate {
                name,
                title_norm,
                tags,
            });
        }
        idx
    }

    pub fn len(&self) -> usize {
        self.cands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cands.is_empty()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.exact.contains_key(&name.to_lowercase())
    }

    /// Find the best artwork name for a ROM file stem.
    pub fn best_match(&self, rom_stem: &str, prefs: &MatchPrefs) -> Option<Match> {
        if self.cands.is_empty() {
            return None;
        }
        let sanitized = libretro_sanitize(rom_stem);
        if let Some(&i) = self.exact.get(&sanitized.to_lowercase()) {
            return Some(Match {
                name: self.cands[i].name.clone(),
                method: MatchMethod::Exact,
                confidence: 1.0,
            });
        }

        let (title, rom_tags) = split_title(rom_stem);
        let norm = normalize_title(&title);
        if norm.is_empty() {
            return None;
        }

        // Same title, ignoring spacing, so every catalogue spelling competes on region/tags.
        if let Some(list) = self.by_squash.get(&squash(&norm)) {
            let i = self.pick_best(list, &rom_tags, prefs);
            return Some(Match {
                name: self.cands[i].name.clone(),
                method: MatchMethod::Title,
                confidence: 0.95,
            });
        }

        if !prefs.fuzzy {
            return None;
        }

        // ROM has no subtitle but the artwork does, or vice versa: compare the part before " - ".
        let base = normalize_title(title.split(" - ").next().unwrap_or(&title));
        if base.len() >= 4 {
            if let Some(list) = self.by_base.get(&base) {
                // Only accept when the base title is unambiguous.
                let distinct: std::collections::HashSet<_> =
                    list.iter().map(|&i| &self.cands[i].title_norm).collect();
                if distinct.len() == 1 {
                    let i = self.pick_best(list, &rom_tags, prefs);
                    return Some(Match {
                        name: self.cands[i].name.clone(),
                        method: MatchMethod::Fuzzy,
                        confidence: 0.85,
                    });
                }
            }
        }

        // Similarity search over distinct titles.
        let mut best: Option<(f64, &String)> = None;
        for t in self.by_title.keys() {
            let sim = strsim::jaro_winkler(&norm, t);
            if sim >= prefs.fuzzy_threshold as f64 && best.map(|(b, _)| sim > b).unwrap_or(true) {
                // Guard against sequels being matched to each other ("Game 2" vs "Game 3").
                if digits(&norm) != digits(t) {
                    continue;
                }
                best = Some((sim, t));
            }
        }
        if let Some((sim, t)) = best {
            let i = self.pick_best(&self.by_title[t], &rom_tags, prefs);
            return Some(Match {
                name: self.cands[i].name.clone(),
                method: MatchMethod::Fuzzy,
                confidence: (sim as f32) * 0.9,
            });
        }

        // Short names: every word of the ROM's title appears in exactly one
        // artwork title that has at most one extra word ("Smash Bros" →
        // "Super Smash Bros."). Only accepted when unambiguous.
        let words: Vec<&str> = norm.split_whitespace().collect();
        if words.len() >= 2 {
            let hits: Vec<&String> = self
                .by_title
                .keys()
                .filter(|t| {
                    let tw: Vec<&str> = t.split_whitespace().collect();
                    tw.len() <= words.len() + 1
                        && words.iter().all(|w| tw.contains(w))
                        && digits(&norm) == digits(t)
                })
                .collect();
            if hits.len() == 1 {
                let i = self.pick_best(&self.by_title[hits[0]], &rom_tags, prefs);
                return Some(Match {
                    name: self.cands[i].name.clone(),
                    method: MatchMethod::Fuzzy,
                    confidence: 0.75,
                });
            }
        }
        None
    }

    fn pick_best(&self, list: &[usize], rom_tags: &[String], prefs: &MatchPrefs) -> usize {
        *list
            .iter()
            .max_by_key(|&&i| self.score(&self.cands[i], rom_tags, prefs))
            .expect("non-empty candidate list")
    }

    fn score(&self, c: &Candidate, rom_tags: &[String], prefs: &MatchPrefs) -> i64 {
        let mut s: i64 = 0;
        let rom_regions: Vec<&String> = rom_tags
            .iter()
            .filter(|t| KNOWN_REGIONS.contains(&t.as_str()))
            .collect();
        let cand_regions: Vec<&String> = c
            .tags
            .iter()
            .filter(|t| KNOWN_REGIONS.contains(&t.as_str()))
            .collect();

        // Same region as the ROM is the strongest signal.
        if !rom_regions.is_empty() && rom_regions.iter().all(|r| cand_regions.contains(r)) {
            s += 60;
        }
        // Otherwise the user's preferred region order.
        for (rank, pref) in prefs.regions.iter().enumerate() {
            if cand_regions.contains(&pref) {
                s += (20 - rank as i64).max(1) * 2;
                break;
            }
        }
        for t in &c.tags {
            let unwanted = UNWANTED_TAGS.iter().any(|u| t.starts_with(u));
            if rom_tags.contains(t) {
                s += 4;
            } else if unwanted {
                s -= 200;
            } else {
                s -= 1;
            }
        }
        // Avoid rerelease-specific art when the ROM doesn't ask for it.
        if c.tags.iter().any(|t| {
            t.contains("virtual console")
                || t.contains("switch online")
                || t.contains("classic mini")
        }) && !rom_tags.iter().any(|t| {
            t.contains("virtual console")
                || t.contains("switch online")
                || t.contains("classic mini")
        }) {
            s -= 8;
        }
        s
    }
}

fn digits(s: &str) -> Vec<String> {
    s.split_whitespace()
        .filter(|w| w.chars().all(|c| c.is_ascii_digit()))
        .map(String::from)
        .collect()
}

/// Turn a ROM stem into a friendly display title ("Legend of Zelda, The - A Link" → "The Legend of Zelda - A Link").
pub fn display_title(stem: &str) -> String {
    let (title, _) = split_title(stem);
    let title = if title.is_empty() {
        stem.to_string()
    } else {
        title
    };
    // Move trailing article back to the front of the main title.
    let (main, rest) = match title.find(" - ") {
        Some(i) => (title[..i].to_string(), title[i..].to_string()),
        None => (title.clone(), String::new()),
    };
    for art in ["The", "A", "An"] {
        let suffix = format!(", {art}");
        if let Some(m) = main.strip_suffix(&suffix) {
            return format!("{art} {m}{rest}");
        }
    }
    title.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> NameIndex {
        NameIndex::new(
            [
                "Super Mario World (USA)",
                "Super Mario World (Europe)",
                "Super Mario World (Europe) (Rev 1)",
                "Super Mario World (USA, Europe) (Virtual Console, Classic Mini, Switch Online)",
                "Super Mario World - Super Mario Bros. 4 (Japan)",
                "Legend of Zelda, The - A Link to the Past (USA)",
                "Final Fantasy III (USA)",
                "Final Fantasy II (USA)",
                "Street Fighter II Turbo (USA)",
                "Donkey Kong Country (USA) (Beta)",
                "Donkey Kong Country (Europe)",
                "Pokemon - Red Version (USA, Europe)",
                "Mega Man X (USA)",
                "Chrono Trigger (USA)",
            ]
            .into_iter()
            .map(String::from),
        )
    }

    #[test]
    fn exact_match() {
        let m = idx()
            .best_match("Super Mario World (Europe)", &MatchPrefs::default())
            .unwrap();
        assert_eq!(m.name, "Super Mario World (Europe)");
        assert_eq!(m.method, MatchMethod::Exact);
    }

    #[test]
    fn region_preference() {
        let m = idx()
            .best_match("super_mario_world", &MatchPrefs::default())
            .unwrap();
        assert_eq!(m.name, "Super Mario World (USA)");
        let m = idx()
            .best_match("Super Mario World (Germany)", &MatchPrefs::default())
            .unwrap();
        assert_eq!(m.name, "Super Mario World (USA)");
    }

    #[test]
    fn articles_and_numerals() {
        let p = MatchPrefs::default();
        assert_eq!(
            idx()
                .best_match("The Legend of Zelda - A Link to the Past", &p)
                .unwrap()
                .name,
            "Legend of Zelda, The - A Link to the Past (USA)"
        );
        assert_eq!(
            idx().best_match("Final Fantasy 3 [!]", &p).unwrap().name,
            "Final Fantasy III (USA)"
        );
        assert_eq!(
            idx()
                .best_match("Pokémon - Red Version (USA)", &p)
                .unwrap()
                .name,
            "Pokemon - Red Version (USA, Europe)"
        );
    }

    #[test]
    fn short_names_match_when_unambiguous() {
        let p = MatchPrefs::default();
        let i = NameIndex::new(
            [
                "Super Smash Bros. (USA)",
                "Mario Kart 64 (USA)",
                "Super Mario Kart (USA)",
                "Mario Tennis (USA)",
            ]
            .into_iter()
            .map(String::from),
        );
        assert_eq!(
            i.best_match("Smash Brothers", &p).unwrap().name,
            "Super Smash Bros. (USA)"
        );
        // "Mario Kart" alone must not pick the N64 game (digits differ) — the SNES one is unambiguous.
        assert_eq!(
            i.best_match("Mario Kart", &p).unwrap().name,
            "Super Mario Kart (USA)"
        );
        // Single words are never guessed.
        assert!(i.best_match("Mario", &p).is_none());
    }

    #[test]
    fn ampersands_match_libretro_underscores() {
        let p = MatchPrefs::default();
        let i = NameIndex::new([
            "Itchy _ Scratchy Game, The (USA)".to_string(),
            "Ren _ Stimpy Show, The - Veediots! (USA)".to_string(),
        ]);
        assert_eq!(
            i.best_match("Itchy & Scratchy", &p).unwrap().name,
            "Itchy _ Scratchy Game, The (USA)"
        );
        assert_eq!(
            i.best_match("Ren and Stimpy Show, The - Veediots!", &p)
                .unwrap()
                .name,
            "Ren _ Stimpy Show, The - Veediots! (USA)"
        );
    }

    #[test]
    fn avoids_betas_and_sequels() {
        let p = MatchPrefs::default();
        assert_eq!(
            idx()
                .best_match("Donkey Kong Country (USA)", &p)
                .unwrap()
                .name,
            "Donkey Kong Country (Europe)"
        );
        // "Final Fantasy 4" must not fuzzy-match FF2 or FF3.
        assert!(idx().best_match("Final Fantasy IV (USA)", &p).is_none());
    }

    #[test]
    fn fuzzy_typos() {
        let p = MatchPrefs::default();
        assert_eq!(
            idx().best_match("Crono Trigger", &p).unwrap().name,
            "Chrono Trigger (USA)"
        );
        assert!(idx().best_match("Completely Different Game", &p).is_none());
    }

    #[test]
    fn sanitize_and_titles() {
        assert_eq!(
            libretro_sanitize("Tom & Jerry: Frantic"),
            "Tom _ Jerry_ Frantic"
        );
        assert_eq!(
            display_title("Legend of Zelda, The - A Link to the Past (USA)"),
            "The Legend of Zelda - A Link to the Past"
        );
        assert_eq!(
            split_title("Game (USA, Europe) (Rev 1) [!]").1,
            vec!["usa", "europe", "rev 1", "!"]
        );
    }

    #[test]
    fn spacing_variants_compete_on_region() {
        let idx = NameIndex::new(
            [
                "Duck Tales (1989-09)(Capcom)(US)",
                "DuckTales (USA)",
                "DuckTales (Europe)",
            ]
            .map(String::from),
        );
        let m = idx
            .best_match("Duck Tales", &MatchPrefs::default())
            .unwrap();
        assert_eq!(m.name, "DuckTales (USA)");
    }
}
