"""Build a messy "Downloads" folder to test Import & sort, plus a ROMs folder
that already has a few things in it.

Files have real console *headers* (built here from public format specs, no
copyrighted data) so RomCrate has to work out the console from the contents,
not the name or folder.

Usage: python make_messy.py <sim dir>
Creates <sim>/Downloads (messy) and <sim>/ROMs sorted (destination).
"""

import io
import os
import random
import struct
import sys
import zipfile

sim = sys.argv[1]
src = os.path.join(sim, "Downloads")
dst = os.path.join(sim, "ROMs sorted")
rng = random.Random(7)


def junk(n):
    return bytes(rng.getrandbits(8) for _ in range(n))


def put(rel, data, root=src):
    p = os.path.join(root, *rel.split("/"))
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "wb") as f:
        f.write(data)


# --- cartridge headers ------------------------------------------------------
def nes():
    return b"NES\x1a\x02\x01\x00\x00" + bytes(8) + junk(32 * 1024)


def gba(title=b"TESTGAME"):
    r = bytearray(junk(0x4000))
    r[0xA0:0xAC] = title.ljust(12, b"\0")
    r[0xAC:0xB0] = b"ATST"
    r[0xB0:0xB2] = b"01"
    r[0xB2] = 0x96
    c = 0
    for b in r[0xA0:0xBD]:
        c = (c - b) & 0xFF
    r[0xBD] = (c - 0x19) & 0xFF
    return bytes(r)


def gb(color=False):
    r = bytearray(junk(0x8000))
    r[0x134:0x13F] = b"POCKETTEST\0"
    r[0x143] = 0x80 if color else 0x00
    x = 0
    for b in r[0x134:0x14D]:
        x = (x - b - 1) & 0xFF
    r[0x14D] = x
    return bytes(r)


def snes(copier=False):
    r = bytearray(junk(0x20000))
    h = 0x7FC0
    r[h:h + 21] = b"TEST GAME".ljust(21, b" ")
    s = 0x4321
    r[h + 0x1C:h + 0x1E] = struct.pack("<H", s ^ 0xFFFF)
    r[h + 0x1E:h + 0x20] = struct.pack("<H", s)
    return (bytes(512) if copier else b"") + bytes(r)


def n64_crc(rom):
    """Standard N64 header checksum (CIC-6102 seed), independent of RomCrate's Rust code."""
    M = 0xFFFFFFFF
    t1 = t2 = t3 = t4 = t5 = t6 = 0xF8CA4DDC
    for i in range(0x1000, 0x101000, 4):
        d = int.from_bytes(rom[i:i + 4], "big")
        if (t6 + d) & M < t6:
            t4 = (t4 + 1) & M
        t6 = (t6 + d) & M
        t3 ^= d
        sh = d & 0x1F
        r = ((d << sh) | (d >> (32 - sh))) & M if sh else d
        t5 = (t5 + r) & M
        t2 = t2 ^ r if t2 > d else t2 ^ (t6 ^ d)
        t1 = (t1 + (t5 ^ d)) & M
    return t6 ^ t4 ^ t3, t5 ^ t2 ^ t1


def n64_z64(damaged=False):
    r = bytearray(junk(0x101000 + 0x2000))
    r[0:4] = bytes([0x80, 0x37, 0x12, 0x40])
    c1, c2 = n64_crc(r)
    r[0x10:0x14] = c1.to_bytes(4, "big")
    r[0x14:0x18] = c2.to_bytes(4, "big")
    if damaged:
        r[0x8000] ^= 0xFF
    return bytes(r)


def n64_v64():
    z = n64_z64()
    return bytes(b for i in range(0, len(z), 2) for b in (z[i + 1], z[i]))


def megadrive():
    r = bytearray(junk(0x8000))
    r[0x100:0x110] = b"SEGA GENESIS    "
    return bytes(r)


def crc16_modbus(data):
    crc = 0xFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


def nds():
    r = bytearray(junk(0x4000))
    r[0:12] = b"TESTGAME\0\0\0\0"
    r[0x0C:0x10] = b"ATSE"
    r[0x15E:0x160] = struct.pack("<H", crc16_modbus(bytes(r[:0x15E])))
    return bytes(r)


# --- discs ------------------------------------------------------------------
def iso(files):
    import pycdlib
    i = pycdlib.PyCdlib()
    i.new(interchange_level=1)
    for name, data in files.items():
        i.add_fp(io.BytesIO(data), len(data), "/" + name + ";1")
    out = io.BytesIO()
    i.write_fp(out)
    i.close()
    return out.getvalue()


def ps2_iso(serial="SLUS_200.62"):
    return iso({"SYSTEM.CNF": f"BOOT2 = cdrom0:\\{serial};1\r\nVER = 1.00\r\n".encode(), "DATA.BIN": junk(4096)})


def ps1_iso(serial="SCUS_949.00"):
    return iso({"SYSTEM.CNF": f"BOOT = cdrom:\\{serial};1\r\nTCB = 4\r\n".encode(), "DATA.BIN": junk(4096)})


def psp_iso():
    return iso({"UMD_DATA.BIN": b"ULUS-10041|0000000000000001|0001|G", "DATA.BIN": junk(2048)})


def pc_iso():
    return iso({"AUTORUN.INF": b"[autorun]\r\nopen=setup.exe\r\n", "SETUP.EXE": b"MZ" + junk(2048), "DATA1.CAB": junk(4096)})


def nintendo_disc(code, wii):
    b = bytearray(junk(64 * 1024))
    b[0:6] = code.encode()
    b[6] = 0
    b[7] = 0
    b[0x18:0x1C] = struct.pack(">I", 0x5D1C9EA3 if wii else 0)
    b[0x1C:0x20] = struct.pack(">I", 0 if wii else 0xC2339F3D)
    return bytes(b)


def saturn():
    b = bytearray(junk(64 * 1024))
    b[0:16] = b"SEGA SEGASATURN "
    return bytes(b)


def chd(tag):
    head = bytearray(124)
    head[0:8] = b"MComprHD"
    head[8:12] = struct.pack(">I", 124)
    head[12:16] = struct.pack(">I", 5)
    head[0x30:0x38] = struct.pack(">Q", 124)  # metadata right after the header
    # entry: tag(4) flags(1) length(3) next(8) data
    meta = tag + b"\x01" + (6).to_bytes(3, "big") + struct.pack(">Q", 0) + b"TYPE:X"
    return bytes(head) + meta + junk(32 * 1024)


# --- the mess ---------------------------------------------------------------
put("game1.bin", nes())                                   # NES rom with a meaningless .bin name
put("stuff/Zelda Minish.gba", gba())
put("stuff/pokemon red.gb", gb())
put("stuff/pokemon crystal.gb", gb(color=True))            # really a Game Boy Color game
SMW = snes(copier=True)
put("SMW.smc", SMW)
put("n64/Super Mario 64 (USA).v64", n64_v64())
put("Pokémon Stadium (USA).z64", n64_z64())                # unicode name
put("sega/Sonic.bin", megadrive())
put("ds/nds game.nds", nds())
zbuf = io.BytesIO()
with zipfile.ZipFile(zbuf, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("Mega Man 2 (USA).nes", nes())
put("Mega Man 2 (USA).zip", zbuf.getvalue())

put("random/thing.iso", ps2_iso())                         # PS2 by SYSTEM.CNF only
put("PS2 games/Enter the Matrix PC Disc 1.iso", pc_iso())   # in a PS2 folder but it's a PC disc
put("psp/UMD.iso", psp_iso())
put("wii stuff/mystery.iso", nintendo_disc("RMGE01", wii=True))
put("gc.iso", nintendo_disc("GALE01", wii=False))
put("Nights.bin", saturn())
put("Nights.cue", b'FILE "Nights.bin" BINARY\r\n  TRACK 01 MODE1/2352\r\n    INDEX 01 00:00:00\r\n')
# PS1 game as cue + bin (bin is a cooked ISO).
put("ps1/My Game.bin", ps1_iso())
put("ps1/My Game.cue", b'FILE "My Game.bin" BINARY\r\n  TRACK 01 MODE1/2048\r\n    INDEX 01 00:00:00\r\n')
# Two-disc PS1 game with a playlist.
for d in (1, 2):
    put(f"ps1/Big Game (Disc {d}).bin", ps1_iso("SLUS_008.92"))
    put(f"ps1/Big Game (Disc {d}).cue", f'FILE "Big Game (Disc {d}).bin" BINARY\r\n  TRACK 01 MODE1/2048\r\n    INDEX 01 00:00:00\r\n'.encode())
put("ps1/Big Game.m3u", b"Big Game (Disc 1).cue\r\nBig Game (Disc 2).cue\r\n")
# CHDs: content can't be read, so folder name / official list / CHD metadata decide.
put("PS2 Games/Kingdom Hearts (USA).chd", chd(b"DVD "))
put("Crash Bandicoot (USA).chd", chd(b"CHT2"))             # loose: official PlayStation list should find it
put("Shenmue (USA) (Disc 1).chd", chd(b"CHGD"))            # GD-ROM metadata → Dreamcast
# Things that must not be imported as games.
put("scph1001.bin", junk(512 * 1024))                      # BIOS
put("readme.txt", b"downloaded from somewhere")
put("cover.jpg", junk(2000))
put("setup.exe", b"MZ" + junk(1000))
put("failed download.gba", b"")
put("Broken (USA).cue", b'FILE "Broken (USA).bin" BINARY\r\n')  # bin missing
put("mystery.dat2", junk(5000))                            # nobody knows
put("evil.m3u", b"../../../../etc/passwd\r\n")
put(".hidden.sfc", snes())

# Problems "Check my files" must catch (named after real cases).
put("Unconfirmed 812733.crdownload", junk(4000))
put("Manhunt (PS2 Classics Vault) [SLUS20827].pkg", b"\x7fPKG" + junk(4000))
zbad = io.BytesIO()
with zipfile.ZipFile(zbad, "w", zipfile.ZIP_STORED) as z:
    z.writestr("Manhunt (Europe) (Disc 1).bin", junk(50_000))
zb = bytearray(zbad.getvalue())
zb[300] ^= 0xFF  # damage the stored data
put("Manhunt (Europe) (Disc 1).zip", bytes(zb))
put("Broken Kart (USA).z64", n64_z64(damaged=True))
put("GTA San Andreas/GTA San Andreas.mds", b"MEDIA DESCRIPTOR" + junk(100))
put("GTA San Andreas/GTA San Andreas.mdf", junk(8000))
put("PC/Manhunt/manhunt.exe", b"MZ" + junk(500))
put("PC/Manhunt/CmdLineExt03.dll", b"MZ" + junk(500))
put("PC/FEAR/Readme.txt", b"This game uses SecuROM copy protection.")
put("PC/FEAR/FEAR.exe", b"MZ" + junk(500))

# Not games: a cheat file and a scraper's game list (real case: Skraper's Super_Famicom.dat).
put("ds/nds game.cht", b";@@@@@ Game Title: test\r\n;@@@@@ Game Code: ATSE\r\n")
put("snes stuff/Super_Famicom.dat", b'<?xml version="1.0" encoding="utf-8"?>\n<!DOCTYPE datafile PUBLIC "-//Logiqx//DTD ROM Management Datafile//EN" "x">\n<datafile><header><name>Super Nintendo Entertainment System</name></header></datafile>\n')
put("Half Game (USA).iso.part", junk(3000))

# Destination already has a few things.
os.makedirs(os.path.join(dst, "Nintendo 64"), exist_ok=True)  # existing folder name must be reused
put("snes/SMW.smc", SMW, root=dst)            # identical → "already there"
put("gba/Zelda Minish.gba", b"a different file", root=dst)  # different → conflict, never overwritten

print("messy folder at", src, "→", dst)
