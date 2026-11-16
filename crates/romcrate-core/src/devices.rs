//! Devices the games are being prepared for. Picking one sets sensible
//! defaults (frontend layout, Android launch lines, artwork size) and warns
//! about consoles the device can't realistically run.
//!
//! The data lives in `data/devices.json` with its sources; only entries
//! checked on a real unit are marked `tested`.

use crate::launchers::LauncherKind;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const DATA: &str = include_str!("../data/devices.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
    pub inches: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub family: String,
    /// "android", "linux" or "desktop".
    pub platform: String,
    pub screen: Screen,
    pub chipset: String,
    /// Frontend layout to use by default.
    pub launcher: LauncherKind,
    /// Consoles it can't realistically run.
    pub too_demanding: Vec<String>,
    /// Consoles where only some games run well.
    pub limited: Vec<String>,
    pub tested: bool,
    #[serde(default)]
    pub tested_note: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Data {
    devices: Vec<Device>,
}

/// All known devices.
pub fn all() -> &'static [Device] {
    static D: OnceLock<Vec<Device>> = OnceLock::new();
    D.get_or_init(|| {
        serde_json::from_str::<Data>(DATA)
            .expect("data/devices.json is valid (checked by tests)")
            .devices
    })
}

pub fn by_id(id: &str) -> Option<&'static Device> {
    all().iter().find(|d| d.id == id)
}

/// How well a device copes with a console.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    Fine,
    Limited,
    TooDemanding,
}

impl Device {
    pub fn fit(&self, system: &str) -> Fit {
        if self.too_demanding.iter().any(|s| s == system) {
            Fit::TooDemanding
        } else if self.limited.iter().any(|s| s == system) {
            Fit::Limited
        } else {
            Fit::Fine
        }
    }

    /// Artwork size preset id that suits the screen (see `launchers::size_presets`).
    pub fn size_preset(&self) -> &'static str {
        if self.screen.height.min(self.screen.width) <= 900 {
            "compact"
        } else {
            "standard"
        }
    }

    /// Platform for Pegasus launch lines, when this device runs Android.
    pub fn launch_platform(&self) -> Option<&'static str> {
        (self.platform == "android").then_some("android")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn data_is_valid() {
        let mut ids = HashSet::new();
        assert!(!all().is_empty());
        for d in all() {
            assert!(ids.insert(&d.id), "duplicate {}", d.id);
            assert!(!d.sources.is_empty(), "{} needs a source", d.id);
            assert!(
                ["android", "linux", "desktop"].contains(&d.platform.as_str()),
                "{}",
                d.id
            );
            assert!(d.screen.width >= 320 && d.screen.height >= 240, "{}", d.id);
            for s in d.too_demanding.iter().chain(&d.limited) {
                assert!(
                    crate::systems::by_id(s).is_some(),
                    "{}: unknown system {s}",
                    d.id
                );
            }
            let overlap: Vec<_> = d
                .too_demanding
                .iter()
                .filter(|s| d.limited.contains(s))
                .collect();
            assert!(overlap.is_empty(), "{}: {overlap:?} in both lists", d.id);
            assert!(
                crate::launchers::launchers()
                    .iter()
                    .any(|l| l.id == d.launcher),
                "{}: launcher not offered in the app",
                d.id
            );
            assert!(
                crate::launchers::size_presets()
                    .iter()
                    .any(|p| p.0 == d.size_preset()),
                "{}",
                d.id
            );
        }
        // Only the owner's own device is marked tested.
        let tested: Vec<_> = all()
            .iter()
            .filter(|d| d.tested)
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(tested, ["odin2-portal"]);
    }

    #[test]
    fn defaults_follow_the_hardware() {
        let odin = by_id("odin2-portal").unwrap();
        assert_eq!(odin.launcher, LauncherKind::Pegasus);
        assert_eq!(odin.launch_platform(), Some("android"));
        assert_eq!(odin.size_preset(), "standard");
        assert_eq!(odin.fit("ps2"), Fit::Fine);
        let small = by_id("anbernic-h700").unwrap();
        assert_eq!(small.size_preset(), "compact");
        assert_eq!(small.launch_platform(), None);
        assert_eq!(small.fit("ps2"), Fit::TooDemanding);
        assert_eq!(small.fit("snes"), Fit::Fine);
        assert_eq!(by_id("steam-deck").unwrap().size_preset(), "compact");
    }
}
