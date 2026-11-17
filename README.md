<p align="center">
  <img src="assets/icon-source.png" width="112" alt="RomCrate icon">
</p>

<h1 align="center">RomCrate</h1>

<p align="center">
  <b>Get your games ready for your handheld — sorted, checked, with artwork. No account, no tracking.</b><br>
  Sorts any messy folder into console folders, checks for broken downloads, and adds box art, 3D boxes, screenshots, logos, videos and manuals<br>
  exactly where Pegasus, ES-DE, EmulationStation, RetroArch or LaunchBox expect them.
</p>

> **RomCrate was called ShelfArt until v0.2.0.** Same project, new name now that it does more than artwork.

<p align="center">
  <a href="../../releases/latest"><img alt="Download" src="https://img.shields.io/github/v/release/Obamna1234/RomCrate?label=download&style=for-the-badge"></a>
  <a href="../../actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/Obamna1234/RomCrate/ci.yml?branch=main&style=for-the-badge&label=tests"></a>
  <img alt="Platforms" src="https://img.shields.io/badge/Windows%20%7C%20macOS%20%7C%20Linux-555?style=for-the-badge">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-555?style=for-the-badge">
</p>

---

RomCrate is a free, open-source alternative to account-based scrapers. Point it at your ROM folder, pick your launcher and theme style, press **Start**. That's it.

![Choosing artwork](docs/screenshot-artwork.png)

## Why RomCrate

| | Typical scrapers | RomCrate |
|---|---|---|
| Account | Required (email + password) | **None** |
| Tracking | Every request tied to your username | **Anonymous file downloads only** |
| Daily limits | Yes, by account level | **No limits** on the free collection |
| Renamed / odd file names | Often missed | **Identified by contents** (checksums & disc serials) |
| 3D boxes when none exist | — | **Rendered on your PC** from the cover |

## Features

- **Made for your device.** Pick what you play on (Odin 2 family, Retroid Pocket 5, Anbernic handhelds, Steam Deck, PC/TV or "not sure") and RomCrate chooses the frontend layout, launch lines and artwork size for it — and warns about consoles it can't run. Then either **set up everything** step by step, or go **custom** and run just one task (check files, sort, artwork, launch lines). Everything it does is written to an activity log.
- **Pegasus launch lines for Android handhelds.** Pick your emulator per console and RomCrate writes the `launch:` line Pegasus needs — only lines tested on a real device are switched on (ARMSX2 Play Store for PS2 so far); others are added switched off with a note. Your own Pegasus files are only changed if you ask, with a `.bak` backup.
- **Check my files.** Catches unfinished downloads, damaged zips, PlayStation Store packages, damaged N64 ROMs, disc formats that need converting, missing `.cue` files, PS3 games, and PC games whose disc copy protection won't run under Wine/GameNative (information only).
- **Import & sort any messy folder.** Point RomCrate at a Downloads folder (or a `.zip`) and it puts every game into `ROMs/<console>/`, working out the console from *inside* the file — cartridge headers, GameCube/Wii and Sega disc signatures, PlayStation `SYSTEM.CNF`, PSP and PC discs — then file type, folder name and the official game lists. You see a preview first; it copies by default (or moves), checks every copy byte-for-byte with a CRC, keeps cue/bin and multi-disc `.m3u` sets together, never overwrites, and skips BIOS files, broken sets and non-game files.
- **Recognises games by their contents.** Cartridge games are matched by checksum against No-Intro; disc games (PS1, PS2, PSP, GameCube, Wii) by the serial number read from inside the image (Redump). `my crash backup.iso` still becomes *Crash Bandicoot (USA)*.
- **Zipped folders welcome.** Pick a `.zip` of your games folder and RomCrate unzips it next to the zip (safely — no zip-slip, zip-bomb or overwrite) and carries on.
- **Works with what you already have.** If a system already uses Skraper's Pegasus layout (`media/box2dfront/…`), RomCrate adds to it and never overwrites your existing art.
- **Finds odd dumps too:** copier-headered SNES ROMs, byte-swapped N64 ROMs (`.v64`/`.n64`), and the title stored inside SNES, N64, GBA, Game Boy and Mega Drive headers as a last resort.
- **Game info too:** descriptions, genre, developer, publisher, release date, players and rating go into `gamelist.xml` (EmulationStation and ES-DE) and new Pegasus metadata files — without ever replacing what you wrote yourself.
- **Every common art type:** box art, 3D boxes, back covers, cartridges/discs, screenshots, title screens, wheels/logos, fan art, mix images, video snaps and PDF manuals.
- **Made for your launcher:**

  | Launcher | Where art goes |
  |---|---|
  | Pegasus | Skraper style `<system>/media/box2dfront/<game>.png`, `wheel/`, `screenshot/`… or per-game `<system>/media/<game>/boxFront.png` — matches what you already use |
  | ES-DE | `downloaded_media/<system>/covers`, `3dboxes`, `screenshots`, `marquees`, `miximages`, `videos`, `manuals`… |
  | EmulationStation (Batocera, RetroPie, Recalbox, Knulli, ArkOS) | `<system>/images` + `gamelist.xml` (original backed up) |
  | RetroArch | `thumbnails/<system>/Named_Boxarts`, `Named_Snaps`, `Named_Titles`, `Named_Logos` (+ playlists) |
  | LaunchBox / Big Box | `Images/<platform>/Box - Front`, `Box - 3D`, `Clear Logo`…, `Videos`, `Manuals` |
  | Anything else | Plain folders per system and type |

- **Send to your Android handheld** over USB (internal storage or SD card, per console) — nothing on the handheld is overwritten, and every large file is checked after copying. It can also fetch artwork for games that are only on the handheld.
- **Game settings for the handheld's emulators**: sharp, full-speed settings per console and per game, with a backup of every file and Undo. **Picture styles** per console or per game: Sharp pixels, Smooth HD, CRT TV, CRT TV (best quality) or no shader. **Play modes** switch input-delay settings for the handheld screen, a TV or a Bluetooth controller.
- **Theme presets** pre-select what your style of theme shows (showcase, cover grid, 3D shelves, mix image…).
- **Sharp at any size.** Pixel-art screenshots are scaled crisply; box art is enlarged smoothly with light sharpening. Optional AI upscaling with [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN) if you have it installed.
- **Region preference**, skip-existing or overwrite, compact → 4K size presets.
- **75+ systems** — Nintendo, Sega, Sony, NEC, SNK, Atari, Commodore, MSX, arcade and more.
- **Command-line version** (`romcrate-cli`) for scripts, servers and handheld PCs.

## Download

Grab the installer for your system from **[Releases](../../releases/latest)**:

| System | File |
|---|---|
| Windows 10 / 11 | `RomCrate_x.y.z_x64-setup.exe` (or `.msi`) |
| macOS (Apple Silicon & Intel) | `RomCrate_x.y.z_universal.dmg` |
| Linux (Ubuntu 22.04 / Zorin 17 / Mint 21 or newer) | `.AppImage`, `.deb` or `.rpm` |
| Any 64-bit Linux, including older ones | `romcrate-cli-…-x86_64-unknown-linux-musl` (command line) |

> Builds are not code-signed yet. Windows SmartScreen may say "unknown publisher" — click **More info → Run anyway**. On macOS, right-click the app → **Open** the first time.

## Where the artwork comes from

| Art | Source | Account? |
|---|---|---|
| Box art, screenshots, title screens, logos | [libretro-thumbnails](https://github.com/libretro-thumbnails) — the free collection RetroArch uses | No |
| Game identification | [libretro-database](https://github.com/libretro/libretro-database) (No-Intro & Redump data) | No |
| 3D boxes, back covers, cartridges, fan art, game info (descriptions, genre, developer, release date, players, rating) | [LaunchBox Games Database](https://gamesdb.launchbox-app.com) — the free daily download LaunchBox publishes for any app to use | No |
| Mix images, fallback 3D boxes | Made by RomCrate on your computer | No |
| Videos, manuals (and anything the others miss) | [ScreenScraper](https://www.screenscraper.fr) in guest mode (RomCrate's own developer registration) | No — optional login only for extra speed |

If one source is unreachable RomCrate tries the other mirror, and everything already downloaded is kept in a local cache.

## Privacy & safety

- **No account, no telemetry, no analytics.**
- **Allow-listed connections only:** GitHub, libretro, the LaunchBox games database and screenscraper.fr over HTTPS. Redirects anywhere else are refused.
- **Clean files:** every downloaded image is fully decoded and re-saved as a fresh PNG; videos and manuals must be real MP4/PDF files; all downloads are size-capped.
- **Your files are safe:** RomCrate never deletes or modifies your games, never writes outside the folders you choose, and backs up `gamelist.xml` before changing it.
- An optional ScreenScraper login is kept in memory for the session only — never saved to disk.

See [SECURITY.md](SECURITY.md) to report a vulnerability, and the [security review](docs/SECURITY-REVIEW.md) for the latest audit and penetration-test results.

## Roadmap — one-click setup

RomCrate is growing into a one-click setup tool for emulation handhelds (AYN Odin, Retroid Pocket…): pick a folder of your own games, press Start, and get a ready-to-copy `ROMs` folder.

- [x] Artwork scraper with launcher & theme layouts
- [x] Game identification by checksum and disc serial
- [x] Verification of Sony's official PS3 system software file
- [x] Import & sort any folder layout into `ROMs/<console>`, with free-space check
- [ ] Compress disc games (PS1/PS2 → CHD, GameCube/Wii → RVZ) with verification before and after — originals never touched
- [ ] PS1/PS2 BIOS from your own copy of the official PS3 update (see below)
- [x] Pegasus `metadata.pegasus.txt` with tested launch lines (ARMSX2 so far) and "Check my files"
- [ ] On-device setup checklist (AYN Odin 2 Portal first)
- [ ] Android version of RomCrate (runs on the handheld itself)

Third-party tools (chdman, DolphinTool, RPCS3) will be downloaded from their official sites when needed — they are never bundled. **RomCrate never includes or downloads games, BIOS files or firmware.**

### PS3 system software (for the BIOS step)

| | |
|---|---|
| Version | **4.93** |
| Released | **18 March 2026** |
| Official download | <https://www.playstation.com/en-us/support/hardware/ps3/system-software/> |
| Official file checksum (MD5) | `a2b60b6ac1d2e49e230144345616927c` |

You download `PS3UPDAT.PUP` yourself from Sony's page (accepting Sony's terms). RomCrate checks your file against the checksum Sony publishes in its own download link, so you know it's genuine and unmodified. The BIOS extraction approach is based on the community *PS BIOS Claim Tool*.

## Building from source

Requirements: [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 20+, and on Linux the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev        # run the desktop app
npm run tauri build      # build installers
cargo test --workspace   # run the tests
cargo run -p romcrate-cli -- --help
```

ScreenScraper extras are enabled when the build has `SS_DEVID` and `SS_DEVPASSWORD` environment variables (RomCrate's developer registration, stored as GitHub Actions secrets). Builds without them work fully with the free sources.

### Releasing

Go to **Actions → Release → Run workflow**, type the new version (e.g. `0.2.0`) and run it. The workflow bumps the version everywhere, tags it, builds Windows, macOS and Linux installers plus CLI binaries, and publishes the GitHub release.

## Credits

- Artwork and game info: [libretro-thumbnails](https://github.com/libretro-thumbnails) contributors, the [LaunchBox Games Database](https://gamesdb.launchbox-app.com) community and [ScreenScraper](https://www.screenscraper.fr). All artwork belongs to its respective owners.
- Game data: [No-Intro](https://no-intro.org), [Redump](http://redump.org), via [libretro-database](https://github.com/libretro/libretro-database).
- Built with [Tauri](https://tauri.app) and Rust.

## License

[MIT](LICENSE)
