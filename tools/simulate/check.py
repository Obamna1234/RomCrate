"""Check a simulation run against what a real user would expect to see.

Prints a PASS/FAIL report and exits non-zero on any FAIL, so CI turns red
instead of anyone having to eyeball it.

Usage: python check.py <sim dir> [--gui]
"""

import os
import re
import struct
import sys

sim = sys.argv[1]
gui = "--gui" in sys.argv
fails, notes = [], []


def ok(cond, msg):
    print(("PASS  " if cond else "FAIL  ") + msg)
    if not cond:
        fails.append(msg)


roms = next(os.path.join(sim, d) for d in os.listdir(sim)
            if d not in ("out", "gui", "ES-DE", "import") and os.path.isdir(os.path.join(sim, d)))


def pegasus(system):
    p = os.path.join(roms, system, "metadata.pegasus.txt")
    if not os.path.exists(p):
        return None
    games, cur = {}, None
    for line in open(p, encoding="utf-8"):
        if line.startswith("game: "):
            cur = line[6:].strip()
        elif line.startswith("file: ") and cur is not None:
            games[line[6:].strip()] = cur
    return games


def png_ok(path):
    try:
        with open(path, "rb") as f:
            head = f.read(24)
        if head[:8] != b"\x89PNG\r\n\x1a\n":
            return False, "not a PNG"
        w, h = struct.unpack(">II", head[16:24])
        size = os.path.getsize(path)
        if size < 1000 or w < 32 or h < 32:
            return False, f"suspiciously small ({w}x{h}, {size} bytes)"
        return True, f"{w}x{h}"
    except OSError as e:
        return False, str(e)


systems = {"snes": 8, "NES": 5, "Nintendo 64": 4, "gba": 3, "megadrive": 3,
           "psx": 5, "ps2": 4, "wii": 3, "gc": 2}

print("== Pegasus output")
total = covered = 0
for sysdir, count in systems.items():
    g = pegasus(sysdir)
    ok(g is not None, f"{sysdir}: metadata.pegasus.txt written")
    if g is None:
        continue
    ok(len(g) == count, f"{sysdir}: {len(g)} games listed (expected {count})")
    for f in g:
        stem = os.path.splitext(f)[0]
        total += 1
        art = os.path.join(roms, sysdir, "media", stem, "boxFront.png")
        if os.path.exists(art):
            covered += 1
            good, why = png_ok(art)
            ok(good, f"{sysdir}/{f}: cover is a valid image ({why})")
        else:
            notes.append(f"no cover: {sysdir}/{f}  (title shown: {g[f]})")

junk = ["._Super Mario World (USA).sfc", ".DS_Store", "Thumbs.db", "desktop.ini", "empty.sfc"]
listed = pegasus("snes") or {}
for j in junk:
    ok(j not in listed, f"junk file not treated as a game: {j}")
psx = pegasus("psx") or {}
ok("Final Fantasy VII (USA).m3u" in psx and not any("(Disc " in f and f.endswith(".chd") for f in psx),
   "multi-disc FF7 shows once (via .m3u), not as 3 discs")

print("\n== Identification of badly named files (title shown in the launcher)")
expect = [
    ("ps2", "game disc 1.iso", r"grand theft auto iii"),
    ("wii", "mystery disc.iso", r"super mario galaxy"),
    ("gc", "smash.iso", r"melee"),
    ("wii", "Wii Sports (USA) (Rev 1).wbfs", r"wii sports"),
    ("snes", "super metroid.smc", r"super metroid"),
    ("snes", "Street Fighter II Turbo (U) [!].smc", r"street fighter ii"),
    ("Nintendo 64", "Mario Kart 64 (U) [!].v64", r"mario kart 64"),
    ("NES", "Duck Tales.nes", r"duck ?tales"),
    ("gba", "Pokémon - Emerald Version (USA, Europe).gba", r"pok[eé]mon.*emerald"),
]
for sysdir, f, pat in expect:
    title = (pegasus(sysdir) or {}).get(f, "<missing>")
    ok(re.search(pat, title, re.I) is not None, f"{sysdir}/{f} → '{title}'")

pct = 100 * covered / max(1, total)
print(f"\n== Cover coverage: {covered}/{total} games ({pct:.0f}%)")
ok(pct >= 80, "at least 80% of games got a cover")

if not gui:
    out = os.path.join(sim, "out")
    rerun = open(os.path.join(out, "3-pegasus-rerun.log"), encoding="utf-8", errors="replace").read()
    m = re.search(r"(\d+) files saved", rerun)
    ok(m is not None and m.group(1) == "0", f"second run re-downloads nothing ({m.group(0) if m else 'no summary line'})")
    first = open(os.path.join(out, "2-pegasus.log"), encoding="utf-8", errors="replace").read()
    ok("Metal Gear Solid" in first and re.search(r"missing|not found", first.split("Metal Gear Solid", 1)[1][:400], re.I) is not None,
       "warns about the .cue whose .bin is missing")
    ok("panicked" not in first + rerun, "no crashes in the logs")

    print("\n== ES-DE output")
    es = os.path.join(sim, "ES-DE")
    for sysdir in ("snes", "ps2", "wii"):
        gl = os.path.join(es, "gamelists", sysdir, "gamelist.xml")
        ok(os.path.exists(gl), f"ES-DE gamelist for {sysdir}")
    gl = os.path.join(es, "gamelists", "ps2", "gamelist.xml")
    if os.path.exists(gl):
        ok("Grand Theft Auto III" in open(gl, encoding="utf-8").read(), "ES-DE shows 'game disc 1.iso' as Grand Theft Auto III")
    covers = os.path.join(es, "downloaded_media", "snes", "covers")
    n = len(os.listdir(covers)) if os.path.isdir(covers) else 0
    ok(n >= 6, f"ES-DE snes covers: {n}")
else:
    g = os.path.join(sim, "gui")
    p = os.path.join(g, "gui-problems.txt")
    probs = open(p, encoding="utf-8").read().strip() if os.path.exists(p) else "gui-problems.txt missing"
    ok(probs == "none", f"app window problems: {probs}")
    fin = [f for f in os.listdir(g) if f.endswith("-finished.txt")]
    text = open(os.path.join(g, fin[0]), encoding="utf-8").read() if fin else ""
    ok(bool(fin), "app reached the finished screen")
    ok("failed" not in text.lower() or "0 failed" in text.lower(), "finished screen shows no failures")

print("\n== Notes (not failures)")
for n in notes:
    print("  " + n)
print(f"\n{len(fails)} failure(s)")
sys.exit(1 if fails else 0)
