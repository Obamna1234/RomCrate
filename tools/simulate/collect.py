"""Gather the parts of a simulation worth reviewing into one small folder.

Usage: python collect.py <sim dir> <dest dir> [--covers]
"""

import os
import shutil
import sys

sim, dest = sys.argv[1], sys.argv[2]
covers = "--covers" in sys.argv
os.makedirs(dest, exist_ok=True)

for top, dirs, files in os.walk(sim):
    rel = os.path.relpath(top, sim)
    for f in files:
        src = os.path.join(top, f)
        keep = (
            f.endswith((".log", ".txt", ".xml", ".lpl"))
            or (f.endswith(".png") and (rel.startswith("gui") or (covers and f == "boxFront.png")))
        )
        if keep and os.path.getsize(src) < 5_000_000:
            out = os.path.join(dest, rel, f)
            os.makedirs(os.path.dirname(out), exist_ok=True)
            shutil.copy2(src, out)
print("collected into", dest)
