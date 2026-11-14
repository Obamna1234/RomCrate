"""Drive the installed ShelfArt app like a user would, via tauri-driver (WebDriver).

Points the app at a ROM folder (pre-set in its saved preferences, since the
native folder picker can't be clicked by WebDriver), walks every wizard step,
starts a scrape, waits for it to finish and saves screenshots + page text.

Usage: python drive_gui.py <app executable> <rom folder> <out dir>
"""

import json
import os
import sys
import time

from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.options import ArgOptions

app, roms, out = sys.argv[1:4]
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
    wait_for(lambda: "Your games" in body(), 60, "app to load")
    shot("first-launch")
    errs = d.get_log("browser") if "goog:loggingPrefs" in d.capabilities else []

    prefs = {"roots": [roms], "launcher": "pegasus"}
    d.execute_script("localStorage.setItem('shelfart.prefs', arguments[0]);", json.dumps(prefs))
    d.refresh()
    wait_for(lambda: " games · " in body(), 120, "library scan")
    time.sleep(1)
    shot("library")

    step(1)
    shot("launcher")
    step(2)
    shot("artwork")
    step(3)
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
    step(0)
    shot("library-after")

    if "failed to start" in body().lower():
        problems.append("app shows 'failed to start'")
finally:
    with open(os.path.join(out, "gui-problems.txt"), "w", encoding="utf-8") as f:
        f.write("\n".join(problems) or "none")
    print("problems:", problems or "none")
    d.quit()
