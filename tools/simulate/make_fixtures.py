"""Build a fake-but-realistic ROM collection for the "real user" simulation.

Files carry real-world game names but junk contents (no copyrighted data).
A few discs get a genuine *header* (game code / SYSTEM.CNF) so identification
by disc contents is exercised even when the file name is useless.

Usage: python make_fixtures.py <root>
"""

import io
import os
import random
import sys
import zipfile

root = sys.argv[1]
rng = random.Random(42)


def junk(n=64 * 1024):
    return bytes(rng.getrandbits(8) for _ in range(n))


def put(rel, data=None):
    p = os.path.join(root, *rel.split("/"))
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "wb") as f:
        f.write(junk() if data is None else data)


def disc_header(code, rev=0, offset=0, size=128 * 1024):
    """GameCube/Wii image whose header carries a real game code."""
    b = bytearray(junk(size))
    b[offset:offset + 6] = code.encode()
    b[offset + 7] = rev
    return bytes(b)


def ps_iso(cnf_text):
    import pycdlib

    iso = pycdlib.PyCdlib()
    iso.new(interchange_level=1)
    data = cnf_text.encode()
    iso.add_fp(io.BytesIO(data), len(data), "/SYSTEM.CNF;1")
    filler = junk(32 * 1024)
    iso.add_fp(io.BytesIO(filler), len(filler), "/DATA.BIN;1")
    out = io.BytesIO()
    iso.write_fp(out)
    iso.close()
    return out.getvalue()


# --- cartridge systems (name + CRC paths) ---------------------------------
for n in [
    "Super Mario World (USA).sfc",
    "Chrono Trigger (USA).sfc",
    "Legend of Zelda, The - A Link to the Past (USA).sfc",
    "Donkey Kong Country 2 - Diddy's Kong Quest (USA) (En,Fr) (Rev 1).sfc",
    "super metroid.smc",                       # sloppy lower-case name
    "Street Fighter II Turbo (U) [!].smc",     # GoodTools style
    "Earthbound.sfc",                          # no region at all
]:
    put("snes/" + n)
# A zipped ROM next to loose ones.
buf = io.BytesIO()
with zipfile.ZipFile(buf, "w") as z:
    z.writestr("Super Mario Kart (USA).sfc", junk())
put("snes/Super Mario Kart (USA).zip", buf.getvalue())
# Things that must be ignored, not treated as games.
put("snes/._Super Mario World (USA).sfc", b"\x00\x05\x16\x07")   # macOS AppleDouble
put("snes/.DS_Store", b"\x00")
put("snes/Thumbs.db", b"\x00")
put("snes/desktop.ini", b"[.ShellClassInfo]\r\n")
put("snes/empty.sfc", b"")                                       # zero-byte file

for n in [
    "Super Mario Bros. 3 (USA).nes",
    "Mega Man 2 (USA).nes",
    "Castlevania (USA).nes",
    "Duck Tales.nes",
    "Legend of Zelda, The (USA).nes",
]:
    put("NES/" + n)  # upper-case folder name

for n in [
    "Super Mario 64 (USA).z64",
    "GoldenEye 007 (USA).n64",
    "Mario Kart 64 (U) [!].v64",
    "Legend of Zelda, The - Ocarina of Time (USA) (Rev 2).z64",
]:
    put("Nintendo 64/" + n)  # folder name with a space

for n in [
    "Pokémon - Emerald Version (USA, Europe).gba",  # non-ASCII
    "Metroid Fusion (USA).gba",
    "Mario & Luigi - Superstar Saga (USA).gba",     # ampersand
]:
    put("gba/" + n)

for n in [
    "Sonic The Hedgehog 2 (World) (Rev A).md",
    "Streets of Rage 2 (USA).md",
    "Gunstar Heroes (USA).md",
]:
    put("megadrive/" + n)

# --- disc systems -----------------------------------------------------------
for n in [
    "Crash Bandicoot (USA).chd",
    "Spyro the Dragon (USA).chd",
    "Tony Hawk's Pro Skater 2 (USA).chd",
]:
    put("psx/" + n)
# Multi-disc game: discs + .m3u should show up as ONE game.
for d in (1, 2, 3):
    put(f"psx/Final Fantasy VII (USA) (Disc {d}).chd")
put("psx/Final Fantasy VII (USA).m3u",
    "".join(f"Final Fantasy VII (USA) (Disc {d}).chd\r\n" for d in (1, 2, 3)).encode())
# .cue whose .bin is missing -> should warn, not crash.
put("psx/Metal Gear Solid (USA) (Disc 1).cue",
    b'FILE "Metal Gear Solid (USA) (Disc 1).bin" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n')

for n in [
    "Shadow of the Colossus (USA).chd",
    "Kingdom Hearts (USA).chd",
    "Ratchet & Clank (USA).chd",
]:
    put("ps2/" + n)
# Badly named PS2 ISO: only the SYSTEM.CNF inside says what it is (GTA III, SLUS-20062).
put("ps2/game disc 1.iso", ps_iso("BOOT2 = cdrom0:\\SLUS_200.62;1\r\nVER = 1.40\r\nVMODE = NTSC\r\n"))

# Wii / GameCube with real header codes but unhelpful names.
put("wii/mystery disc.iso", disc_header("RMGE01"))                 # Super Mario Galaxy (USA)
put("wii/Wii Sports (USA) (Rev 1).wbfs", disc_header("RSPE01", rev=1, offset=0x200))
put("wii/Mario Kart Wii (USA).rvz")                                 # junk header -> name fallback
put("gc/smash.iso", disc_header("GALE01", rev=2))                   # Melee (USA) v1.02
put("gc/Legend of Zelda, The - The Wind Waker (USA).rvz")

# Folders that are not systems / leftovers.
os.makedirs(os.path.join(root, "random stuff"), exist_ok=True)
put("random stuff/notes.txt", b"hello")

print("fixtures written to", root)
