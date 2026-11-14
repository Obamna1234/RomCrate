# Changelog

## 0.1.1 — first full release

- Windows, macOS and Linux installers are attached to the release (0.1.0 only had the command-line tools because of a release-script bug).
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
