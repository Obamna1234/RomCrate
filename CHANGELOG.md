# Changelog

## 0.7.0 — Picture styles

- **Choose how each console looks** (Game settings → Each console, SNES and NES): **Sharp pixels** (the default, as before), **Smooth HD** (ScaleFX: clean outlines, like an HD remaster — great for Donkey Kong Country and Yoshi's Island), **CRT TV** (crt-easymode — great on a big TV), **CRT TV (best quality)** (crt-guest-advanced, heavier, Vulkan only) or **No shader**. Shader paths checked against libretro's slang-shaders and glsl-shaders. Only Sharp is marked tested on a real device so far.
- With Smooth HD or a CRT style, RetroArch's own smoothing (`video_smooth`) is turned off for that console, so the shader isn't blurred twice.
- **A different style for one game**: "Different style for one game…" under the console lists its games on the handheld. RomCrate writes `<Core>/<game>.slangp|.glslp` (and `.cfg`) with its marker, and "Undo RomCrate's changes" removes them.
- **Your own shader presets are never replaced** — for a console or a single game. (Before, RomCrate replaced a console preset of yours after backing it up.)
- If RetroArch's shaders aren't downloaded yet, RomCrate says how (Online Updater → Update Shaders) instead of writing a broken preset. On the OpenGL driver, the Vulkan-only style is shown but can't be picked.

## 0.6.0 — Play modes, device check, keep yours or use RomCrate's

- **Play modes** (Game settings → Play mode): Handheld / TV by cable / TV streamed × built-in / wired / Bluetooth controls. Sets RetroArch run-ahead (1 frame, 2 for streaming or Bluetooth) and automatic frame delay for SNES, NES, Game Boy and GBA; steps for what no setting can fix (TV Game mode, streaming delay).
- **Device check:** shows which system the handheld runs (e.g. "Stock Android 13", "LineageOS"), and asks "use what RomCrate found, or keep yours" if it looks like a different device. On a system RomCrate doesn't know, settings can't be written until you tick "I understand". Linux handhelds (no USB debugging) are explained.
- **Artwork you already have:** new "Ask me" default — RomCrate shows your picture next to the one it found and you choose per picture; replaced pictures are kept and can be put back. On your PC and on the handheld.
- **Artwork for games that are only on the handheld:** names read over the cable, artwork fetched on the PC, only the new pictures and Pegasus files copied back.
- Use a photo of your own device instead of the drawing.
- Fixed: applying Game settings twice made Undo restore RomCrate's own file instead of removing it.

## 0.5.0 — New look

- Calmer, clearer design: one amber colour for "do this next", larger readable text, line icons, numbered setup steps; the sidebar fits on a laptop screen.
- Long pages split into short parts with Back / Next (Artwork, Launcher); Settings in tabs.
- Device drawings redrawn from each device's real size and control layout, all to the same scale, with sizes in mm.

## 0.4.1 – 0.4.5

- **Send to handheld** over USB debugging: Google's platform-tools downloaded on request and checked; choose internal storage or SD card per console; nothing on the handheld is overwritten; every large file is checked after copying.
- **Game settings** for each console's emulator (RetroArch, Dolphin, PPSSPP; checklists for ARMSX2 and melonDS) with a backup of every file and Undo; **Each game** settings for games that need something different.
- Choose where RomCrate keeps its log and downloads; a live activity log and a "Save problem report" button.
- The owner's tested Pegasus launch lines (N64, DS, Wii, PS2); Check my files false alarms fixed.

## 0.4.0 — Your device, Home, and an activity log

- **"What will you play on?"** On first start you pick your device (AYN Odin 2 Portal / Odin 2 / Odin 2 Mini, Retroid Pocket 5, Anbernic RG556, Anbernic RG35XX/RG40XX family, Steam Deck, PC/TV, or "Not sure — safe defaults"). RomCrate then picks the frontend layout, Android launch lines and artwork size for that screen, and marks consoles the device can't realistically run. Change it any time from the sidebar. Specs and sources are in `data/devices.json`; only the Odin 2 Portal is marked as tested on a real unit.
- **New Home screen with two clear choices:**
  - **Set up everything** (recommended) walks you through sorting, choosing folders, frontend, artwork and launcher files — with a "My games are already sorted" shortcut for people who are already set up.
  - **Custom — pick what to do** runs one task on its own: Check my files, Import & sort, Get artwork, or Pegasus launch lines. Disc compression and the device setup checklist appear there as "soon".
- **Pegasus launch lines on their own:** write or fix launch lines without re-scraping (app task, and `romcrate-cli pegasus --roms <dir>`).
- **Activity log:** everything RomCrate does — games found or not found and why, files skipped, errors — is saved to a daily log on your computer (kept 30 days). Open it from Settings or Home.
- CLI: `romcrate-cli devices`.

## 0.3.1

Found by testing with real game files (6 DS and 5 SNES cartridges — all 11 sorted to the right console, 10 identified and given the right cover):
- Cheat files (`.cht`, `.pnach`…), patches (`.ips`, `.bps`, `.ups`) and scraper game lists (Skraper's `Super_Famicom.dat`, No-Intro/Redump `.dat`) are left alone during import instead of being listed as "console unknown".
- Unfinished `.part` downloads are now shown and refused during import (they were silently skipped).
- Using a short folder name like `ROMs` for the destination no longer hides the free-space check.

## 0.3.0 — RomCrate, Pegasus launch lines and "Check my files"

- **ShelfArt is now RomCrate.** Same app, new name now that it sorts, checks and sets up games as well as artwork. The command-line tool is now `romcrate-cli`. Downloads cached by ShelfArt are reused automatically. Because the app's ID changed, Windows/macOS install it as a new app (you can uninstall ShelfArt) and the app's saved settings start fresh.
- **Pegasus launch lines.** Choose "Android handheld" on the Pegasus step and RomCrate writes the line that tells Pegasus how to start each console's emulator.
  - Commands live in a data file with their source and test status. Only lines tested on a real device are switched on; currently ARMSX2 (Play Store) for PS2, tested on an AYN Odin 2 Portal. RetroArch (SNES, N64), melonDS and PPSSPP are included switched off, with a note asking you to test them.
  - Your existing `metadata.pegasus.txt` files are never changed unless you turn on "Fix launch lines in Pegasus files I already have": then a `.bak` backup is made first and only the launch block is added (or a known-broken one replaced, such as the GitHub-build ARMSX2 line that fails on Play Store installs). Otherwise RomCrate shows you the exact line to add.
  - Warns when an existing file doesn't list `.chd` but the folder has `.chd` games.
  - Consoles split across an SD card and internal storage get the same collection name and launch line in both folders, so Pegasus shows them as one.
  - CLI: `--pegasus-on android`, `--emulator ps2=armsx2-playstore`, `--fix-launch`, and `romcrate-cli emulators`.
- **Check my files.** New button on the Import screen and `romcrate-cli check <folder>`: finds unfinished downloads (`.crdownload`, `.part`…), damaged zips (naming the broken file inside), PlayStation Store `.pkg` files, damaged or modified N64 ROMs (cartridge checksum, any byte order), `.mds/.mdf/.mdx` disc images that need converting, disc `.bin` files without their `.cue`, PS3 games, and PC games with SafeDisc/SecuROM/StarForce disc copy protection that probably won't run under Wine/GameNative (information only — RomCrate never modifies or works around copy protection). Import skips files with problems and says why.

## 0.2.0 — Import & sort

- **New: Import & sort.** Pick any messy folder or `.zip` and ShelfArt sorts the games into `ROMs/<console>/`.
  - Recognises the console from inside the file: NES, SNES, N64, Game Boy/Color/Advance, DS, 3DS, Mega Drive/32X, Master System/Game Gear, Lynx, Atari 7800 headers; GameCube/Wii (incl. WBFS/RVZ/CISO), Saturn, Sega CD, Dreamcast discs; PS1/PS2 (`SYSTEM.CNF`), PSP and PC discs; Dreamcast GD-ROM CHDs. Then file type, the folder it was in, and exact names in the official Redump lists.
  - A PC disc sitting in a "PS2 games" folder still goes to `pc/` (and the preview says why).
  - Keeps `.cue`/`.bin`, `.gdi` and `.m3u` multi-disc sets together; incomplete sets, BIOS files, empty files and non-game files are left alone.
  - Preview first. Copy (default) or move; every copy is read back and CRC-checked before it counts; originals are removed (move mode) only after that. Never overwrites; all-or-nothing per game; free-space check before and during.
  - Reuses console folders you already have ("PS2", "Nintendo 64"…). Optional renaming to official names.
  - App: new **Import & sort games** screen with a console picker for anything it can't work out, and a "Get artwork for these games" button. CLI: `shelfart-cli import`.
- Library: when you have several folders, each console card says which folder it's in.
- The automated real-user simulation now also imports a messy folder (copy, move, re-run, then scrape) on Windows, macOS and Linux, and clicks through the import screen in the app.

## 0.1.4

Found by the new automated "real user" simulation (runs on Windows, macOS and Linux every week):
- Badly named games identified from their contents now show their real title in Pegasus, ES-DE and EmulationStation (e.g. `game disc 1.iso` → Grand Theft Auto III), and on the result cards.
- Empty (0-byte) files are no longer counted as games.
- Titles match regardless of spacing ("Duck Tales" → DuckTales (USA) instead of an odd catalogue entry).
- The window no longer opens partly off-screen on small displays.

## 0.1.3

- New `shelfart-cli-…-x86_64-unknown-linux-musl` download: a fully static command-line build that runs on any 64-bit Linux, including older ones like Ubuntu 20.04/20.10 where the desktop app can't run.


## 0.1.2 — first full release

- Windows, macOS and Linux installers are attached to the release (0.1.0 only had the command-line tools, and the 0.1.1 build was stopped, because of a release-script bug).
- Everything listed under 0.1.0 below.

## 0.1.0 — first release

- Desktop app for Windows, macOS and Linux with a four-step flow: games → launcher & theme → artwork → scrape.
- Account-free artwork from libretro-thumbnails; optional ScreenScraper guest mode for 3D boxes, videos, manuals, back covers, cartridges and fan art.
- Game identification by checksum (No-Intro) and disc serial (Redump) for PS1, PS2, PSP, GameCube and Wii.
- 3D boxes rendered locally from 2D covers; mix images.
- Layouts for Pegasus, ES-DE, EmulationStation (Batocera/RetroPie/…), RetroArch, LaunchBox and plain folders.
- Smart pixel-art-aware upscaling, size presets up to 4K, optional Real-ESRGAN.
- Verification of the official PS3 system software file (4.93).
- `shelfart-cli` command-line tool.
- Zipped games folders are unpacked next to the zip (with zip-slip, zip-bomb, free-space and no-overwrite protection).
- Pegasus: Skraper-style media layout (`media/box2dfront/…`) detected and kept automatically; existing art never overwritten.
- Identification of byte-swapped N64 ROMs, ROM-header titles as a fallback, better matching of "&" titles and short names.
- GameCube/Wii discs matched by region and revision from the disc header; .cue/.gdi sets with missing track files are flagged.
- Extra art types (logo, title, screenshot) never fall back to a fuzzy guess once a game is identified.
- Free LaunchBox Games Database source (no account): 3D boxes, back covers, cartridges, fan art and game info (descriptions, genre, developer, publisher, release date, players, rating) written to gamelist.xml, ES-DE gamelists and new Pegasus metadata; hand edits are kept. Downloaded at most weekly, streamed to disk and parsed per platform.
- Security review and penetration test before release (see docs/SECURITY-REVIEW.md); 51 automated tests.
