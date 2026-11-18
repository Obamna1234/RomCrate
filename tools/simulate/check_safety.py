"""File-safety check for Import & sort, run on Windows, macOS and Linux.

Usage: python check_safety.py <romcrate-cli> <work dir>

Builds a messy folder (made-up test files only, no real games) with emoji and
accented names, a path longer than Windows' old 260-character limit, a
read-only file, two different files with the same name, a symlink pointing out
of the folder, a zip, a 7z (when 7-Zip is installed) and a CHD. Then, with the
real CLI:

1. preview only → nothing changes
2. move everything → no file is lost (every original's bytes exist somewhere)
3. undo → the folder is back exactly as it was, the ROMs folder holds no games
4. a copy killed halfway → afterwards no half-written file; import-recover
   leaves the originals whole and the ROMs folder without a partial game

Exits 1 if any check fails.
"""

import hashlib
import os
import shutil
import stat
import subprocess
import sys
import time
import zipfile

cli, work = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
WIN = os.name == "nt"
fails = []


def ok(cond, msg):
    print(("PASS  " if cond else "FAIL  ") + msg, flush=True)
    if not cond:
        fails.append(msg)


def long(p):
    """Windows needs the \\\\?\\ prefix for paths over 260 characters."""
    p = os.path.abspath(p)
    return "\\\\?\\" + p if WIN and not p.startswith("\\\\?\\") else p


def put(rel, data, root):
    p = long(os.path.join(root, rel))
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "wb") as f:
        f.write(data)
    return p


def nes(tag):
    return b"NES\x1a" + bytes([1, 1, 0, 0]) + bytes(8) + tag.encode().ljust(16384, b"\0")


def gba(tag):
    r = bytearray(0x200)
    r[0xA0:0xAC] = tag.encode()[:12].ljust(12, b"\0")
    r[0xAC:0xB0] = b"ATST"
    r[0xB0:0xB2] = b"01"
    r[0xB2] = 0x96
    c = 0
    for b in r[0xA0:0xBD]:
        c = (c - b) & 0xFF
    r[0xBD] = (c - 0x19) & 0xFF
    return bytes(r)


def chd(tag, body=4096):
    h = bytearray(124)
    h[0:8] = b"MComprHD"
    h[8:12] = (124).to_bytes(4, "big")
    h[12:16] = (5).to_bytes(4, "big")
    return bytes(h) + tag + bytes(body)


def snapshot(root):
    """Every file under root → md5 (walks long paths too)."""
    out = {}
    for top, _, files in os.walk(long(root)):
        for f in files:
            p = os.path.join(top, f)
            if os.path.islink(p):
                out[os.path.relpath(p, long(root)) + " -> link"] = os.readlink(p)
                continue
            with open(p, "rb") as fh:
                out[os.path.relpath(p, long(root))] = hashlib.md5(fh.read()).hexdigest()
    return out


def run(*args):
    r = subprocess.run([cli, *args], capture_output=True, text=True, encoding="utf-8", errors="replace")
    return r.returncode, r.stdout + r.stderr


shutil.rmtree(long(work), ignore_errors=True)
src, dest, outside = (os.path.join(work, x) for x in ("messy", "ROMs", "outside"))

# ---- the messy folder
put("ünïcødé 🎮 folder/Spïel 🎮 Ünïcødé.nes", nes("unicode"), src)
put("a/Same Name.nes", nes("first"), src)
put("b/Same Name.nes", nes("second, different"), src)
deep = os.path.join(*(["a folder with a fairly long name to push the path length"] * 5))
put(os.path.join(deep, "Deep Game With A Long Name.gba"), gba("DEEPGAME"), src)
ro = put("read only/Read Only Game.gba", gba("READONLY"), src)
os.chmod(ro, stat.S_IREAD)
with zipfile.ZipFile(long(os.path.join(src, "Zipped Game.zip")), "w") as z:
    z.writestr("Zipped Game.nes", nes("zipped"))
put("CHD Game.chd", chd(b"DVD "), src)
seven = shutil.which("7z") or shutil.which("7za") or shutil.which("7zz")
if seven:
    tmp = put("_seven/Seven Game.nes", nes("seven"), work)
    subprocess.run([seven, "a", "-bd", long(os.path.join(src, "Seven Game.7z")), tmp], capture_output=True)
put("Real Outside.nes", nes("outside the folder"), outside)
try:
    os.symlink(os.path.join(outside, "Real Outside.nes"), long(os.path.join(src, "Link To Outside.nes")))
    linked = True
except (OSError, NotImplementedError):
    linked = False  # Windows without symlink rights
print(f"(7z: {'yes' if seven else 'not installed, skipped'}; symlink: {'yes' if linked else 'not allowed here, skipped'})")
longest = max(len(os.path.join(src, k)) for k in snapshot(src))
ok(longest > 260, f"the folder has a path over 260 characters ({longest})")

before_src, before_out = snapshot(src), snapshot(outside)

# ---- 1. preview only
code, out = run("import", "--from", src, "--to", dest, "--move")
ok(code == 0 and snapshot(src) == before_src and not os.path.exists(long(dest)), "preview only: nothing changes")

# ---- 2. move everything
code, out = run("import", "--from", src, "--to", dest, "--move", "--yes")
print(out[-1500:])
ok(code == 0 and "0 failed" in out, "move ran with nothing failed")
after_src, after_dest = snapshot(src), snapshot(dest)
have = set(after_src.values()) | set(after_dest.values())
lost = [k for k, v in before_src.items() if not k.endswith("-> link") and v not in have]
ok(not lost, f"no file lost in the move {lost}")
ok(sum(1 for k in after_dest if k.endswith(".nes")) >= 2, "games arrived in the ROMs folder")
same = [v for k, v in after_dest.items() if "Same Name" in k] + [v for k, v in after_src.items() if "Same Name" in k]
ok(len(set(same)) == 2, "both different 'Same Name.nes' files survive")
ok(snapshot(outside) == before_out, "the file the symlink points to (outside the folder) is untouched")
ok(not any(k.endswith(".part") for k in after_dest), "no half-written files")

# ---- 3. undo
code, out = run("import-undo", "--last")
print(out[-800:])
ok(code == 0, "undo ran")
ok(snapshot(src) == before_src, "after undo the messy folder is exactly as it was")
left = {k: v for k, v in snapshot(dest).items() if not k.startswith("_")} if os.path.exists(long(dest)) else {}
ok(not left, f"after undo the ROMs folder holds no games {list(left)[:5]}")
ok(snapshot(outside) == before_out, "outside file still untouched")

# ---- 4. a copy killed halfway
big_src = os.path.join(work, "big")
big = put("Huge Game.gba", gba("HUGEGAME"), big_src)
with open(big, "ab") as f:  # ~1.5 GB of real data (not sparse), so the copy takes a moment
    block = os.urandom(1 << 20)
    for _ in range(1536):
        f.write(block)
big_before = snapshot(big_src)
dest2 = os.path.join(work, "ROMs2")
p = subprocess.Popen([cli, "import", "--from", big_src, "--to", dest2, "--yes"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
cut = False
t0 = time.time()
while time.time() - t0 < 120 and p.poll() is None:
    if any(f.endswith(".part") for _, _, fs in os.walk(long(dest2)) for f in fs):
        time.sleep(0.05)
        p.kill()
        cut = True
        break
    time.sleep(0.01)
p.wait()
if not cut:
    print("NOTE  the copy finished before it could be cut off; the crash part of this check didn't run")
else:
    ok(any(k.endswith(".part") for k in snapshot(dest2)), "killed halfway: a half-written .part file is left (as expected)")
    ok(not any(k.endswith("Huge Game.gba") for k in snapshot(dest2)), "killed halfway: no partial file under the real name")
    code, out = run("import-recover")
    print(out[-600:])
    ok(code == 0 and "put back as it was" in out, "import-recover rolls back the cut-off game")
    ok(not any(k.endswith(".part") for k in snapshot(dest2)), "after recovery: no half-written leftovers")
    ok(snapshot(big_src) == big_before, "after recovery: the original is whole")

print(f"\n{len(fails)} failed" if fails else "\nall passed")
sys.exit(1 if fails else 0)
