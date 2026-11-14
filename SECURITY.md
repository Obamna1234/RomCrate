# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's
private reporting instead: **Security → Report a vulnerability** on this
repository. You'll get a reply within 7 days.

## What ShelfArt does to keep you safe

- **No accounts and no telemetry.** Nothing identifies you to any server.
- **Network allow-list.** The HTTP client only connects over HTTPS to
  `raw.githubusercontent.com`, `api.github.com`, `thumbnails.libretro.com`
  and `*.screenscraper.fr`, and refuses redirects to any other host.
- **Untrusted downloads are sanitised.** Images are decoded with strict size
  and memory limits and re-encoded as new PNG files; videos must be MP4 and
  manuals PDF; every download is size-capped while streaming.
- **Path safety.** Every file name is sanitised (no `..`, separators, reserved
  Windows names); output is always inside the folders the user selected.
- **Atomic writes and backups.** Files are written to a temporary name and
  renamed; `gamelist.xml` is backed up before it is modified.
- **Least-privilege desktop app.** The Tauri window has a strict Content
  Security Policy and only the permissions it needs (open/save dialogs, open a
  folder). Previews are limited to files ShelfArt produced in the session.
- **Secrets.** ShelfArt's ScreenScraper developer credentials are injected at
  build time from GitHub Actions secrets and are never committed. A user's
  optional ScreenScraper login is kept in memory only.
- **No bundled third-party binaries, BIOS, firmware or games.**

## Supported versions

Only the latest release receives security fixes.
