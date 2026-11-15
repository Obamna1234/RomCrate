"""Check an Import & sort run made from make_messy.py.

Usage: python check_import.py <import dir> [--gui]
  <import dir> contains Downloads/ (source) and "ROMs sorted"/ (destination);
  CLI runs also have Downloads2/ → "ROMs moved"/ (move mode) and log files.
"""

import os
import re
import sys
import zlib

root = sys.argv[1]
gui = "--gui" in sys.argv
src = os.path.join(root, "Downloads")
dst = os.path.join(root, "ROMs sorted")
fails = []


def ok(cond, msg):
    print(("PASS  " if cond else "FAIL  ") + msg)
    if not cond:
        fails.append(msg)


def crc(p):
    with open(p, "rb") as f:
        return zlib.crc32(f.read())


def tree(d):
    """Game files only (artwork and launcher files written by a later scrape are ignored)."""
    out = []
    for top, _, files in os.walk(d):
        for f in files:
            rel = os.path.relpath(os.path.join(top, f), d).replace("\\", "/")
            if "/media/" in rel or f in ("metadata.pegasus.txt", "gamelist.xml"):
                continue
            out.append(rel)
    return sorted(out)


# Every game, where it must end up, and the original it came from.
expected = {
    "nes/game1.bin": "game1.bin",
    "gba/Zelda Minish.gba": None,  # conflict: must still be the user's own different file
    "gb/pokemon red.gb": "stuff/pokemon red.gb",
    "gbc/pokemon crystal.gb": "stuff/pokemon crystal.gb",
    "snes/SMW.smc": "SMW.smc",
    "Nintendo 64/Super Mario 64 (USA).v64": "n64/Super Mario 64 (USA).v64",
    "Nintendo 64/Pokémon Stadium (USA).z64": "Pokémon Stadium (USA).z64",
    "megadrive/Sonic.bin": "sega/Sonic.bin",
    "nds/nds game.nds": "ds/nds game.nds",
    "nes/Mega Man 2 (USA).zip": "Mega Man 2 (USA).zip",
    "ps2/thing.iso": "random/thing.iso",
    "pc/Enter the Matrix PC Disc 1.iso": "PS2 games/Enter the Matrix PC Disc 1.iso",
    "psp/UMD.iso": "psp/UMD.iso",
    "wii/mystery.iso": "wii stuff/mystery.iso",
    "gc/gc.iso": "gc.iso",
    "saturn/Nights.cue": "Nights.cue",
    "saturn/Nights.bin": "Nights.bin",
    "psx/My Game.cue": "ps1/My Game.cue",
    "psx/My Game.bin": "ps1/My Game.bin",
    "psx/Big Game.m3u": "ps1/Big Game.m3u",
    "psx/Big Game (Disc 1).cue": "ps1/Big Game (Disc 1).cue",
    "psx/Big Game (Disc 1).bin": "ps1/Big Game (Disc 1).bin",
    "psx/Big Game (Disc 2).cue": "ps1/Big Game (Disc 2).cue",
    "psx/Big Game (Disc 2).bin": "ps1/Big Game (Disc 2).bin",
    "ps2/Kingdom Hearts (USA).chd": "PS2 Games/Kingdom Hearts (USA).chd",
    "psx/Crash Bandicoot (USA).chd": "Crash Bandicoot (USA).chd",
    "dreamcast/Shenmue (USA) (Disc 1).chd": "Shenmue (USA) (Disc 1).chd",
}
never = ["scph1001.bin", "readme.txt", "cover.jpg", "setup.exe", "failed download.gba",
         "Broken (USA).cue", "mystery.dat2", "evil.m3u", ".hidden.sfc", "passwd",
         "Unconfirmed 812733.crdownload", "Manhunt (PS2 Classics Vault) [SLUS20827].pkg",
         "Manhunt (Europe) (Disc 1).zip", "Broken Kart (USA).z64", "GTA San Andreas.mds",
         "GTA San Andreas.mdf", "CmdLineExt03.dll", "manhunt.exe", "nds game.cht",
         "Super_Famicom.dat", "Half Game (USA).iso.part"]

print("== Copy import (Downloads → ROMs sorted)")
have = tree(dst)
for rel, orig in expected.items():
    p = os.path.join(dst, rel)
    ok(os.path.exists(p), f"{rel} is there")
    if orig and os.path.exists(p):
        ok(crc(p) == crc(os.path.join(src, orig)), f"{rel} is identical to the original")
ok(open(os.path.join(dst, "gba", "Zelda Minish.gba"), "rb").read() == b"a different file",
   "existing different file was NOT overwritten")
for n in never:
    ok(not any(h.split("/")[-1] == n for h in have), f"not imported: {n}")
ok(not any(h.endswith(".part") for h in have), "no half-written .part files left")
extra = [h for h in have if h not in expected]
ok(not extra, f"nothing unexpected in the ROMs folder {extra if extra else ''}")
for orig in [o for o in expected.values() if o]:
    ok(os.path.exists(os.path.join(src, orig)), f"original kept (copy mode): {orig}")

with open(os.path.join(root, "tree-sorted.txt"), "w", encoding="utf-8") as f:
    f.write("\n".join(have))

if not gui:
    log = lambda n: open(os.path.join(root, n), encoding="utf-8", errors="replace").read()
    pre = log("1-import-preview.log")
    ok("Nothing was changed" in pre, "preview alone changes nothing")
    ok("even though it was in" in pre, "explains that the PC disc in the PS2 folder is really a PC disc")
    ok("official PlayStation list" in pre, "Crash Bandicoot.chd found in the official PlayStation list")
    ok("checksum OK" in pre, "N64 ROMs with good checksums are marked 'checksum OK'")
    ok("unknown free" not in pre and re.search(r"free\.", pre) is not None, "free space is found for a relative ROMs folder name")
    ok("nds game.cht" not in pre and "Super_Famicom.dat" not in pre, "cheat files and scraper game lists are left alone, not listed as unknown games")
    ok(re.search(r"Half Game \(USA\)\.iso\.part.*\n\s*skipped: [^\n]*Download didn't finish", pre) is not None, "a .part download is shown and refused in import")
    for name, why in [("Unconfirmed 812733.crdownload", "Download didn't finish"),
                      ("Manhunt (PS2 Classics Vault) [SLUS20827].pkg", "PlayStation Store package"),
                      ("Manhunt (Europe) (Disc 1).zip", "archive is damaged"),
                      ("Broken Kart (USA).z64", "N64 ROM is damaged")]:
        ok(re.search(re.escape(name) + r".*\n\s*skipped: [^\n]*" + re.escape(why), pre) is not None,
           f"import preview refuses {name} ({why})")

    chk = log("0-check.log")
    for want in ["Download didn't finish", "PlayStation Store package", "archive is damaged",
                 "Disc 1).bin", "N64 ROM is damaged", "N64 ROM verified",
                 "chdman can't read", "SecuROM disc copy protection"]:
        ok(want in chk, f"check my files reports: {want}")
    ok(re.search(r"PC[/\\]Manhunt\n.*SecuROM", chk) is not None, "copy protection named per game folder (PC/Manhunt)")
    ok(re.search(r"PC[/\\]FEAR\n.*SecuROM", chk) is not None, "readme mention found (PC/FEAR)")
    for bad in ["crack", "no-cd", "nocd", "bypass"]:
        ok(bad not in chk.lower(), f"no copy-protection bypass advice ('{bad}')")
    ok(os.path.exists(os.path.join(root, "0-check.exit")) and open(os.path.join(root, "0-check.exit")).read().strip() == "1",
       "check exits with code 1 when there are problems")

    rerun = log("3-import-rerun.log")
    m = re.search(r"Done: (\d+) imported, (\d+) already there", rerun)
    ok(m is not None and m.group(1) == "0", f"second import copies nothing again ({m.group(0) if m else 'no summary'})")

    print("\n== Move import (Downloads2 → ROMs moved)")
    src2 = os.path.join(root, "Downloads2")
    dst2 = os.path.join(root, "ROMs moved")
    for rel, orig in expected.items():
        if not orig:
            continue
        # This ROMs folder had no "Nintendo 64" folder yet, so a new "n64" one is made.
        rel2 = rel.replace("Nintendo 64/", "n64/")
        p = os.path.join(dst2, rel2)
        ok(os.path.exists(p) and crc(p) == crc(os.path.join(src, orig)), f"moved intact: {rel2}")
        ok(not os.path.exists(os.path.join(src2, orig)), f"original removed after move: {orig}")
    for n in ["scph1001.bin", "readme.txt", "mystery.dat2", "Broken (USA).cue"]:
        ok(os.path.exists(os.path.join(src2, n)), f"left alone in source: {n}")

    print("\n== Scraping the sorted folder afterwards")
    meta = os.path.join(dst, "ps2", "metadata.pegasus.txt")
    text = open(meta, encoding="utf-8").read() if os.path.exists(meta) else ""
    ok("game: Grand Theft Auto III" in text and "file: thing.iso" in text, "imported 'thing.iso' scrapes as Grand Theft Auto III")

print(f"\n{len(fails)} failure(s)")
sys.exit(1 if fails else 0)
