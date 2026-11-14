# Security review — v0.1.0 (14 November 2026)

A review and penetration test of ShelfArt before its first release. The rule
for this pass: **no fix ships unless the full test suite still passes**, so
hardening can't break working features.

## Method

1. **Locked in current behaviour first.** 8 end-to-end tests run the whole
   pipeline offline for every launcher layout (ES-DE, Pegasus, EmulationStation,
   RetroArch, LaunchBox, plain folders), plus re-runs, overwrite and size presets.
2. **Checked every supported system** against the live artwork repositories and
   game databases.
3. **Attacked it** with 9 hostile-input test groups (below).
4. **Reviewed** the network client, file handling, parsers, desktop-app
   permissions and the GitHub build pipeline.
5. **Fixed findings one at a time**, re-running the full test suite after each, then a
   live scrape, a release build and a scripted walk through the app.

## Penetration tests (all pass)

| Attack | Result |
|---|---|
| 20,000 random/garbage game names (brackets, separators, null bytes, Unicode) | No crashes; names can never contain path separators |
| Path traversal in names (`../../etc/passwd`, `..\..\Windows`, `C:\`, `CON`, absolute paths) | Always stays inside the chosen folder |
| Malicious artwork listings (`..%2F..%2Fevil.png`) | Dropped |
| Garbage game database files | No crashes |
| Random/forged disc images, zips, ROM headers (40 files × 7 formats) | No crashes |
| Cue sheet pointing at `../../../../etc/passwd` | Not followed |
| Forged PS3 update with the right header and a fake "4.93" version | Rejected (checksum must match Sony's) |
| Image bombs (60000×60000 PNG ≈ 14 GB), truncated/garbage PNG & JPEG, HTML error pages | Rejected without allocating |
| XML "billion laughs" and external-entity (XXE) `gamelist.xml` | Refused safely; no local files read; original left untouched |
| Broken `gamelist.xml` | Left exactly as it was |
| Markup in game names | Escaped |
| Command injection into `metadata.pegasus.txt` via a file name with a newline | Blocked |
| Desktop app: open `/etc`, preview `/etc/hostname`, write to an arbitrary path, call the file-opener or save-dialog plugins directly, start a second scrape | All refused |

## Findings and fixes

| # | Severity | Finding | Fix |
|---|---|---|---|
| 1 | Medium | Network error messages included the full request URL, which for ScreenScraper contains the app's developer password and a user's optional password — these could appear in the activity log or a saved report. | URLs stripped from all network errors; any `devpassword=`, `sspassword=`, `devid=`, `ssid=` value is redacted. Tested. |
| 2 | Medium | The web page could ask the app to write a `.txt` file to any path. | The save dialog now runs natively in the app; the page only supplies the text. Old command removed. |
| 3 | Medium | The web page could ask the system to open any path. | Replaced with an app command that only opens folders the user chose this session; the plugin permission was removed from the page. |
| 4 | Medium | The optional AI upscaler would run whatever program path it was given, using a predictable temp folder. | Only a program named `realesrgan-ncnn-vulkan` is ever run; private random temp folder; output size capped. |
| 5 | Low | ROM hashing loaded up to 96 MB per file into memory (×8 in parallel). | Streams in 1 MB chunks. |
| 6 | Low | "Already running" check could be bypassed after a cancelled scrape. | Dedicated atomic running flag. Tested. |
| 7 | Low | Release workflow pasted the version input straight into a shell script and gave every job write access; third-party actions were referenced by movable tags. | Inputs passed via environment; no permissions by default, per-job write only where needed; actions pinned to commit SHAs; `cargo audit` + `npm audit` added to CI. |

## Bugs found by the all-systems test (fixed)

- 6 artwork repositories (NAOMI, NAOMI 2, Atari 8-bit, Amiga CD32, CD-i, Arduboy) use the `main` branch, not `master` → downloads now use each repository's default branch.
- Atari 8-bit repository name was wrong (`Atari_-_8-bit`).
- 4 systems (Neo Geo, PC-98, ZX Spectrum, Amstrad CPC) have no No-Intro database → they now match by name instead of failing a download.
- An artwork output folder inside the ROM folder (e.g. `downloaded_media/psx`) showed up as an empty "system" → output folders and empty systems are ignored.

## Caught by CI after the review (first GitHub run, 14 Nov 2026)

The audit job in CI flagged six newly published advisories in two libraries
added after this review: `zip` 2.6 (RUSTSEC-2026-0367/0368/0369/0429 — all in
`ZipArchive::extract`, which ShelfArt doesn't use; it has its own checked
extraction) and `quick-xml` 0.37 (RUSTSEC-2026-0194/0195 — quadratic attribute
check, `NsReader` memory use). Both were upgraded (`zip` 9.5, `quick-xml` 0.42)
anyway; entity handling was rewritten to resolve only the five standard
entities and numeric references. `cargo audit` is back to 0 vulnerabilities.

## Dependency audit

- **Rust (`cargo audit`, 521 crates): 0 vulnerabilities.** 5 informational warnings, all inside Tauri's Linux GTK/D-Bus stack (`glib` 0.18, `zbus`/`zvariant`, `proc-macro-error`): marked unmaintained, or an unsound `glib` iterator ShelfArt never calls. They'll clear when Tauri updates its Linux dependencies; Dependabot is set up to pick that up.
- **npm (`npm audit`): 0 vulnerabilities.**

## Added after the review: zipped folders

Unzipping untrusted archives is a classic attack surface, so it shipped with its own tests:

| Attack | Result |
|---|---|
| Zip slip (`../escaped.txt`, `/abs.txt`) | Entries outside the target folder are skipped |
| Zip bomb (64 MB of zeros, >1000:1) | Refused while streaming; partial file removed |
| Zip that would overwrite an existing file | Refused; the user's file is untouched |
| Fake `.zip` that isn't a zip | Reported, nothing written |
| Not enough disk space | Checked before anything is written |

## Added after the review: LaunchBox Games Database

| Risk | Handling |
|---|---|
| ~200 MB download filling memory | Streamed straight to disk, 1 GB cap, atomic rename |
| Hostile or broken XML (entity bombs, truncation) | Streaming parser with no external entities; tested — error or empty result, no hang |
| Image file names used as paths | Names containing `/`, `\` or `..` are refused; cached under sanitised names |
| Overwriting the user's own game info | Fields are only filled when empty unless "replace existing" is on; gamelist.xml backed up |

## Not covered yet

- Windows and macOS builds have not been run yet. CI runs every test on both as soon as the repository exists.
- Installers are not code-signed (needs paid certificates).
- The LaunchBox download itself couldn't be fetched from the build environment (network allow-list); parsing is tested against the published format and RomM's fixtures, and the live download will be exercised by the first real run.
- The ScreenScraper extras are only partly tested until developer codes exist: URL redaction, host allow-list and file validation are tested, live responses are not.
- Conversion, BIOS extraction and sorting don't exist yet; each will get the same test-first review when built.
