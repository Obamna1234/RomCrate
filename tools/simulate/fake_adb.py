#!/usr/bin/env python3
"""A stand-in for Android's `adb`, for tests. The "handheld" is a folder:
$FAKE_ADB_ROOT/storage/emulated/0 (internal) and $FAKE_ADB_ROOT/storage/1A2B-3C4D (SD card).

Options (environment):
  FAKE_ADB_STATE=unauthorized   the handheld hasn't allowed this computer
  FAKE_ADB_CORRUPT=<text>       pushes of files whose name contains <text> arrive damaged
"""
import os, shutil, subprocess, sys

root = os.environ["FAKE_ADB_ROOT"]
args = sys.argv[1:]

def to_fake(s: str) -> str:
    return s.replace("/storage", root + "/storage")

def from_fake(s: str) -> str:
    return s.replace(root + "/storage", "/storage")

if args[:1] == ["version"]:
    print("Android Debug Bridge version 1.0.41 (RomCrate test stand-in)"); sys.exit(0)
if args[:1] == ["devices"]:
    state = os.environ.get("FAKE_ADB_STATE", "device")
    print("List of devices attached")
    print(f"FAKE0001       {state} usb:1-1 product:odin2 model:Test_Handheld device:odin2 transport_id:1\n")
    sys.exit(0)
if args[:1] == ["-s"]:
    if os.environ.get("FAKE_ADB_STATE") == "unauthorized":
        print("adb: device unauthorized.", file=sys.stderr); sys.exit(1)
    cmd, rest = args[2], args[3:]
    ro = os.environ.get("FAKE_ADB_READONLY")  # a storage this "handheld" won't let us write to
    if ro and cmd == "shell" and "touch" in rest[0] and ro in rest[0]:
        print(f"touch: '{ro}/.romcrate-write-test': Permission denied"); sys.exit(0)
    if cmd == "shell":
        p = subprocess.run(["sh", "-c", to_fake(rest[0])], capture_output=True, text=True)
        sys.stdout.write(from_fake(p.stdout)); sys.stderr.write(from_fake(p.stderr)); sys.exit(p.returncode)
    if cmd == "push":
        local, remote = rest[0], to_fake(rest[1])
        if not os.path.isdir(os.path.dirname(remote)):
            print(f"adb: error: failed to copy '{local}' to '{rest[1]}': remote No such file or directory", file=sys.stderr); sys.exit(1)
        shutil.copyfile(local, remote)
        bad = os.environ.get("FAKE_ADB_CORRUPT")
        if bad and bad in os.path.basename(remote):
            with open(remote, "r+b") as f:
                f.seek(100); b = f.read(1); f.seek(100); f.write(bytes([b[0] ^ 0xFF]))
        print(f"{local}: 1 file pushed, 0 skipped."); sys.exit(0)
print("adb: unknown command " + " ".join(args), file=sys.stderr); sys.exit(1)
