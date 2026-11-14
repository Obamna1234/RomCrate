//! Catalogue of supported systems.
//!
//! Each entry links:
//! * the folder names launchers commonly use (ES-DE, Pegasus, RetroPie, Batocera…)
//! * the libretro-thumbnails collection name (art source)
//! * the LaunchBox platform name
//! * the ROM file extensions we recognise for that system

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct System {
    /// Stable short id used internally and in the UI.
    pub id: &'static str,
    /// Friendly name shown to the user.
    pub name: &'static str,
    /// Manufacturer, used to group systems in the UI.
    pub maker: &'static str,
    /// libretro-thumbnails collection name (e.g. "Nintendo - Super Nintendo Entertainment System").
    pub libretro: &'static str,
    /// GitHub repository name under github.com/libretro-thumbnails.
    pub repo: &'static str,
    /// Folder names that identify this system in a ROM library (lower-case).
    pub folders: &'static [&'static str],
    /// ROM file extensions (lower-case, no dot).
    pub extensions: &'static [&'static str],
    /// LaunchBox platform name.
    pub launchbox: &'static str,
}

macro_rules! sys {
    ($id:expr, $name:expr, $maker:expr, $lr:expr, $repo:expr, [$($f:expr),*], [$($e:expr),*], $lb:expr) => {
        System { id: $id, name: $name, maker: $maker, libretro: $lr, repo: $repo,
                 folders: &[$($f),*], extensions: &[$($e),*], launchbox: $lb }
    };
}

/// Disc / archive formats shared by many CD-based systems.
pub const ARCHIVES: &[&str] = &["zip", "7z"];

pub static SYSTEMS: &[System] = &[
    // ---------------- Nintendo ----------------
    sys!(
        "nes",
        "NES / Famicom",
        "Nintendo",
        "Nintendo - Nintendo Entertainment System",
        "Nintendo_-_Nintendo_Entertainment_System",
        [
            "nes",
            "famicom",
            "nintendo entertainment system",
            "nes-hacks"
        ],
        ["nes", "unf", "unif", "zip", "7z"],
        "Nintendo Entertainment System"
    ),
    sys!(
        "fds",
        "Famicom Disk System",
        "Nintendo",
        "Nintendo - Family Computer Disk System",
        "Nintendo_-_Family_Computer_Disk_System",
        ["fds", "famicom disk system"],
        ["fds", "zip", "7z"],
        "Nintendo Famicom Disk System"
    ),
    sys!(
        "snes",
        "Super Nintendo / Super Famicom",
        "Nintendo",
        "Nintendo - Super Nintendo Entertainment System",
        "Nintendo_-_Super_Nintendo_Entertainment_System",
        [
            "snes",
            "sfc",
            "superfamicom",
            "super nintendo",
            "snes-hacks",
            "snesna"
        ],
        ["sfc", "smc", "fig", "swc", "bs", "zip", "7z"],
        "Super Nintendo Entertainment System"
    ),
    sys!(
        "n64",
        "Nintendo 64",
        "Nintendo",
        "Nintendo - Nintendo 64",
        "Nintendo_-_Nintendo_64",
        ["n64", "nintendo 64"],
        ["z64", "n64", "v64", "zip", "7z"],
        "Nintendo 64"
    ),
    sys!(
        "n64dd",
        "Nintendo 64DD",
        "Nintendo",
        "Nintendo - Nintendo 64DD",
        "Nintendo_-_Nintendo_64DD",
        ["n64dd"],
        ["ndd", "zip", "7z"],
        "Nintendo 64DD"
    ),
    sys!(
        "gb",
        "Game Boy",
        "Nintendo",
        "Nintendo - Game Boy",
        "Nintendo_-_Game_Boy",
        ["gb", "gameboy", "game boy"],
        ["gb", "zip", "7z"],
        "Nintendo Game Boy"
    ),
    sys!(
        "gbc",
        "Game Boy Color",
        "Nintendo",
        "Nintendo - Game Boy Color",
        "Nintendo_-_Game_Boy_Color",
        ["gbc", "gameboy color", "game boy color"],
        ["gbc", "gb", "zip", "7z"],
        "Nintendo Game Boy Color"
    ),
    sys!(
        "gba",
        "Game Boy Advance",
        "Nintendo",
        "Nintendo - Game Boy Advance",
        "Nintendo_-_Game_Boy_Advance",
        ["gba", "gameboy advance", "game boy advance"],
        ["gba", "zip", "7z"],
        "Nintendo Game Boy Advance"
    ),
    sys!(
        "nds",
        "Nintendo DS",
        "Nintendo",
        "Nintendo - Nintendo DS",
        "Nintendo_-_Nintendo_DS",
        ["nds", "ds", "nintendo ds"],
        ["nds", "dsi", "zip", "7z"],
        "Nintendo DS"
    ),
    sys!(
        "3ds",
        "Nintendo 3DS",
        "Nintendo",
        "Nintendo - Nintendo 3DS",
        "Nintendo_-_Nintendo_3DS",
        ["3ds", "n3ds", "nintendo 3ds"],
        ["3ds", "cia", "cci", "cxi", "3dsx", "app", "zip"],
        "Nintendo 3DS"
    ),
    sys!(
        "gc",
        "GameCube",
        "Nintendo",
        "Nintendo - GameCube",
        "Nintendo_-_GameCube",
        ["gc", "gamecube", "ngc"],
        ["iso", "gcm", "gcz", "rvz", "ciso", "wbfs", "dol", "m3u"],
        "Nintendo GameCube"
    ),
    sys!(
        "wii",
        "Wii",
        "Nintendo",
        "Nintendo - Wii",
        "Nintendo_-_Wii",
        ["wii"],
        ["iso", "wbfs", "rvz", "gcz", "ciso", "wad", "m3u"],
        "Nintendo Wii"
    ),
    sys!(
        "wiiu",
        "Wii U",
        "Nintendo",
        "Nintendo - Wii U",
        "Nintendo_-_Wii_U",
        ["wiiu", "wii u"],
        ["wud", "wux", "wua", "rpx", "iso"],
        "Nintendo Wii U"
    ),
    sys!(
        "vb",
        "Virtual Boy",
        "Nintendo",
        "Nintendo - Virtual Boy",
        "Nintendo_-_Virtual_Boy",
        ["virtualboy", "vb", "virtual boy"],
        ["vb", "vboy", "zip", "7z"],
        "Nintendo Virtual Boy"
    ),
    sys!(
        "pokemini",
        "Pokémon Mini",
        "Nintendo",
        "Nintendo - Pokemon Mini",
        "Nintendo_-_Pokemon_Mini",
        ["pokemini", "pokemon mini"],
        ["min", "zip", "7z"],
        "Nintendo Pokemon Mini"
    ),
    sys!(
        "satellaview",
        "Satellaview",
        "Nintendo",
        "Nintendo - Satellaview",
        "Nintendo_-_Satellaview",
        ["satellaview", "bsx"],
        ["bs", "sfc", "smc", "zip", "7z"],
        "Nintendo Satellaview"
    ),
    sys!(
        "sufami",
        "Sufami Turbo",
        "Nintendo",
        "Nintendo - Sufami Turbo",
        "Nintendo_-_Sufami_Turbo",
        ["sufami", "sufamiturbo"],
        ["st", "sfc", "smc", "zip", "7z"],
        "Nintendo Sufami Turbo"
    ),
    // ---------------- Sega ----------------
    sys!(
        "sg1000",
        "SG-1000",
        "Sega",
        "Sega - SG-1000",
        "Sega_-_SG-1000",
        ["sg-1000", "sg1000"],
        ["sg", "bin", "zip", "7z"],
        "Sega SG-1000"
    ),
    sys!(
        "mastersystem",
        "Master System",
        "Sega",
        "Sega - Master System - Mark III",
        "Sega_-_Master_System_-_Mark_III",
        ["mastersystem", "sms", "master system", "markiii"],
        ["sms", "bin", "zip", "7z"],
        "Sega Master System"
    ),
    sys!(
        "megadrive",
        "Mega Drive / Genesis",
        "Sega",
        "Sega - Mega Drive - Genesis",
        "Sega_-_Mega_Drive_-_Genesis",
        [
            "megadrive",
            "genesis",
            "md",
            "mega drive",
            "megadrivejp",
            "sega genesis"
        ],
        ["md", "gen", "smd", "bin", "68k", "sgd", "zip", "7z"],
        "Sega Genesis"
    ),
    sys!(
        "gamegear",
        "Game Gear",
        "Sega",
        "Sega - Game Gear",
        "Sega_-_Game_Gear",
        ["gamegear", "gg", "game gear"],
        ["gg", "zip", "7z"],
        "Sega Game Gear"
    ),
    sys!(
        "segacd",
        "Mega-CD / Sega CD",
        "Sega",
        "Sega - Mega-CD - Sega CD",
        "Sega_-_Mega-CD_-_Sega_CD",
        ["segacd", "megacd", "sega cd", "mega-cd", "megacdjp"],
        ["chd", "cue", "iso", "m3u", "pbp"],
        "Sega CD"
    ),
    sys!(
        "sega32x",
        "32X",
        "Sega",
        "Sega - 32X",
        "Sega_-_32X",
        ["sega32x", "32x", "sega 32x"],
        ["32x", "bin", "zip", "7z"],
        "Sega 32X"
    ),
    sys!(
        "saturn",
        "Saturn",
        "Sega",
        "Sega - Saturn",
        "Sega_-_Saturn",
        ["saturn", "sega saturn", "saturnjp"],
        ["chd", "cue", "iso", "m3u", "ccd", "mds"],
        "Sega Saturn"
    ),
    sys!(
        "dreamcast",
        "Dreamcast",
        "Sega",
        "Sega - Dreamcast",
        "Sega_-_Dreamcast",
        ["dreamcast", "dc"],
        ["chd", "cdi", "gdi", "cue", "m3u"],
        "Sega Dreamcast"
    ),
    sys!(
        "naomi",
        "NAOMI",
        "Sega",
        "Sega - Naomi",
        "Sega_-_Naomi",
        ["naomi"],
        ["zip", "7z", "chd", "lst", "dat"],
        "Sega Naomi"
    ),
    sys!(
        "naomi2",
        "NAOMI 2",
        "Sega",
        "Sega - Naomi 2",
        "Sega_-_Naomi_2",
        ["naomi2"],
        ["zip", "7z", "chd"],
        "Sega Naomi 2"
    ),
    sys!(
        "atomiswave",
        "Atomiswave",
        "Sammy",
        "Atomiswave",
        "Atomiswave",
        ["atomiswave"],
        ["zip", "7z", "chd", "lst"],
        "Sammy Atomiswave"
    ),
    sys!(
        "pico",
        "Sega Pico",
        "Sega",
        "Sega - PICO",
        "Sega_-_PICO",
        ["pico", "segapico"],
        ["md", "bin", "zip", "7z"],
        "Sega Pico"
    ),
    // ---------------- Sony ----------------
    sys!(
        "psx",
        "PlayStation",
        "Sony",
        "Sony - PlayStation",
        "Sony_-_PlayStation",
        ["psx", "ps1", "playstation", "psone"],
        ["chd", "cue", "pbp", "m3u", "iso", "img", "ccd", "ecm", "mds"],
        "Sony Playstation"
    ),
    sys!(
        "ps2",
        "PlayStation 2",
        "Sony",
        "Sony - PlayStation 2",
        "Sony_-_PlayStation_2",
        ["ps2", "playstation 2", "playstation2"],
        ["iso", "chd", "cso", "zso", "gz", "m3u", "cue", "bin"],
        "Sony Playstation 2"
    ),
    sys!(
        "ps3",
        "PlayStation 3",
        "Sony",
        "Sony - PlayStation 3",
        "Sony_-_PlayStation_3",
        ["ps3", "playstation 3"],
        ["ps3", "iso", "pkg", "ps3dir"],
        "Sony Playstation 3"
    ),
    sys!(
        "psp",
        "PlayStation Portable",
        "Sony",
        "Sony - PlayStation Portable",
        "Sony_-_PlayStation_Portable",
        ["psp", "playstation portable"],
        ["iso", "cso", "pbp", "chd"],
        "Sony PSP"
    ),
    sys!(
        "psvita",
        "PlayStation Vita",
        "Sony",
        "Sony - PlayStation Vita",
        "Sony_-_PlayStation_Vita",
        ["psvita", "vita", "ps vita"],
        ["vpk", "zip", "psvita"],
        "Sony Playstation Vita"
    ),
    // ---------------- NEC ----------------
    sys!(
        "pcengine",
        "PC Engine / TurboGrafx-16",
        "NEC",
        "NEC - PC Engine - TurboGrafx 16",
        "NEC_-_PC_Engine_-_TurboGrafx_16",
        ["pcengine", "tg16", "turbografx16", "turbografx-16", "pce"],
        ["pce", "zip", "7z"],
        "NEC TurboGrafx-16"
    ),
    sys!(
        "pcenginecd",
        "PC Engine CD / TurboGrafx-CD",
        "NEC",
        "NEC - PC Engine CD - TurboGrafx-CD",
        "NEC_-_PC_Engine_CD_-_TurboGrafx-CD",
        ["pcenginecd", "tg-cd", "tgcd", "turbografxcd", "pcecd"],
        ["chd", "cue", "ccd", "m3u", "pce"],
        "NEC TurboGrafx-CD"
    ),
    sys!(
        "supergrafx",
        "PC Engine SuperGrafx",
        "NEC",
        "NEC - PC Engine SuperGrafx",
        "NEC_-_PC_Engine_SuperGrafx",
        ["supergrafx", "sgx"],
        ["sgx", "pce", "zip", "7z"],
        "PC Engine SuperGrafx"
    ),
    sys!(
        "pcfx",
        "PC-FX",
        "NEC",
        "NEC - PC-FX",
        "NEC_-_PC-FX",
        ["pcfx", "pc-fx"],
        ["chd", "cue", "ccd", "m3u"],
        "NEC PC-FX"
    ),
    sys!(
        "pc98",
        "PC-98",
        "NEC",
        "NEC - PC-98",
        "NEC_-_PC-98",
        ["pc98", "pc-98"],
        ["d98", "hdi", "fdi", "hdm", "zip", "7z"],
        "NEC PC-9801"
    ),
    // ---------------- SNK ----------------
    sys!(
        "neogeo",
        "Neo Geo",
        "SNK",
        "SNK - Neo Geo",
        "SNK_-_Neo_Geo",
        ["neogeo", "neo geo", "aes", "mvs"],
        ["zip", "7z"],
        "SNK Neo Geo AES"
    ),
    sys!(
        "neogeocd",
        "Neo Geo CD",
        "SNK",
        "SNK - Neo Geo CD",
        "SNK_-_Neo_Geo_CD",
        ["neogeocd", "neo geo cd", "neogeocdjp"],
        ["chd", "cue", "m3u"],
        "SNK Neo Geo CD"
    ),
    sys!(
        "ngp",
        "Neo Geo Pocket",
        "SNK",
        "SNK - Neo Geo Pocket",
        "SNK_-_Neo_Geo_Pocket",
        ["ngp", "neogeopocket"],
        ["ngp", "zip", "7z"],
        "SNK Neo Geo Pocket"
    ),
    sys!(
        "ngpc",
        "Neo Geo Pocket Color",
        "SNK",
        "SNK - Neo Geo Pocket Color",
        "SNK_-_Neo_Geo_Pocket_Color",
        ["ngpc", "neogeopocketcolor"],
        ["ngc", "ngpc", "zip", "7z"],
        "SNK Neo Geo Pocket Color"
    ),
    // ---------------- Atari ----------------
    sys!(
        "atari2600",
        "Atari 2600",
        "Atari",
        "Atari - 2600",
        "Atari_-_2600",
        ["atari2600", "2600", "a2600"],
        ["a26", "bin", "zip", "7z"],
        "Atari 2600"
    ),
    sys!(
        "atari5200",
        "Atari 5200",
        "Atari",
        "Atari - 5200",
        "Atari_-_5200",
        ["atari5200", "5200", "a5200"],
        ["a52", "bin", "car", "zip", "7z"],
        "Atari 5200"
    ),
    sys!(
        "atari7800",
        "Atari 7800",
        "Atari",
        "Atari - 7800",
        "Atari_-_7800",
        ["atari7800", "7800", "a7800"],
        ["a78", "bin", "zip", "7z"],
        "Atari 7800"
    ),
    sys!(
        "atari800",
        "Atari 8-bit",
        "Atari",
        "Atari - 8-bit Family",
        "Atari_-_8-bit_Family",
        ["atari800", "atari8bit", "atarixe"],
        ["atr", "xex", "xfd", "car", "cas", "zip", "7z"],
        "Atari 800"
    ),
    sys!(
        "lynx",
        "Atari Lynx",
        "Atari",
        "Atari - Lynx",
        "Atari_-_Lynx",
        ["atarilynx", "lynx"],
        ["lnx", "lyx", "o", "zip", "7z"],
        "Atari Lynx"
    ),
    sys!(
        "jaguar",
        "Atari Jaguar",
        "Atari",
        "Atari - Jaguar",
        "Atari_-_Jaguar",
        ["atarijaguar", "jaguar"],
        ["j64", "jag", "rom", "abs", "cof", "zip", "7z"],
        "Atari Jaguar"
    ),
    sys!(
        "atarist",
        "Atari ST",
        "Atari",
        "Atari - ST",
        "Atari_-_ST",
        ["atarist", "st"],
        ["st", "msa", "stx", "dim", "ipf", "zip", "7z"],
        "Atari ST"
    ),
    // ---------------- Computers ----------------
    sys!(
        "c64",
        "Commodore 64",
        "Commodore",
        "Commodore - 64",
        "Commodore_-_64",
        ["c64", "commodore64"],
        ["d64", "t64", "prg", "crt", "tap", "g64", "x64", "m3u", "zip", "7z"],
        "Commodore 64"
    ),
    sys!(
        "vic20",
        "Commodore VIC-20",
        "Commodore",
        "Commodore - VIC-20",
        "Commodore_-_VIC-20",
        ["vic20", "vic-20"],
        ["20", "40", "60", "a0", "b0", "crt", "d64", "prg", "tap", "zip"],
        "Commodore VIC-20"
    ),
    sys!(
        "amiga",
        "Amiga",
        "Commodore",
        "Commodore - Amiga",
        "Commodore_-_Amiga",
        ["amiga", "amiga500", "amiga1200", "amiga600"],
        ["adf", "adz", "dms", "ipf", "lha", "hdf", "m3u", "zip", "7z"],
        "Commodore Amiga"
    ),
    sys!(
        "amigacd32",
        "Amiga CD32",
        "Commodore",
        "Commodore - CD32",
        "Commodore_-_CD32",
        ["amigacd32", "cd32"],
        ["chd", "cue", "iso", "m3u"],
        "Commodore Amiga CD32"
    ),
    sys!(
        "msx",
        "MSX",
        "Microsoft",
        "Microsoft - MSX",
        "Microsoft_-_MSX",
        ["msx", "msx1"],
        ["rom", "mx1", "mx2", "dsk", "cas", "zip", "7z"],
        "Microsoft MSX"
    ),
    sys!(
        "msx2",
        "MSX2",
        "Microsoft",
        "Microsoft - MSX2",
        "Microsoft_-_MSX2",
        ["msx2"],
        ["rom", "mx2", "dsk", "cas", "zip", "7z"],
        "Microsoft MSX2"
    ),
    sys!(
        "zxspectrum",
        "ZX Spectrum",
        "Sinclair",
        "Sinclair - ZX Spectrum",
        "Sinclair_-_ZX_Spectrum",
        ["zxspectrum", "spectrum", "zx spectrum"],
        ["tzx", "tap", "z80", "sna", "dsk", "scl", "trd", "zip", "7z"],
        "Sinclair ZX Spectrum"
    ),
    sys!(
        "amstradcpc",
        "Amstrad CPC",
        "Amstrad",
        "Amstrad - CPC",
        "Amstrad_-_CPC",
        ["amstradcpc", "cpc"],
        ["dsk", "sna", "cdt", "voc", "m3u", "zip", "7z"],
        "Amstrad CPC"
    ),
    sys!(
        "x68000",
        "Sharp X68000",
        "Sharp",
        "Sharp - X68000",
        "Sharp_-_X68000",
        ["x68000"],
        ["dim", "hdf", "2hd", "xdf", "m3u", "zip", "7z"],
        "Sharp X68000"
    ),
    sys!(
        "dos",
        "DOS",
        "PC",
        "DOS",
        "DOS",
        ["dos", "pc", "msdos"],
        ["exe", "com", "bat", "dosz", "zip", "7z", "conf"],
        "MS-DOS"
    ),
    sys!(
        "scummvm",
        "ScummVM",
        "PC",
        "ScummVM",
        "ScummVM",
        ["scummvm"],
        ["scummvm", "svm", "zip"],
        "ScummVM"
    ),
    // ---------------- Other consoles ----------------
    sys!(
        "colecovision",
        "ColecoVision",
        "Coleco",
        "Coleco - ColecoVision",
        "Coleco_-_ColecoVision",
        ["colecovision", "coleco"],
        ["col", "rom", "bin", "zip", "7z"],
        "ColecoVision"
    ),
    sys!(
        "intellivision",
        "Intellivision",
        "Mattel",
        "Mattel - Intellivision",
        "Mattel_-_Intellivision",
        ["intellivision", "intv"],
        ["int", "rom", "bin", "zip", "7z"],
        "Mattel Intellivision"
    ),
    sys!(
        "vectrex",
        "Vectrex",
        "GCE",
        "GCE - Vectrex",
        "GCE_-_Vectrex",
        ["vectrex"],
        ["vec", "gam", "bin", "zip", "7z"],
        "GCE Vectrex"
    ),
    sys!(
        "odyssey2",
        "Odyssey² / Videopac",
        "Magnavox",
        "Magnavox - Odyssey2",
        "Magnavox_-_Odyssey2",
        ["odyssey2", "videopac", "o2em"],
        ["bin", "zip", "7z"],
        "Magnavox Odyssey 2"
    ),
    sys!(
        "channelf",
        "Channel F",
        "Fairchild",
        "Fairchild - Channel F",
        "Fairchild_-_Channel_F",
        ["channelf", "channel f"],
        ["chf", "bin", "zip", "7z"],
        "Fairchild Channel F"
    ),
    sys!(
        "wonderswan",
        "WonderSwan",
        "Bandai",
        "Bandai - WonderSwan",
        "Bandai_-_WonderSwan",
        ["wonderswan", "ws"],
        ["ws", "zip", "7z"],
        "WonderSwan"
    ),
    sys!(
        "wonderswancolor",
        "WonderSwan Color",
        "Bandai",
        "Bandai - WonderSwan Color",
        "Bandai_-_WonderSwan_Color",
        ["wonderswancolor", "wsc"],
        ["wsc", "zip", "7z"],
        "WonderSwan Color"
    ),
    sys!(
        "3do",
        "3DO",
        "Panasonic",
        "The 3DO Company - 3DO",
        "The_3DO_Company_-_3DO",
        ["3do"],
        ["chd", "cue", "iso", "m3u"],
        "3DO Interactive Multiplayer"
    ),
    sys!(
        "cdi",
        "Philips CD-i",
        "Philips",
        "Philips - CD-i",
        "Philips_-_CD-i",
        ["cdi", "cd-i"],
        ["chd", "cue", "iso"],
        "Philips CD-i"
    ),
    sys!(
        "supervision",
        "Watara Supervision",
        "Watara",
        "Watara - Supervision",
        "Watara_-_Supervision",
        ["supervision"],
        ["sv", "bin", "zip", "7z"],
        "Watara Supervision"
    ),
    sys!(
        "gamecom",
        "Game.com",
        "Tiger",
        "Tiger - Game.com",
        "Tiger_-_Game.com",
        ["gamecom", "game.com"],
        ["tgc", "bin", "zip"],
        "Tiger Game.com"
    ),
    sys!(
        "arduboy",
        "Arduboy",
        "Arduboy",
        "Arduboy Inc - Arduboy",
        "Arduboy_Inc_-_Arduboy",
        ["arduboy"],
        ["hex", "arduboy", "zip"],
        "Arduboy"
    ),
    sys!(
        "xbox",
        "Xbox",
        "Microsoft",
        "Microsoft - Xbox",
        "Microsoft_-_Xbox",
        ["xbox"],
        ["iso", "xiso"],
        "Microsoft Xbox"
    ),
    // ---------------- Arcade ----------------
    sys!(
        "fbneo",
        "FinalBurn Neo (Arcade)",
        "Arcade",
        "FBNeo - Arcade Games",
        "FBNeo_-_Arcade_Games",
        ["fbneo", "fba", "finalburn"],
        ["zip", "7z"],
        "Arcade"
    ),
    sys!(
        "mame",
        "MAME (Arcade)",
        "Arcade",
        "MAME",
        "MAME",
        ["mame", "arcade", "mame2003", "mame2003-plus", "mame2010"],
        ["zip", "7z", "chd"],
        "Arcade"
    ),
];

/// Find a system by its id.
pub fn by_id(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// Guess the system for a folder name (case-insensitive, ignores separators).
pub fn detect_from_folder(folder: &str) -> Option<&'static System> {
    let key = folder_key(folder);
    if key.is_empty() {
        return None;
    }
    SYSTEMS
        .iter()
        .find(|s| s.folders.iter().any(|f| folder_key(f) == key) || folder_key(s.id) == key)
        .or_else(|| {
            SYSTEMS
                .iter()
                .find(|s| folder_key(s.libretro) == key || folder_key(s.name) == key)
        })
}

fn folder_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = SYSTEMS.iter().map(|s| s.id).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len());
    }

    #[test]
    fn detects_common_folders() {
        assert_eq!(detect_from_folder("snes").unwrap().id, "snes");
        assert_eq!(detect_from_folder("Genesis").unwrap().id, "megadrive");
        assert_eq!(detect_from_folder("PSX").unwrap().id, "psx");
        assert_eq!(
            detect_from_folder("Nintendo - Game Boy Advance")
                .unwrap()
                .id,
            "gba"
        );
        assert_eq!(detect_from_folder("tg-cd").unwrap().id, "pcenginecd");
        assert!(detect_from_folder("random stuff").is_none());
    }
}
