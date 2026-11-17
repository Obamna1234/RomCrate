//! Which handheld is plugged in, and which system it runs — read from Android's
//! own build properties (`getprop`) over USB debugging.
//!
//! Linux-based systems (AYN Linux, ROCKNIX, Knulli, muOS…) don't offer USB
//! debugging, so they never show up here: RomCrate can't see or change them
//! over the cable at all.

use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub manufacturer: String,
    pub model: String,
    /// Android version, e.g. "13".
    pub android: String,
    /// "stock" (the maker's own Android), "lineage" (LineageOS) or "other".
    pub os: String,
    /// For people: "Stock Android 13", "LineageOS 23.2 (Android 16)", …
    pub os_label: String,
    /// The RomCrate device this matches (an id from devices.json), if any.
    pub device_id: Option<String>,
    /// Shown when the system isn't one RomCrate knows.
    pub caution: Option<String>,
}

/// Parse `getprop` output: lines like `[ro.product.model]: [Odin2 Portal]`.
pub fn parse_props(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (k, v) = l.split_once("]: [")?;
            Some((
                k.trim_start_matches('[').to_string(),
                v.trim_end_matches(']').to_string(),
            ))
        })
        .collect()
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Makers whose own Android builds RomCrate is made for.
const KNOWN_MAKERS: &[&str] = &["ayn", "retroid", "anbernic", "moorechip", "goretroid"];

/// (device id, name fragments that identify it), most specific first.
const MATCHES: &[(&str, &[&str])] = &[
    ("odin2-portal", &["odin2portal", "odinportal"]),
    ("odin2-mini", &["odin2mini"]),
    ("odin2", &["odin2"]),
    ("retroid-pocket-5", &["retroidpocket5", "pocket5", "rp5"]),
    ("anbernic-rg556", &["rg556"]),
];

pub fn identify(props: &HashMap<String, String>) -> Identity {
    let get = |k: &str| props.get(k).cloned().unwrap_or_default();
    let manufacturer = get("ro.product.manufacturer");
    let model = get("ro.product.model");
    let android = get("ro.build.version.release");
    let haystack = norm(&format!(
        "{} {} {} {}",
        model,
        get("ro.product.device"),
        get("ro.product.name"),
        get("ro.lineage.device")
    ));
    let device_id = MATCHES
        .iter()
        .find(|(_, keys)| keys.iter().any(|k| haystack.contains(k)))
        .map(|(id, _)| id.to_string());
    let lineage = get("ro.lineage.version");
    let maker = norm(&format!("{} {}", manufacturer, get("ro.product.brand")));
    let (os, os_label, caution) = if !lineage.is_empty() {
        let v = lineage.split('-').next().unwrap_or(&lineage).to_string();
        (
            "lineage",
            format!("LineageOS {v} (Android {android})"),
            None,
        )
    } else if KNOWN_MAKERS.iter().any(|m| maker.contains(m)) {
        ("stock", format!("Stock Android {android}"), None)
    } else {
        (
            "other",
            format!(
                "Android {android}{}",
                Some(get("ro.build.display.id")).filter(|s| !s.is_empty()).map(|s| format!(" ({s})")).unwrap_or_default()
            ),
            Some(format!(
                "RomCrate hasn't been checked on this system{}. Sending files is still safe (nothing is overwritten), but emulator settings may be stored in other places — RomCrate asks before changing anything.",
                if manufacturer.is_empty() { String::new() } else { format!(" ({manufacturer})") }
            )),
        )
    };
    Identity {
        manufacturer,
        model,
        android,
        os: os.into(),
        os_label,
        device_id,
        caution,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        let text: String = pairs
            .iter()
            .map(|(k, v)| format!("[{k}]: [{v}]\n"))
            .collect();
        parse_props(&text)
    }

    #[test]
    fn stock_portal() {
        let id = identify(&props(&[
            ("ro.product.manufacturer", "AYN"),
            ("ro.product.model", "Odin2 Portal"),
            ("ro.product.device", "odin2portal"),
            ("ro.build.version.release", "13"),
        ]));
        assert_eq!(id.device_id.as_deref(), Some("odin2-portal"));
        assert_eq!(id.os, "stock");
        assert_eq!(id.os_label, "Stock Android 13");
        assert!(id.caution.is_none());
    }

    #[test]
    fn lineage_portal_is_not_mistaken_for_odin2() {
        let id = identify(&props(&[
            ("ro.product.manufacturer", "AYN"),
            ("ro.product.model", "Odin2 Portal"),
            ("ro.lineage.device", "odin2portal"),
            ("ro.lineage.version", "23.2-20261101-NIGHTLY-odin2portal"),
            ("ro.build.version.release", "16"),
        ]));
        assert_eq!(id.device_id.as_deref(), Some("odin2-portal"));
        assert_eq!(id.os, "lineage");
        assert_eq!(id.os_label, "LineageOS 23.2 (Android 16)");
    }

    #[test]
    fn plain_odin2_and_mini() {
        let o = identify(&props(&[
            ("ro.product.manufacturer", "AYN"),
            ("ro.product.model", "Odin2"),
            ("ro.build.version.release", "13"),
        ]));
        assert_eq!(o.device_id.as_deref(), Some("odin2"));
        let m = identify(&props(&[
            ("ro.product.manufacturer", "AYN"),
            ("ro.product.model", "Odin2 Mini"),
            ("ro.build.version.release", "13"),
        ]));
        assert_eq!(m.device_id.as_deref(), Some("odin2-mini"));
    }

    #[test]
    fn unknown_system_gets_a_caution() {
        let id = identify(&props(&[
            ("ro.product.manufacturer", "Contoso"),
            ("ro.product.model", "Game Slab"),
            ("ro.build.version.release", "14"),
            ("ro.build.display.id", "CustomROM 2.1"),
        ]));
        assert_eq!(id.device_id, None);
        assert_eq!(id.os, "other");
        assert_eq!(id.os_label, "Android 14 (CustomROM 2.1)");
        assert!(id
            .caution
            .unwrap()
            .contains("asks before changing anything"));
    }
}
