"""Drive the installed RomCrate app like a user would, via tauri-driver (WebDriver).

Points the app at a ROM folder (pre-set in its saved preferences, since the
native folder picker can't be clicked by WebDriver), walks every wizard step,
starts a scrape, waits for it to finish and saves screenshots + page text.

Usage: python drive_gui.py <app executable> <rom folder> <out dir> [<import dir>]
  With <import dir> (from make_messy.py) it also runs Import & sort through the app.
"""

import json
import os
import sys
import time

from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.options import ArgOptions

app, roms, out = sys.argv[1:4]
imp_dir = sys.argv[4] if len(sys.argv) > 4 else None
os.makedirs(out, exist_ok=True)

opts = ArgOptions()
opts.set_capability("browserName", "wry")
opts.set_capability("tauri:options", {"application": app})

d = None
for attempt in range(5):
    try:
        d = webdriver.Remote("http://127.0.0.1:4444", options=opts)
        break
    except Exception as e:  # driver may still be starting
        print("connect retry:", e)
        time.sleep(3)
if d is None:
    sys.exit("could not start a WebDriver session")

shot_n = 0
problems = []


def shot(name):
    global shot_n
    shot_n += 1
    p = os.path.join(out, f"{shot_n:02d}-{name}.png")
    d.save_screenshot(p)
    with open(p[:-4] + ".txt", "w", encoding="utf-8") as f:
        f.write(d.find_element(By.TAG_NAME, "body").text)
    print("screenshot", p)


def wait_for(cond, secs, what):
    end = time.time() + secs
    while time.time() < end:
        try:
            if cond():
                return True
        except Exception:
            pass
        time.sleep(1)
    problems.append(f"timed out waiting for: {what}")
    return False


def body():
    return d.find_element(By.TAG_NAME, "body").text


def step(i):
    d.find_elements(By.CSS_SELECTOR, "nav.steps button.step")[i].click()
    time.sleep(1.5)


try:
    d.set_window_size(1280, 860)
    d.execute_script("localStorage.clear()")
    d.refresh()
    # First start: pick the device.
    if not wait_for(lambda: "What will you play on?" in body(), 60, "device picker on first start"):
        problems.append("first start doesn't ask for the device")
    shot("first-launch-device")
    odin = [b for b in d.find_elements(By.CSS_SELECTOR, "button.device") if "Odin 2 Portal" in b.text]
    if not odin:
        problems.append("Odin 2 Portal not offered")
    else:
        odin[0].click()
        if wait_for(lambda: "What would you like to do?" in body(), 20, "home after picking a device"):
            shot("home")
        got = json.loads(d.execute_script("return localStorage.getItem('romcrate.prefs')") or "{}")
        for k, want in [("device", "odin2-portal"), ("launcher", "pegasus"), ("pegasusOn", "android"), ("size", "standard")]:
            if got.get(k) != want:
                problems.append(f"device defaults: {k} = {got.get(k)!r}, expected {want!r}")
        for t in ["Set up everything", "Custom — pick what to do", "Pegasus launch lines", "Get artwork"]:
            if t not in body():
                problems.append(f"home: '{t}' missing")

    prefs = json.loads(d.execute_script("return localStorage.getItem('romcrate.prefs')") or "{}")
    prefs.update({"roots": [roms], "launcher": "pegasus", "pegasusOn": "android"})
    d.execute_script("localStorage.setItem('romcrate.prefs', arguments[0]);", json.dumps(prefs))
    d.refresh()
    d.find_element(By.ID, "nav-art").click()
    wait_for(lambda: " games · " in body(), 120, "library scan")
    time.sleep(1)
    shot("library")

    step(2)
    shot("launcher")
    step(3)
    shot("artwork")
    step(4)
    shot("scrape-ready")

    start = [b for b in d.find_elements(By.TAG_NAME, "button") if b.text.strip() == "Start scraping"]
    if not start:
        problems.append("no 'Start scraping' button")
    elif not start[0].is_enabled():
        problems.append("'Start scraping' button is disabled")
    else:
        start[0].click()
        time.sleep(20)
        shot("scraping")
        wait_for(lambda: "Finished" in d.find_element(By.ID, "steps").text, 30 * 60, "scrape to finish")
        time.sleep(2)
        shot("finished")
        main = d.find_element(By.ID, "main")
        d.execute_script("arguments[0].scrollTop = arguments[0].scrollHeight", main)
        time.sleep(1)
        shot("finished-bottom")

    # Settings page + a second visit to the library (re-render paths).
    d.find_element(By.ID, "nav-settings").click()
    time.sleep(1.5)
    shot("settings")
    step(1)
    shot("library-after")

    # Custom task: launch lines only.
    d.find_element(By.ID, "nav-pegasus").click()
    time.sleep(1.5)
    wl = [b for b in d.find_elements(By.TAG_NAME, "button") if b.text.strip() == "Write launch lines"]
    if not wl:
        problems.append("pegasus task: no 'Write launch lines' button")
    else:
        wl[0].click()
        if wait_for(lambda: "What changed" in body(), 60, "launch lines written"):
            time.sleep(1)
            shot("pegasus-task")

    if "failed to start" in body().lower():
        problems.append("app shows 'failed to start'")

    if imp_dir:
        # ---- Import & sort, the way a user clicks through it.
        prefs = json.loads(d.execute_script("return localStorage.getItem('romcrate.prefs')") or "{}")
        prefs["importSources"] = [os.path.join(imp_dir, "Downloads")]
        prefs["importDest"] = os.path.join(imp_dir, "ROMs sorted")
        prefs["importMode"] = "copy"
        d.execute_script("localStorage.setItem('romcrate.prefs', arguments[0]);", json.dumps(prefs))
        d.refresh()
        wait_for(lambda: "Your games" in body(), 60, "app to reload")
        d.find_element(By.ID, "nav-import").click()
        time.sleep(1)
        shot("import-start")
        chk = [b for b in d.find_elements(By.TAG_NAME, "button") if b.text.strip() == "Check my files"]
        if not chk:
            problems.append("import: no 'Check my files' button")
        else:
            chk[0].click()
            if wait_for(lambda: "problems and" in body() or "no problems found" in body(), 300, "check my files"):
                time.sleep(1)
                d.execute_script("document.getElementById('check-results')?.scrollIntoView({block:'start'})")
                time.sleep(1)
                shot("check-my-files")
                t = body()
                for want in ["Download didn't finish", "archive is damaged", "N64 ROM is damaged", "SecuROM"]:
                    if want not in t:
                        problems.append(f"check my files: '{want}' not shown")
            d.execute_script("document.getElementById('main').scrollTop = 0")
            time.sleep(1)
        pv = [b for b in d.find_elements(By.TAG_NAME, "button") if b.text.strip() == "Preview"]
        if not pv:
            problems.append("import: no Preview button")
        else:
            pv[0].click()
            if wait_for(lambda: "Need you to choose a console" in body(), 300, "import preview"):
                time.sleep(1)
                shot("import-preview")
                main = d.find_element(By.ID, "main")
                d.execute_script("arguments[0].scrollTop = arguments[0].scrollHeight", main)
                time.sleep(1)
                shot("import-preview-bottom")
                go_btn = [b for b in d.find_elements(By.TAG_NAME, "button") if b.text.strip().startswith("Copy ") and "game" in b.text]
                if not go_btn or not go_btn[0].is_enabled():
                    problems.append("import: Copy button missing or disabled")
                else:
                    go_btn[0].click()
                    if wait_for(lambda: "copied and checked" in body(), 300, "import to finish"):
                        time.sleep(1)
                        d.execute_script("document.getElementById('imp-progress')?.scrollIntoView({block:'start'})")
                        time.sleep(1)
                        shot("import-done")
                        t = body()
                        if " failed" in t and "0 failed" not in t:
                            problems.append("import: some games failed")
                        nxt = [b for b in d.find_elements(By.TAG_NAME, "button") if b.text.startswith("Get artwork for these games")]
                        if nxt:
                            nxt[0].click()
                            wait_for(lambda: "Wii" in body() and "Saturn" in body(), 120, "library to show the imported consoles")
                            time.sleep(1)
                            shot("library-after-import")
                        else:
                            problems.append("import: no 'Get artwork' button at the end")
finally:
    with open(os.path.join(out, "gui-problems.txt"), "w", encoding="utf-8") as f:
        f.write("\n".join(problems) or "none")
    print("problems:", problems or "none")
    d.quit()
