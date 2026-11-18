"""Big library check: 10,000+ games through the real CLI.

Usage: python check_big.py <romcrate-cli> <work dir> [count]

Makes <count> (default 10,500) tiny made-up NES/GBA/GB files in nested folders,
then: preview, copy them all, run again (everything already there), undo.
Checks every game arrives exactly once, nothing is lost, the times stay
reasonable, and (where the OS reports it) memory stays under 1 GB.
"""

import hashlib
import os
import shutil
import subprocess
import sys
import time

cli, work = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
count = int(sys.argv[3]) if len(sys.argv) > 3 else 10500
fails = []


def ok(cond, msg):
    print(("PASS  " if cond else "FAIL  ") + msg, flush=True)
    if not cond:
        fails.append(msg)


def nes(i):
    return b"NES\x1a" + bytes([1, 1, 0, 0]) + bytes(8) + f"game {i}".encode().ljust(2048, b"\0")


def gb(i):
    r = bytearray(0x150)
    r[0x104:0x108] = b"\xce\xed\x66\x66"
    r[0x134:0x13C] = f"G{i:07d}".encode()
    return bytes(r) + f"gb {i}".encode()


def run(args, measure=False):
    """Run the CLI; returns (code, output, seconds, peak memory MB or None)."""
    t = time.time()
    if measure and os.name != "nt" and os.path.exists("/usr/bin/time"):
        flag = "-l" if sys.platform == "darwin" else "-v"
        r = subprocess.run(["/usr/bin/time", flag, cli, *args], capture_output=True, text=True, errors="replace")
        mem = None
        for line in r.stderr.splitlines():
            line = line.strip()
            if "Maximum resident set size" in line:  # Linux: KB
                mem = int(line.split()[-1]) / 1024
            elif line.endswith("maximum resident set size"):  # macOS: bytes
                mem = int(line.split()[0]) / 1024 / 1024
        return r.returncode, r.stdout + r.stderr, time.time() - t, mem
    r = subprocess.run([cli, *args], capture_output=True, text=True, errors="replace")
    return r.returncode, r.stdout + r.stderr, time.time() - t, None


def files(root):
    out = {}
    for top, _, fs in os.walk(root):
        for f in fs:
            p = os.path.join(top, f)
            with open(p, "rb") as h:
                out[os.path.relpath(p, root)] = hashlib.md5(h.read()).hexdigest()
    return out


shutil.rmtree(work, ignore_errors=True)
src, dest = os.path.join(work, "Downloads"), os.path.join(work, "ROMs")
t = time.time()
for i in range(count):
    d = os.path.join(src, f"batch {i // 500:02d}", f"sub {i // 50 % 10}")
    os.makedirs(d, exist_ok=True)
    if i % 2:
        name, data = f"Homebrew Game {i:05d}.nes", nes(i)
    else:
        name, data = f"Homebrew Game {i:05d}.gb", gb(i)
    with open(os.path.join(d, name), "wb") as f:
        f.write(data)
print(f"made {count} test files in {time.time() - t:.1f}s")
before = files(src)

code, out, secs, mem = run(["import", "--from", src, "--to", dest], measure=True)
ok(code == 0 and f"{count // 2} games" in out.replace(",", ""), f"preview of {count} games ({secs:.1f}s{f', {mem:.0f} MB' if mem else ''})")
ok(secs < 300, "preview takes under 5 minutes")
if mem:
    ok(mem < 1024, f"preview memory under 1 GB ({mem:.0f} MB)")

code, out, secs, mem = run(["import", "--from", src, "--to", dest, "--yes"], measure=True)
print(out[-400:])
ok(code == 0 and "0 failed" in out, f"copied all ({secs:.1f}s{f', {mem:.0f} MB' if mem else ''})")
if mem:
    ok(mem < 1024, f"import memory under 1 GB ({mem:.0f} MB)")
got = files(dest)
ok(len(got) == count, f"every game arrived exactly once ({len(got)} of {count})")
ok(sorted(got.values()) == sorted(before.values()), "the copies are byte-for-byte the originals")
progress = [l for l in out.splitlines() if l.strip().endswith("%")]
ok(len(progress) >= 5, f"progress is reported as it goes ({len(progress)} updates)")

code, out, secs, _ = run(["import", "--from", src, "--to", dest, "--yes"])
ok(code == 0 and f"{count} already there" in out.replace(",", ""), f"second run: everything already there ({secs:.1f}s)")
ok(len(files(dest)) == count, "second run made no duplicates")

code, out, secs, _ = run(["import-undo", "--last"])
ok(code == 0 and f"{count} copies removed" in out, f"undo removed every copy ({secs:.1f}s)")
ok(files(src) == before, "originals untouched throughout")

print(f"\n{len(fails)} failed" if fails else "\nall passed")
sys.exit(1 if fails else 0)
