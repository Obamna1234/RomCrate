# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's
private reporting instead: **Security → Report a vulnerability** on this
repository. You'll get a reply within 7 days.

## What RomCrate does to keep you safe

- **No accounts and no telemetry.** Nothing identifies you to any server.
- **Network allow-list.** The HTTP client only connects over HTTPS to
  `raw.githubusercontent.com`, `api.github.com`, `thumbnails.libretro.com`,
  `gamesdb.launchbox-app.com`, `images.launchbox-app.com` and `*.screenscraper.fr`, and refuses redirects to any other host.
- **Untrusted downloads are sanitised.** Images are decoded with strict size
  and memory limits and re-encoded as new PNG files; videos must be MP4 and
  manuals PDF; every download is size-capped while streaming.
- **Path safety.** Every file name is sanitised (no `..`, separators, reserved
  Windows names); output is always inside the folders the user selected.
- **Atomic writes and backups.** Files are written to a temporary name and
  renamed; `gamelist.xml` is backed up before it is modified.
- **Least-privilege desktop app.** The Tauri window has a strict Content
  Security Policy and can only open a folder picker. Saving reports and opening
  folders go through app commands that only accept native dialog choices or
  folders the user picked; previews are limited to files RomCrate produced.
- **No secrets in logs.** Network errors never include URLs, and anything that
  looks like a credential is redacted.
- **External programs.** The only program RomCrate can run is a
  `realesrgan-ncnn-vulkan` the user installed and selected.
- **Secrets.** RomCrate's ScreenScraper developer credentials are injected at
  build time from GitHub Actions secrets and are never committed. A user's
  optional ScreenScraper login is kept in memory only.
- **No bundled third-party binaries, BIOS, firmware or games.**

## Audits

See [docs/SECURITY-REVIEW.md](docs/SECURITY-REVIEW.md). CI runs `cargo audit`
and `npm audit` on every push.

## Supported versions

Only the latest release receives security fixes.
