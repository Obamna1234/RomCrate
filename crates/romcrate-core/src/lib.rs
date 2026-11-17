//! RomCrate core: an account-free, privacy-first artwork scraper for emulated games.
//!
//! Pipeline: [`scan`] ROM folders → load the artwork list for each system from
//! [`source`] → [`matcher`] picks the right artwork for each game → download
//! through the locked-down [`net`] client → [`imaging`] validates, resizes and
//! upscales → [`launchers`] decides where files go and updates metadata.

pub mod activity;
pub mod adb;
pub mod archive;
pub mod datadir;
pub mod devices;
pub mod gamesettings;
pub mod identify;
pub mod imaging;
pub mod import;
pub mod launch;
pub mod launchbox;
pub mod launchers;
pub mod matcher;
pub mod pergame;
pub mod net;
pub mod paths;
pub mod preflight;
pub mod ps3bios;
pub mod scan;
pub mod scrape;
pub mod screenscraper;
pub mod source;
pub mod systems;

pub use scrape::{run as run_scrape, Event, ScrapeConfig, Sources, Summary};
pub use source::{ArtKind, Libretro};
