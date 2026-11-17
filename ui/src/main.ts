import { deviceArt } from "./deviceArt";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

// ---------------------------------------------------------------- types

type ArtKind = "boxart" | "box3d" | "boxback" | "cartridge" | "snap" | "title" | "logo" | "fanart" | "mix" | "video" | "manual";
type LauncherId = "pegasus" | "es-de" | "emulation-station" | "retro-arch" | "launch-box" | "folder";

interface Theme { id: string; name: string; description: string; kinds: ArtKind[] }
interface Launcher {
  id: LauncherId; name: string; tagline: string; supported: ArtKind[];
  needs_folder: boolean; folder_label: string; folder_hint: string; themes: Theme[]; where_note: string;
}
interface SystemInfo { id: string; name: string; maker: string }
interface Catalog {
  version: string; launchers: Launcher[]; systems: SystemInfo[];
  kinds: { id: ArtKind; label: string; source: string }[];
  sizePresets: [string, string, number][]; screenscraperAvailable: boolean; cacheDir: string;
  launchProfiles: LaunchProfile[];
  devices: DeviceInfo[];
}
interface DeviceInfo {
  id: string; name: string; family: string; platform: string; screen: { width: number; height: number; inches: number };
  chipset: string; launcher: string; too_demanding: string[]; limited: string[]; tested: boolean; tested_note?: string; notes?: string;
  sizePreset: string; launchPlatform: string | null;
}
interface LaunchProfile { id: string; emulator: string; platform: string; systems: string[]; verified: boolean; verified_on?: string; notes?: string }
interface Game { stem: string; file_name: string; rel_path: string }
interface ScannedSystem { system_id: string; system_name: string; maker: string; path: string; folder_name: string; games: Game[] }
interface Library { systems: ScannedSystem[] }
type Status = "done" | "partial" | "skipped" | "missing" | "failed";
interface GameResult {
  systemId: string; game: string; status: Status; matched?: string; method?: string;
  saved: ArtKind[]; missing: ArtKind[]; preview?: string; message?: string; thumb?: string;
}
interface Summary {
  games: number; done: number; partial: number; skipped: number; missing: number; failed: number;
  filesSaved: number; cancelled: boolean; missingGames: string[];
}

// ---------------------------------------------------------------- state

interface Prefs {
  roots: string[];
  deselected: string[]; // system paths the user unticked
  launcher: LauncherId;
  theme: Record<string, string>;
  folders: Record<string, string>;
  kinds: Record<string, ArtKind[]>;
  size: string;
  upscale: "off" | "smooth" | "smart" | "ai";
  aiBinary: string;
  regions: string[];
  identify: boolean;
  overwrite: boolean;
  metadata: boolean;
  extras: boolean;
  render3d: boolean;
  threads: number;
  launchbox: boolean;
  gameInfo: boolean;
  pegasusLayout: "auto" | "per-game" | "skraper";
  pegasusOn: "android" | "none";
  device: string;
  emulators: Record<string, string>;
  fixLaunch: boolean;
  importDest: string;
  importSources: string[];
  /** Console folder on this PC → storage on the handheld. */
  sendTargets: Record<string, string>;
  /** Ticked checklist steps in Game settings ("ps2:0" → true). */
  tuneTicks: Record<string, boolean>;
  /** PS2 game file → chosen ARMSX2 resolution (set by hand in ARMSX2). */
  ps2Picks: Record<string, string>;
  importMode: "copy" | "move";
  importRename: boolean;
}

const DEFAULT_PREFS: Prefs = {
  roots: [], deselected: [], launcher: "pegasus", theme: {}, folders: {}, kinds: {},
  size: "standard", upscale: "smart", aiBinary: "", regions: ["usa", "world", "europe", "australia", "uk", "japan"],
  identify: true, overwrite: false, metadata: true, extras: true, render3d: true, threads: 4, pegasusLayout: "auto", launchbox: true, gameInfo: true,
  pegasusOn: "none", device: "", emulators: {}, fixLaunch: false,
  importDest: "", importSources: [], importMode: "copy", importRename: false, sendTargets: {}, tuneTicks: {}, ps2Picks: {},
};

function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem("romcrate.prefs");
    if (raw) return { ...DEFAULT_PREFS, ...JSON.parse(raw) };
  } catch { /* storage unavailable */ }
  return { ...DEFAULT_PREFS };
}
function savePrefs() {
  try { localStorage.setItem("romcrate.prefs", JSON.stringify(prefs)); } catch { /* ignore */ }
}

let catalog!: Catalog;
let prefs = loadPrefs();
let library: Library = { systems: [] };
let scanning = false;
let unzip: { name: string; pct: number } | null = null;
const scanNotes: string[] = [];
let view: "home" | "device" | "pegasus" | "library" | "launcher" | "artwork" | "scrape" | "settings" | "import" | "send" | "tune" = "home";
// Session-only ScreenScraper login — never stored.
let ssUser = "";
let ssPass = "";

const run = {
  running: false, total: 0, processed: 0, results: [] as GameResult[], log: [] as { level: string; message: string }[],
  summary: null as Summary | null, filter: "all" as "all" | Status, currentSystem: "",
};

// ---------------------------------------------------------------- tiny DOM helper

type Child = Node | string | null | undefined | false;
function h<K extends keyof HTMLElementTagNameMap>(tag: K, props: Record<string, any> = {}, ...children: (Child | Child[])[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2).toLowerCase(), v);
    else if (k === "style") el.setAttribute("style", v);
    else if (k in el && k !== "list") (el as any)[k] = v;
    else el.setAttribute(k, String(v));
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.append(typeof c === "string" ? document.createTextNode(c) : c);
  }
  return el;
}

function toast(msg: string) {
  const t = document.getElementById("toast")!;
  t.textContent = msg;
  t.classList.add("show");
  setTimeout(() => t.classList.remove("show"), 2600);
}

// ---------------------------------------------------------------- helpers

const launcher = () => catalog.launchers.find((l) => l.id === prefs.launcher)!;
const themeOf = (l: Launcher) => l.themes.find((t) => t.id === prefs.theme[l.id]) ?? l.themes[0];
function kindsFor(l: Launcher): ArtKind[] {
  const k = prefs.kinds[l.id] ?? themeOf(l).kinds;
  return k.filter((x) => l.supported.includes(x) && kindAvailable(x));
}
function kindAvailable(k: ArtKind): boolean {
  const info = catalog.kinds.find((x) => x.id === k)!;
  if (info.source === "extras") return catalog.screenscraperAvailable && prefs.extras;
  // Back covers, cartridges and fan art come from the LaunchBox database.
  if (info.source === "launchbox") return prefs.launchbox || (catalog.screenscraperAvailable && prefs.extras);
  return true;
}
const selectedSystems = () => library.systems.filter((s) => !prefs.deselected.includes(s.path));
const selectedGames = () => selectedSystems().reduce((n, s) => n + s.games.length, 0);
const folderReady = () => !launcher().needs_folder || !!prefs.folders[prefs.launcher];

const KIND_HINTS: Record<ArtKind, string> = {
  boxart: "Front cover", box3d: "Rendered 3D case", boxback: "Back of the box", cartridge: "Cart, disc or card",
  snap: "In-game screenshot", title: "Title screen", logo: "Clear logo / wheel", fanart: "Wallpaper-style art",
  mix: "Screenshot + box + logo", video: "Short gameplay clip", manual: "PDF manual",
};
const REGION_NAMES: Record<string, string> = {
  usa: "USA", world: "World", europe: "Europe", australia: "Australia", uk: "UK", japan: "Japan",
  france: "France", germany: "Germany", spain: "Spain", italy: "Italy", korea: "Korea", brazil: "Brazil",
};

// ---------------------------------------------------------------- navigation

const STEPS = [
  { id: "import", t: "Sort new games", s: () => (imp.summary ? `${imp.summary.imported} imported` : "Optional — skip if already sorted") },
  { id: "library", t: "Your games", s: () => (library.systems.length ? `${selectedGames()} games · ${selectedSystems().length} systems` : "Choose your ROM folders") },
  { id: "launcher", t: "Launcher & theme", s: () => `${launcher().name.split(" (")[0]} · ${themeOf(launcher()).name}` },
  { id: "artwork", t: "Artwork", s: () => `${kindsFor(launcher()).length} types · ${catalog.sizePresets.find((p) => p[0] === prefs.size)?.[1].split(" —")[0]}` },
  { id: "scrape", t: "Scrape", s: () => (run.running ? `${run.processed}/${run.total}` : run.summary ? "Finished" : "Ready when you are") },
  { id: "send", t: "Send to handheld", s: () => (snd.running ? `${snd.done}/${snd.total} files` : snd.summary ? `${snd.summary.copied} copied` : "Optional — over USB") },
] as const;

function renderNav() {
  const nav = document.getElementById("steps")!;
  nav.replaceChildren(
    ...STEPS.map((st, i) => {
      // Outside the guided steps (Home, device, tools) nothing counts as "visited".
      const cur = STEPS.findIndex((x) => x.id === view);
      const done = (st.id === "import" && !!imp.summary)
        || (st.id === "library" && library.systems.length > 0 && view !== "library")
        || (st.id === "launcher" && folderReady() && i < cur)
        || (st.id === "artwork" && kindsFor(launcher()).length > 0 && i < cur)
        || (st.id === "scrape" && !!run.summary && !run.running)
        || (st.id === "send" && !!snd.summary && !snd.running && !snd.summary.failed);
      return h("button", { class: `step ${view === st.id ? "active" : ""} ${done ? "done" : ""}`, onclick: () => go(st.id) },
        h("div", { class: "num" }, done ? "✓" : String(i + 1)),
        h("div", {}, h("div", { class: "t" }, st.t), h("div", { class: "s" }, st.s())));
    }),
  );
}

function go(v: typeof view) {
  view = v;
  if (v === "settings") { logInfo = null; cacheBytes = null; }
  render();
  document.getElementById("main")!.scrollTop = 0;
}

function render() {
  renderNav();
  const main = document.getElementById("main")!;
  const views = { tune: viewTune, send: viewSend, home: viewHome, device: viewDevice, pegasus: viewPegasus, library: viewLibrary, launcher: viewLauncher, artwork: viewArtwork, scrape: viewScrape, settings: viewSettings, import: viewImport };
  document.getElementById("nav-home")?.classList.toggle("active", view === "home");
  document.getElementById("nav-pegasus")?.classList.toggle("active", view === "pegasus");
  const d = currentDevice();
  document.getElementById("nav-device")!.replaceChildren(
    prefs.device ? artOf(prefs.device, "chip-art") : "",
    h("div", { class: "k" }, "Games are for"),
    h("div", { class: "v" }, d ? d.name : prefs.device === "unsure" ? "Not sure — safe defaults" : "Choose your device…"));
  const sameView = main.dataset.view === view;
  const keep = sameView ? main.scrollTop : 0;
  // Activity panels follow the newest line, like a terminal — unless you've
  // scrolled up to read something, then they stay where you are.
  const logs = [...main.querySelectorAll<HTMLElement>(".log")].map((l) => ({ top: l.scrollTop, atEnd: l.scrollTop + l.clientHeight >= l.scrollHeight - 12 }));
  main.replaceChildren(...views[view]());
  main.dataset.view = view;
  main.scrollTop = keep;
  main.querySelectorAll<HTMLElement>(".log").forEach((l, i) => {
    const was = sameView ? logs[i] : undefined;
    l.scrollTop = !was || was.atEnd ? l.scrollHeight : was.top;
  });
}

function footer(back: typeof view | null, next: typeof view | null, nextLabel = "Continue", nextDisabled = false) {
  return h("div", { class: "footer-bar" },
    back ? h("button", { class: "btn", onclick: () => go(back) }, "← Back") : h("span"),
    next ? h("button", { class: "btn primary", disabled: nextDisabled, onclick: () => go(next) }, `${nextLabel} →`) : h("span"));
}


// ---------------------------------------------------------------- device & home

/** Short console names for badges ("PS3", not "PlayStation"). */
const SHORT_SYS: Record<string, string> = {
  ps2: "PS2", ps3: "PS3", psx: "PS1", psp: "PSP", psvita: "Vita", gc: "GameCube", wii: "Wii", wiiu: "Wii U",
  "3ds": "3DS", nds: "DS", n64: "N64", saturn: "Saturn", dreamcast: "Dreamcast",
};
function shortSys(id: string): string {
  return SHORT_SYS[id] ?? catalog.systems.find((s) => s.id === id)?.name ?? id;
}

function currentDevice(): DeviceInfo | undefined {
  return catalog.devices.find((d) => d.id === prefs.device);
}

/** Warn about consoles the chosen device can't realistically run. */
function fitBadge(systemId: string): Node | null {
  const d = currentDevice();
  if (!d) return null;
  if (d.too_demanding.includes(systemId)) return h("span", { class: "badge attention", style: "margin-top:6px;display:inline-block" }, `Too demanding for ${d.name.split(" (")[0]}`);
  if (d.limited.includes(systemId)) return h("span", { class: "badge likely", style: "margin-top:6px;display:inline-block" }, "Some games only on this device");
  return null;
}

function applyDevice(id: string) {
  prefs.device = id;
  const d = currentDevice();
  if (d) {
    if (catalog.launchers.some((l) => l.id === d.launcher)) prefs.launcher = d.launcher as LauncherId;
    prefs.size = d.sizePreset;
    prefs.pegasusOn = d.launchPlatform === "android" ? "android" : "none";
  } else {
    prefs.size = "standard";
  }
  savePrefs();
  go("home");
  toast(d ? `Set up for ${d.name}. You can still change any setting.` : "Using safe defaults. You can pick a device any time.");
}

/** Drawing of a device (our own line art; no photos or logos). */
function artOf(id: string, cls = "art"): HTMLElement {
  const d = document.createElement("div");
  d.className = cls;
  d.innerHTML = deviceArt(id);
  return d;
}

function viewDevice(): Node[] {
  if (!logInfo) loadLogInfo();
  const card = (d: DeviceInfo) => h("button", { class: `device ${prefs.device === d.id ? "on" : ""}`, onclick: () => applyDevice(d.id) },
    artOf(d.id),
    h("div", { class: "n" }, d.name),
    h("div", { class: "f" }, d.family),
    h("div", { class: "spec" }, `${d.screen.inches ? `${d.screen.inches}" ` : ""}${d.screen.width}×${d.screen.height}${d.chipset ? ` · ${d.chipset}` : ""}`),
    h("div", { class: "row" },
      d.tested ? h("span", { class: "badge sure" }, "Tested on this device") : null,
      d.too_demanding.length ? h("span", { class: "badge attention", title: `Too demanding: ${d.too_demanding.map(shortSys).join(", ")}` }, `Not for ${d.too_demanding.slice(0, 3).map(shortSys).join(", ")}${d.too_demanding.length > 3 ? "…" : ""}`) : null));
  return [
    h("h1", {}, "What will you play on?"),
    h("p", { class: "lead" }, "RomCrate sets things up to suit your device — the frontend it uses, how games are launched, and artwork sized for its screen. You can still change every setting yourself."),
    h("div", { class: "device-grid" },
      ...catalog.devices.map(card),
      h("button", { class: `device ${prefs.device === "unsure" ? "on" : ""}`, onclick: () => applyDevice("unsure") },
        artOf("unsure"),
        h("div", { class: "n" }, "Not sure / something else"),
        h("div", { class: "f" }, "Use safe defaults"),
        h("div", { class: "spec" }, "Works for most setups. Pick a device later for better defaults."))),
    h("div", { class: "note", style: "margin-top:16px" }, "Only devices marked “Tested on this device” have been checked on a real unit. Specs for the others come from the makers and reviews. The pictures are simple drawings made for RomCrate."),
    h("div", { class: "note", style: "margin-top:10px" },
      "RomCrate keeps its log and downloads in ", logInfo ? h("span", { class: "path" }, `\u200E${dataFolderText(logInfo)}\u200E`) : "your app data folder", ". ",
      h("button", { class: "link", style: "text-decoration:underline", onclick: chooseDataFolder }, "Change…")),
  ];
}

function viewHome(): Node[] {
  const d = currentDevice();
  const tasks: [string, string, string, (() => void) | null][] = [
    ["🩺 Check my files", "Find broken downloads, damaged files and disc copy protection.", "check", () => { go("import"); }],
    ["📥 Import & sort", "Put games from a messy folder into console folders.", "import", () => go("import")],
    ["🖼️ Get artwork", "Covers and screenshots — e.g. for a game you just downloaded.", "art", () => go("library")],
    ["🚀 Pegasus launch lines", "Make Pegasus start your games, without re-scraping.", "pegasus", () => go("pegasus")],
    ["📲 Send to handheld", "Copy games and artwork to your Android handheld over USB.", "send", () => go("send")],
    ["🎛️ Game settings", "Graphics and performance settings for each console's emulator, on the handheld and the TV.", "tune", () => go("tune")],
    ["🗜️ Compress discs", "Shrink disc games to CHD. Coming in the next version.", "soon", null],
    ["✅ Device setup checklist", "Recommended emulator settings for your device. Coming soon.", "soon", null],
  ];
  return [
    h("h1", {}, "What would you like to do?"),
    h("p", { class: "lead" }, d
      ? `Your games are for the ${d.name}. `
      : "Tell RomCrate which device your games are for and it will choose the best settings. ",
      h("button", { class: "link", style: "text-decoration:underline", onclick: () => go("device") }, d ? "Change device" : "Choose your device")),
    h("div", { class: "mode-grid" },
      h("div", { class: "mode primary" },
        h("span", { class: "pill" }, "Recommended"),
        h("div", { class: "mode-title" }, "Set up everything"),
        h("div", { class: "desc" }, "RomCrate walks you through it, one step at a time:"),
        h("ol", {},
          h("li", {}, "Sort new games into console folders (skip if they already are)"),
          h("li", {}, "Choose your games folders"),
          h("li", {}, d?.launchPlatform === "android" ? "Pick your frontend and emulators" : "Pick your frontend and theme"),
          h("li", {}, "Choose the artwork you want"),
          h("li", {}, "Get the artwork and launcher files")),
        h("div", { class: "row" },
          h("button", { class: "btn primary big", onclick: () => go("import") }, "Start →"),
          h("button", { class: "btn big", onclick: () => go("library") }, "My games are already sorted →")),
        h("div", { class: "hint" }, "Already set up? The second button skips sorting and goes straight to artwork and launcher files for your device.")),
      h("div", { class: "mode" },
        h("div", { class: "mode-title" }, "Custom — pick what to do"),
        h("div", { class: "desc" }, "Already set up? Run just the task you need."),
        h("div", { class: "task-grid" }, tasks.map(([t, desc, , fn]) =>
          h("button", { class: "task", disabled: !fn, onclick: () => fn?.() }, h("div", { class: "t" }, t), h("div", { class: "d" }, desc)))))),
    h("div", { class: "note", style: "margin-top:18px" },
      "Every task explains anything it couldn't do, and everything is also saved to the ",
      h("button", { class: "link", style: "text-decoration:underline", onclick: () => invoke("open_log_folder").catch((e) => toast(String(e))) }, "activity log"), "."),
  ];
}

// ---------------------------------------------------------------- view: Pegasus launch lines only

let launchNotes: string[] | null = null;
let launchBusy = false;

async function writeLaunchLinesOnly() {
  launchBusy = true; launchNotes = null; render();
  try {
    launchNotes = await invoke<string[]>("write_launch_lines", {
      roots: prefs.roots,
      launch: { platform: "android", perSystem: prefs.emulators, fixExisting: prefs.fixLaunch },
    });
  } catch (e) {
    toast(String(e));
  }
  launchBusy = false; render();
}

function viewPegasus(): Node[] {
  const out: (Node | null)[] = [
    h("h1", {}, "Pegasus launch lines"),
    h("p", { class: "lead" }, "Pegasus needs a launch line in each console folder to start your games. This writes just those — no artwork is downloaded."),
  ];
  if (!prefs.roots.length) {
    out.push(h("div", { class: "banner warn" }, "Add your ROM folders first."), h("button", { class: "btn primary", onclick: () => go("library") }, "Choose ROM folders →"));
    return out.filter((x): x is Node => !!x);
  }
  out.push(h("div", { class: "chips" }, prefs.roots.map((r) => h("span", { class: "chip" }, h("span", { class: "path" }, `‎${r}‎`)))));
  const prevOn = prefs.pegasusOn;
  prefs.pegasusOn = "android";
  out.push(...launchSection().slice(3)); // emulator picker + fix toggle (Android only here)
  prefs.pegasusOn = prevOn;
  out.push(h("div", { class: "row", style: "margin-top:16px" },
    h("button", { class: "btn primary", disabled: launchBusy, onclick: writeLaunchLinesOnly }, launchBusy ? "Writing…" : "Write launch lines")));
  if (launchNotes) {
    out.push(h("h2", {}, "What changed"),
      h("div", { class: "log" }, launchNotes.length ? launchNotes.map((n) => h("div", { style: "white-space:pre-wrap" }, n)) : [h("div", {}, "Nothing to do.")]));
  }
  return out.filter((x): x is Node => !!x);
}

// ---------------------------------------------------------------- view: library

/** Which of the user's chosen folders a system folder lives in (shown when there are several). */
/** "I:\\ScreenScraper Test\\ROMs\\n64" → "ScreenScraper Test\\ROMs\\n64" (last three parts). */
function shortPath(path: string): string {
  const sep = path.includes("\\") ? "\\" : "/";
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return (parts.length > 3 ? "…" + sep : "") + parts.slice(-3).join(sep);
}

/** A whole personal folder or drive (Documents, Desktop, home, C:\\): checking
 * it means reading every other program's files too. */
function isBroadFolder(path: string): boolean {
  const p = path.replace(/[\\/]+$/, "");
  return /^[A-Za-z]:$/.test(p) || p === "" ||
    /[\\/](Documents|Desktop|OneDrive)$/i.test(p) ||
    /^[A-Za-z]:[\\/]Users[\\/][^\\/]+$/i.test(p) ||
    /^\/(home|Users)\/[^/]+$/.test(p);
}

function rootLabel(path: string): string {
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const root = prefs.roots.map(norm).filter((r) => norm(path).startsWith(r + "/")).sort((a, b) => b.length - a.length)[0];
  return root ? root.split("/").pop() || root : "";
}

async function addRoots() {
  const picked = await open({ directory: true, multiple: true, title: "Choose your ROM folder(s)" });
  if (!picked) return;
  const list = Array.isArray(picked) ? picked : [picked];
  for (const p of list) if (!prefs.roots.includes(p)) prefs.roots.push(p);
  savePrefs();
  await rescan();
}

async function addZip() {
  const picked = await open({ multiple: true, title: "Choose a zipped games folder", filters: [{ name: "Zip archive", extensions: ["zip"] }] });
  if (!picked) return;
  const list = Array.isArray(picked) ? picked : [picked];
  for (const p of list) if (!prefs.roots.includes(p)) prefs.roots.push(p);
  savePrefs();
  await rescan();
}

async function rescan() {
  scanNotes.length = 0;
  scanning = true;
  renderIfShowsLibrary();
  try {
    library = await invoke<Library>("scan_library", { roots: prefs.roots });
  } catch (e) {
    toast(`Scan failed: ${e}`);
  }
  scanning = false;
  unzip = null;
  renderIfShowsLibrary();
}

/** A background rescan only rebuilds the screen that shows its result —
 * otherwise the screen you're on would flash a few seconds after start-up. */
function renderIfShowsLibrary() {
  if (["library", "launcher", "pegasus", "artwork", "scrape", "send"].includes(view)) render(); else renderNav();
}

let manualPath = "";
let manualSystem = "";

function viewLibrary(): Node[] {
  const out: Node[] = [
    h("h1", {}, "Your games"),
    h("p", { class: "lead" }, "Point RomCrate at the folder that holds your games. It recognises system folders like snes, psx, gba or megadrive automatically, and identifies each game by its contents — even badly named files."),
    h("div", { class: "row" },
      h("button", { class: "btn primary", onclick: addRoots }, "+ Add ROM folder"),
      h("button", { class: "btn", onclick: addZip, title: "A .zip of your games folder — RomCrate unzips it next to the zip first" }, "+ Add zipped folder"),
      prefs.roots.length ? h("button", { class: "btn", onclick: rescan, disabled: scanning }, scanning ? "Scanning…" : "Rescan") : null),
    h("div", { class: "desc", style: "margin-top:10px;color:var(--faint)" },
      "Games scattered across random folders? ",
      h("button", { class: "link", style: "text-decoration:underline", onclick: () => go("import") }, "Import & sort them into console folders first"), "."),
  ];
  if (prefs.roots.length) {
    out.push(h("div", { class: "chips", style: "margin-top:14px" },
      prefs.roots.map((r) => h("span", { class: "chip", title: r }, h("span", { class: "path" }, `\u200E${r}\u200E`),
        h("button", { class: "x", title: "Remove", onclick: async () => { prefs.roots = prefs.roots.filter((x) => x !== r); savePrefs(); await rescan(); } }, "×")))));
  }

  if (unzip) {
    out.push(h("div", { class: "card", style: "margin-top:16px" },
      h("div", { style: "font-weight:600;margin-bottom:8px" }, `Unzipping ${unzip.name}…`),
      h("div", { class: "progress" }, h("div", { style: `width:${unzip.pct}%` })),
      h("div", { class: "desc" }, "The zip is unpacked into a folder next to it. The zip itself isn't changed.")));
  }
  for (const n of scanNotes) out.push(h("div", { class: "banner warn", style: "margin-top:12px" }, n));
  if (!prefs.roots.length) {
    out.push(h("div", { class: "empty", style: "margin-top:24px" },
      h("div", { class: "big-icon" }, "🗂️"),
      h("div", { style: "font-weight:600;color:var(--text);margin-bottom:6px" }, "No folders yet"),
      h("div", {}, "Add the folder that contains your system folders (for example ROMs/, Emulation/roms/ or a USB drive) — or a .zip of it.")));
  } else if (!library.systems.length && !scanning) {
    out.push(h("div", { class: "banner warn", style: "margin-top:18px" },
      h("div", {}, "No systems recognised in these folders. Name each system's folder after the console (snes, psx, gba…), or assign a folder manually below.")));
  }

  if (library.systems.length) {
    const allOn = selectedSystems().length === library.systems.length;
    // The same console in more than one folder (e.g. an old test copy and a
    // fresh import): every game would get its art made twice.
    const twice = [...new Set(library.systems.map((s) => s.system_id))]
      .map((id) => library.systems.filter((s) => s.system_id === id))
      .filter((list) => list.length > 1);
    for (const list of twice) {
      out.push(h("div", { class: "banner warn", style: "margin-top:14px" }, h("div", {},
        h("b", {}, `${list[0].system_name} is in ${list.length} folders: `),
        list.map((s) => shortPath(s.path)).join(" and "),
        ". If one is an older copy of the same games, untick it below (or remove its folder above) — otherwise each game shows up, and gets its artwork made, twice.")));
    }
    out.push(
      h("h2", {}, `Systems found (${library.systems.length})`),
      h("div", { class: "row", style: "margin:-2px 0 12px" },
        h("button", { class: "link", onclick: () => { prefs.deselected = allOn ? library.systems.map((s) => s.path) : []; savePrefs(); render(); } }, allOn ? "Select none" : "Select all")),
      h("div", { class: "grid" },
        library.systems.map((s) => {
          const on = !prefs.deselected.includes(s.path);
          return h("div", {
            class: `sys ${on ? "on" : ""}`, title: s.path,
            onclick: () => { prefs.deselected = on ? [...prefs.deselected, s.path] : prefs.deselected.filter((p) => p !== s.path); savePrefs(); render(); },
          }, h("div", { class: "check" }),
            h("div", {}, h("div", { class: "name" }, s.system_name),
              h("div", { class: "meta" }, `${s.games.length} game${s.games.length === 1 ? "" : "s"} · ${library.systems.filter((o) => o.system_id === s.system_id).length > 1 ? shortPath(s.path) : `${s.folder_name}/${prefs.roots.length > 1 ? ` in ${rootLabel(s.path)}` : ""}`}`),
              fitBadge(s.system_id)));
        })),
    );
  }

  if (prefs.roots.length) {
    out.push(
      h("h2", {}, "Folder not recognised?"),
      h("div", { class: "card" },
        h("div", { class: "row" },
          h("button", { class: "btn", onclick: async () => { const p = await open({ directory: true, title: "Choose a system folder" }); if (p && !Array.isArray(p)) { manualPath = p; render(); } } }, manualPath ? "Change folder" : "Choose folder"),
          manualPath ? h("span", { class: "chip" }, h("span", { class: "path" }, manualPath)) : h("span", { style: "color:var(--faint)" }, "Pick a folder, then tell RomCrate which system it is."),
        ),
        manualPath ? h("div", { class: "row", style: "margin-top:12px" },
          (() => {
            const sel = h("select", { class: "input", style: "max-width:320px", onchange: (e: Event) => { manualSystem = (e.target as HTMLSelectElement).value; } },
              h("option", { value: "" }, "Select system…"),
              catalog.systems.map((s) => h("option", { value: s.id, selected: s.id === manualSystem }, `${s.maker} — ${s.name}`)));
            return sel;
          })(),
          h("button", {
            class: "btn primary", onclick: async () => {
              if (!manualSystem) return toast("Choose the system first");
              library = await invoke<Library>("add_system_folder", { systemId: manualSystem, path: manualPath });
              manualPath = ""; manualSystem = ""; render();
            },
          }, "Add")) : null),
    );
  }
  out.push(footer(null, "launcher", "Continue", !selectedSystems().length));
  return out;
}

// ---------------------------------------------------------------- view: launcher

function viewLauncher(): Node[] {
  const l = launcher();
  const theme = themeOf(l);
  const out: Node[] = [
    h("h1", {}, "Launcher & theme"),
    h("p", { class: "lead" }, "Where should the artwork go? RomCrate saves files with the exact names and folders your launcher expects, so they show up without any renaming."),
    h("div", { class: "launchers" },
      catalog.launchers.map((x) => h("button", {
        class: `launcher ${x.id === l.id ? "on" : ""}`,
        onclick: async () => {
          prefs.launcher = x.id;
          if (x.needs_folder && !prefs.folders[x.id]) {
            const g = await invoke<string | null>("guess_folder", { launcher: x.id });
            if (g) prefs.folders[x.id] = g;
          }
          savePrefs(); render();
        },
      }, h("div", { class: "name" }, x.name), h("div", { class: "tag" }, x.tagline)))),
    h("h2", {}, "Theme style"),
    h("div", { class: "themes" },
      l.themes.map((t) => h("button", {
        class: `theme ${t.id === theme.id ? "on" : ""}`,
        onclick: () => { prefs.theme[l.id] = t.id; delete prefs.kinds[l.id]; savePrefs(); render(); },
      }, h("div", { class: "name" }, t.name), h("div", { class: "d" }, t.description)))),
  ];
  if (l.needs_folder) {
    const f = prefs.folders[l.id] ?? "";
    out.push(
      h("h2", {}, l.folder_label),
      h("div", { class: "card" },
        h("div", { class: "row" },
          h("input", { class: "input", style: "flex:1;min-width:260px", value: f, placeholder: l.folder_hint, oninput: (e: Event) => { prefs.folders[l.id] = (e.target as HTMLInputElement).value; savePrefs(); renderNav(); } }),
          h("button", { class: "btn", onclick: async () => { const p = await open({ directory: true, title: l.folder_label }); if (p && !Array.isArray(p)) { prefs.folders[l.id] = p; savePrefs(); render(); } } }, "Browse…")),
        h("div", { class: "hint", style: "color:var(--faint);font-size:12.5px;margin-top:8px" }, l.folder_hint)),
    );
  }
  if (l.id === "pegasus") {
    out.push(
      h("h2", {}, "Media folder layout"),
      seg([["auto", "Match what's there"], ["skraper", "Skraper style"], ["per-game", "Folder per game"]], prefs.pegasusLayout, (v) => { prefs.pegasusLayout = v; savePrefs(); render(); }),
      h("div", { class: "desc" }, {
        auto: "If a system already has Skraper folders (media/box2dfront…), RomCrate adds to them; otherwise it uses a folder per game.",
        skraper: "media/box2dfront/<game>.png, media/wheel/… — the layout Skraper makes. Needs “Skraper Assets” enabled in Pegasus (on by default).",
        "per-game": "media/<game>/boxFront.png — Pegasus' own layout.",
      }[prefs.pegasusLayout]));
  }
  if (l.id === "pegasus") out.push(...launchSection());
  out.push(h("div", { class: "note" }, `→ ${l.where_note}`));
  out.push(footer("library", "artwork", "Continue", !folderReady()));
  return out;
}

/** Pegasus needs a launch line per system to start games; pick the emulator per console. */
function launchSection(): Node[] {
  const out: Node[] = [
    h("h2", {}, "Starting games from Pegasus"),
    h("p", { class: "desc" }, "Pegasus needs a “launch” line in each console folder to start your games. RomCrate can write it for the emulator you use."),
    seg([["android", "Android handheld"], ["none", "I'll add my own"]], prefs.pegasusOn, (v) => { prefs.pegasusOn = v; savePrefs(); render(); }),
  ];
  if (prefs.pegasusOn !== "android") return out;
  const systemsHere = [...new Set(library.systems.map((s) => s.system_id))];
  const rows = systemsHere
    .map((id) => ({ id, name: library.systems.find((s) => s.system_id === id)!.system_name, profiles: catalog.launchProfiles.filter((p) => p.platform === "android" && p.systems.includes(id)) }))
    .filter((r) => r.profiles.length)
    .sort((a, b) => a.name.localeCompare(b.name));
  if (!rows.length) {
    out.push(h("div", { class: "desc", style: "margin-top:10px" }, library.systems.length
      ? "None of your consoles have a known launch line yet — Pegasus files will get a placeholder for you to fill in."
      : "Add your games on step 1 to choose emulators."));
  } else {
    out.push(h("div", { class: "card", style: "margin-top:12px" }, rows.map((r) => {
      const sorted = [...r.profiles].sort((a, b) => Number(b.verified) - Number(a.verified));
      const chosen = sorted.find((p) => p.id === prefs.emulators[r.id]) ?? sorted[0];
      return h("div", { class: "emu-row" },
        h("div", {}, h("div", { style: "font-weight:600" }, r.name),
          h("div", { class: "hint" }, chosen.verified ? `Tested on ${chosen.verified_on ?? "a device"}` : "Not tested yet — written switched off, with a note so you can try it")),
        h("span", { class: `badge ${chosen.verified ? "sure" : "attention"}` }, chosen.verified ? "Tested" : "Untested"),
        h("select", {
          class: "input",
          onchange: (e: Event) => { prefs.emulators[r.id] = (e.target as HTMLSelectElement).value; savePrefs(); render(); },
        }, sorted.map((p) => h("option", { value: p.id, selected: p.id === chosen.id }, p.emulator))));
    })));
  }
  out.push(h("div", { style: "margin-top:12px" },
    toggle(prefs.fixLaunch, "Fix launch lines in Pegasus files I already have",
      "Off: your existing metadata.pegasus.txt files are never changed — RomCrate just shows you the line to add. On: a .bak backup is made first, then only the tested launch line is added (or a known-broken one replaced).",
      () => { prefs.fixLaunch = !prefs.fixLaunch; })));
  return out;
}

// ---------------------------------------------------------------- view: artwork

function seg<T extends string>(options: [T, string][], value: T, onPick: (v: T) => void) {
  return h("div", { class: "seg" }, options.map(([v, label]) => h("button", { class: v === value ? "on" : "", onclick: () => onPick(v) }, label)));
}

function toggle(on: boolean, title: string, desc: string, flip: () => void, disabled = false) {
  return h("div", { class: `toggle ${on ? "on" : ""}`, style: disabled ? "opacity:.45;cursor:not-allowed" : "", onclick: () => { if (!disabled) { flip(); savePrefs(); render(); } } },
    h("div", { class: "switch" }), h("div", {}, h("div", { class: "t" }, title), h("div", { class: "d" }, desc)));
}

function viewArtwork(): Node[] {
  const l = launcher();
  const chosen = kindsFor(l);
  const setKinds = (k: ArtKind[]) => { prefs.kinds[l.id] = k; savePrefs(); render(); };
  const out: Node[] = [
    h("h1", {}, "Artwork"),
    h("p", { class: "lead" }, `Pick what to download for ${l.name.split(" (")[0]}. The “${themeOf(l).name}” style pre-selected what that kind of theme shows.`),
  ];
  if (!catalog.screenscraperAvailable) {
    out.push(h("div", { class: "banner" }, h("div", {},
      "Videos and manuals come from the optional ScreenScraper extras source, which this build doesn't include yet. Everything else works with no account.")));
  }
  out.push(
    h("div", { class: "kinds" },
      catalog.kinds.map((k) => {
        const supported = l.supported.includes(k.id);
        const avail = supported && kindAvailable(k.id);
        const on = chosen.includes(k.id);
        const src = k.source === "free" ? ["free", "Free"]
          : k.source === "launchbox" ? ["free", "Free · LaunchBox"]
          : k.source === "launchbox+generated" ? ["free", "Free · LaunchBox / RomCrate"]
          : k.source === "generated" ? ["generated", "Made by RomCrate"]
          : ["extras", catalog.screenscraperAvailable ? "Extras" : "Coming soon"];
        return h("div", {
          class: `kind ${on ? "on" : ""} ${avail ? "" : "disabled"}`,
          title: supported ? KIND_HINTS[k.id] : `${l.name.split(" (")[0]} doesn't use this`,
          onclick: () => { if (avail) setKinds(on ? chosen.filter((x) => x !== k.id) : [...chosen, k.id]); },
        }, h("div", { class: `check ${on ? "on" : ""}` }),
          h("div", {}, h("div", { class: "label" }, k.label), h("span", { class: `badge ${src[0]}` }, supported ? src[1] : "Not used here")));
      })),
    h("h2", {}, "Image size"),
    seg(catalog.sizePresets.map((p) => [p[0], p[1].split(" —")[0]] as [string, string]), prefs.size, (v) => { prefs.size = v; savePrefs(); render(); }),
    h("div", { class: "desc" }, catalog.sizePresets.find((p) => p[0] === prefs.size)?.[1] ?? ""),
    h("h2", {}, "Upscaling"),
    seg([["smart", "Smart (recommended)"], ["smooth", "Smooth"], ["ai", "AI (Real-ESRGAN)"], ["off", "Off"]], prefs.upscale, (v) => { prefs.upscale = v; savePrefs(); render(); }),
    h("div", { class: "desc" }, {
      smart: "Keeps pixel-art screenshots razor sharp and enlarges box art smoothly with light sharpening.",
      smooth: "High-quality smooth resampling for everything.",
      ai: "Uses Real-ESRGAN on your computer for box art — best detail, slower. Screenshots still use Smart.",
      off: "Never enlarge; only shrink images that are too big.",
    }[prefs.upscale]),
  );
  if (prefs.upscale === "ai") {
    out.push(h("div", { class: "card", style: "margin-top:12px" },
      h("div", { class: "field", style: "margin-top:0" },
        h("label", {}, "realesrgan-ncnn-vulkan program"),
        h("div", { class: "row" },
          h("input", { class: "input", style: "flex:1", value: prefs.aiBinary, placeholder: "Path to realesrgan-ncnn-vulkan(.exe)", oninput: (e: Event) => { prefs.aiBinary = (e.target as HTMLInputElement).value; savePrefs(); } }),
          h("button", { class: "btn", onclick: async () => { const p = await open({ multiple: false, title: "Choose realesrgan-ncnn-vulkan" }); if (p && !Array.isArray(p)) { prefs.aiBinary = p; savePrefs(); render(); } } }, "Browse…")),
        h("div", { class: "hint" }, "Free download from github.com/xinntao/Real-ESRGAN (releases → ncnn-vulkan). If it isn't set, RomCrate falls back to Smart."))));
  }
  out.push(
    h("h2", {}, "Region preference"),
    h("div", { class: "regions" },
      prefs.regions.map((r, i) => h("span", { class: "region" }, `${i + 1}. ${REGION_NAMES[r] ?? r}`,
        i > 0 ? h("button", { title: "Move up", onclick: () => { const a = prefs.regions; [a[i - 1], a[i]] = [a[i], a[i - 1]]; savePrefs(); render(); } }, "↑") : null,
        h("button", { title: "Remove", onclick: () => { prefs.regions = prefs.regions.filter((x) => x !== r); savePrefs(); render(); } }, "×"))),
      (() => {
        const missing = Object.keys(REGION_NAMES).filter((r) => !prefs.regions.includes(r));
        if (!missing.length) return null;
        return h("select", { class: "input", style: "width:auto;padding:6px 10px", onchange: (e: Event) => { const v = (e.target as HTMLSelectElement).value; if (v) { prefs.regions.push(v); savePrefs(); render(); } } },
          h("option", { value: "" }, "+ Add region"), missing.map((r) => h("option", { value: r }, REGION_NAMES[r])));
      })()),
    h("div", { class: "desc" }, "When a game has artwork from several regions, RomCrate picks the first one on this list that exists."),
    h("h2", {}, "Options"),
    h("div", { class: "card toggles" },
      toggle(prefs.launchbox, "Use the free LaunchBox games database", "Adds 3D boxes, back covers, cartridges, logos, fan art and game info. No account; downloads about 200 MB once a week.", () => (prefs.launchbox = !prefs.launchbox)),
      toggle(prefs.gameInfo && prefs.launchbox, "Add game info", "Descriptions, genre, developer, release date, players and rating for your launcher. Your own edits are kept.", () => (prefs.gameInfo = !prefs.gameInfo), !prefs.launchbox),
      toggle(prefs.identify, "Identify games by their contents", "Reads checksums and disc serials and looks them up in the No-Intro and Redump databases. Renamed files still get the right art.", () => (prefs.identify = !prefs.identify)),
      toggle(prefs.extras && catalog.screenscraperAvailable, "Use the extras source (ScreenScraper guest mode)", catalog.screenscraperAvailable ? "Adds videos, manuals, 3D boxes and more. No account needed; slower than the free collection." : "Not included in this build yet.", () => (prefs.extras = !prefs.extras), !catalog.screenscraperAvailable),
      toggle(prefs.render3d, "Make 3D boxes myself when none exist", "Renders a 3D case from the 2D cover on your computer.", () => (prefs.render3d = !prefs.render3d)),
      toggle(prefs.metadata, "Update launcher files", "gamelist.xml (backed up first), Pegasus metadata if you don't have one, RetroArch playlists if missing.", () => (prefs.metadata = !prefs.metadata)),
      toggle(prefs.overwrite, "Replace artwork that already exists", "Off = only fill in what's missing.", () => (prefs.overwrite = !prefs.overwrite)),
    ),
    footer("launcher", "scrape", "Continue", chosen.length === 0),
  );
  return out;
}

// ---------------------------------------------------------------- view: scrape

async function startScrape() {
  const l = launcher();
  const preset = catalog.sizePresets.find((p) => p[0] === prefs.size) ?? catalog.sizePresets[1];
  const config = {
    launcher: l.id,
    folder: l.needs_folder ? prefs.folders[l.id] : null,
    kinds: kindsFor(l),
    size_scale: preset[2],
    upscale: prefs.upscale,
    ai_binary: prefs.upscale === "ai" && prefs.aiBinary ? prefs.aiBinary : null,
    overwrite: prefs.overwrite,
    prefs: { regions: prefs.regions, fuzzy: true, fuzzy_threshold: 0.92 },
    concurrency: prefs.threads,
    refresh_index: false,
    write_metadata: prefs.metadata,
    identify_by_contents: prefs.identify,
    use_screenscraper: prefs.extras && catalog.screenscraperAvailable,
    render_3d_fallback: prefs.render3d,
    pegasus_layout: prefs.pegasusLayout,
    use_launchbox: prefs.launchbox,
    game_info: prefs.gameInfo && prefs.launchbox,
    launch: l.id === "pegasus" && prefs.pegasusOn === "android"
      ? { platform: "android", perSystem: prefs.emulators, fixExisting: prefs.fixLaunch }
      : { platform: null, perSystem: {}, fixExisting: false },
  };
  Object.assign(run, { running: true, total: selectedGames(), processed: 0, results: [], log: [], summary: null, filter: "all", currentSystem: "" });
  render();
  try {
    await invoke("start_scrape", {
      selected: selectedSystems().map((s) => s.path),
      config,
      ssUser: ssUser || null,
      ssPassword: ssPass || null,
    });
  } catch (e) {
    run.running = false;
    toast(String(e));
    render();
  }
}

let renderQueued = false;
function queueRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    if (view === "scrape" || view === "import") render(); else renderNav();
  });
}

function handleEvent(e: any) {
  switch (e.type) {
    case "started": run.total = e.totalGames; break;
    case "systemStarted": run.currentSystem = e.systemName; break;
    case "systemIndex": break;
    case "gameDone": {
      run.processed++;
      const r: GameResult = { ...e };
      run.results.push(r);
      if (r.preview) invoke<string>("thumbnail", { path: r.preview }).then((d) => { r.thumb = d; queueRender(); }).catch(() => {});
      break;
    }
    case "log": {
      if (e.level === "debug") break;
      // "Downloading … 10 MB of 104 MB", "… 20 MB of 104 MB": one line that updates.
      const last = run.log[run.log.length - 1];
      const stem = (m: string) => m.split("…")[0];
      if (last && / MB of /.test(e.message) && / MB of /.test(last.message) && stem(last.message) === stem(e.message)) run.log[run.log.length - 1] = { level: e.level, message: e.message };
      else run.log.push({ level: e.level, message: e.message });
      break;
    }
    case "finished": run.running = false; run.summary = e.summary; toast(e.summary.cancelled ? "Scrape stopped" : "All done!"); break;
  }
  queueRender();
}

function viewScrape(): Node[] {
  const l = launcher();
  const out: (Node | null)[] = [h("h1", { style: "margin-bottom:18px" }, "Scrape")];
  if (!run.running && !run.summary) {
    const kinds = kindsFor(l).map((k) => catalog.kinds.find((x) => x.id === k)!.label);
    out.push(
      h("p", { class: "lead" }, "Everything's set. RomCrate only ever saves into the folders below and never deletes your files."),
      h("div", { class: "recap" },
        h("div", { class: "card" }, h("div", { class: "k" }, "Games"), h("div", { class: "v" }, `${selectedGames()} games in ${selectedSystems().length} systems`)),
        h("div", { class: "card" }, h("div", { class: "k" }, "Launcher"), h("div", { class: "v" }, `${l.name.split(" (")[0]} · ${themeOf(l).name}`), l.needs_folder ? h("div", { class: "note", style: "margin-top:6px" }, prefs.folders[l.id] ?? "") : null),
        h("div", { class: "card" }, h("div", { class: "k" }, "Artwork"), h("div", { class: "v" }, kinds.join(", ")))),
      !selectedGames() ? h("div", { class: "banner warn" }, "No games selected — go back to step 1.") : null,
      !folderReady() ? h("div", { class: "banner warn" }, `Choose the ${l.folder_label} in step 2.`) : null,
      h("button", { class: "btn primary big", disabled: !selectedGames() || !folderReady() || !kindsFor(l).length, onclick: startScrape }, "Start scraping"),
    );
    out.push(footer("artwork", null));
    return out.filter((x): x is Node => !!x);
  }

  const s = run.summary;
  const counts = { done: 0, partial: 0, skipped: 0, missing: 0, failed: 0 } as Record<Status, number>;
  run.results.forEach((r) => counts[r.status]++);
  const pct = run.total ? Math.round((run.processed / run.total) * 100) : 0;

  out.push(
    h("div", { class: "summary-grid" },
      h("div", { class: "stat ok" }, h("div", { class: "v" }, String(counts.done)), h("div", { class: "l" }, "Complete")),
      h("div", { class: "stat warn" }, h("div", { class: "v" }, String(counts.partial)), h("div", { class: "l" }, "Partly found")),
      h("div", { class: "stat" }, h("div", { class: "v" }, String(counts.skipped)), h("div", { class: "l" }, "Already had art")),
      h("div", { class: "stat bad" }, h("div", { class: "v" }, String(counts.missing + counts.failed)), h("div", { class: "l" }, "Not found"))),
    h("div", { class: "progress" }, h("div", { style: `width:${s ? 100 : pct}%` })),
    h("div", { class: "progress-label" },
      h("span", {}, s ? (s.cancelled ? "Stopped" : `Finished — ${s.filesSaved} files saved`) : `${run.currentSystem ? run.currentSystem + " · " : ""}${run.processed} of ${run.total} games`),
      h("span", {}, s ? "" : `${pct}%`)),
    h("div", { class: "row" },
      run.running ? h("button", { class: "btn danger", onclick: () => invoke("cancel_scrape") }, "Stop") : null,
      s && currentDevice()?.launchPlatform === "android" ? h("button", { class: "btn primary", onclick: () => go("send") }, "Send to handheld →") : null,
      s ? h("button", { class: `btn ${currentDevice()?.launchPlatform === "android" ? "" : "primary"}`, onclick: () => { run.summary = null; run.results = []; render(); } }, "Scrape again") : null,
      s ? h("button", { class: "btn", onclick: () => openOutput() }, "Open artwork folder") : null,
      s && s.missingGames.length ? h("button", { class: "btn", onclick: exportMissing }, `Save list of ${s.missingGames.length} missing`) : null),
  );

  const filters: ["all" | Status, string][] = [["all", "All"], ["done", "Complete"], ["partial", "Partly found"], ["skipped", "Already had art"], ["missing", "Not found"], ["failed", "Errors"]];
  out.push(h("div", { class: "filters" }, filters.map(([f, label]) =>
    h("button", { class: run.filter === f ? "on" : "", onclick: () => { run.filter = f; render(); } }, `${label}${f === "all" ? ` (${run.results.length})` : ` (${counts[f as Status] ?? 0})`}`))));

  // "Ratchet _ Clank (USA) (En,Fr)" → "Ratchet & Clank": the identified game, not the file name.
  const nice = (m: string) => m.replace(/\s*[([][^)\]]*[)\]]/g, "").replace(/ _ /g, " & ").trim() || m;
  const shown = run.results.filter((r) => run.filter === "all" || r.status === run.filter).slice(-400).reverse();
  out.push(h("div", { class: "results" }, shown.map((r) => h("div", { class: "res", title: [`File: ${r.game}`, r.matched ? `Matched: ${r.matched}${r.method ? ` (${r.method})` : ""}` : "No match", r.missing.length ? `Not available: ${r.missing.join(", ")}` : "", r.message ?? ""].filter(Boolean).join("\n") },
    h("div", { class: "img" }, r.thumb ? h("img", { src: r.thumb, alt: "" }) : r.status === "missing" ? "No artwork found" : "…"),
    h("div", { class: "cap" },
      h("div", { class: "g" }, h("span", { class: `dot ${r.status}` }), r.matched ? nice(r.matched) : r.game),
      h("div", { class: "m" }, r.status === "skipped" ? `${r.method === "hash" ? "✓ identified · " : ""}Already had art` : r.matched ? `${r.method === "hash" ? "✓ identified · " : ""}${r.saved.length} saved` : r.status === "missing" ? "Not in the collections" : r.message ?? ""))))));

  if (run.log.length) {
    out.push(h("h2", {}, "Activity"), h("div", { class: "log" }, run.log.slice(-2000).map((l) => h("div", { class: l.level }, l.message))), logButtons());
  }
  return out.filter((x): x is Node => !!x);
}

async function openOutput() {
  const l = launcher();
  const target = l.needs_folder ? prefs.folders[l.id] : selectedSystems()[0]?.path;
  if (target) await invoke("open_folder", { path: target }).catch((e) => toast(String(e)));
}

async function exportMissing() {
  if (!run.summary) return;
  const text = `Games RomCrate couldn't find artwork for (${new Date().toLocaleString()}):\n\n${run.summary.missingGames.join("\n")}\n`;
  await invoke<boolean>("save_missing_report", { text }).then((ok) => ok && toast("Saved")).catch((e) => toast(String(e)));
}

// ---------------------------------------------------------------- view: settings

let cacheBytes: number | null = null;

interface LogInfo { dir: string; today: string; dataDir: string | null; defaultLogDir: string; defaultCacheDir: string; cacheDir: string; logEnabled: boolean }
let logInfo: LogInfo | null = null;
function loadLogInfo() {
  invoke<LogInfo>("log_info").then((i) => { logInfo = i; if (view === "settings" || view === "device") render(); }).catch(() => {});
}

/** Where RomCrate keeps its own files, in words. */
function dataFolderText(i: LogInfo): string {
  return i.dataDir ?? `${i.defaultLogDir.replace(/[\\/]logs$/, "")} (log) and ${i.defaultCacheDir} (downloads)`;
}

async function chooseDataFolder() {
  const picked = await open({ directory: true, multiple: false, title: "Where should RomCrate keep its log and downloads?" });
  if (!picked || Array.isArray(picked)) return;
  await setDataFolder(picked);
}

async function setDataFolder(path: string | null) {
  try {
    const notes = await invoke<string[]>("set_data_dir", { path });
    cacheBytes = null;
    toast(notes.length ? `Done — ${notes.join("; ")}` : "Done — RomCrate will keep its files there from now on.");
  } catch (e) {
    toast(`Couldn't use that folder: ${e}`);
  }
  loadLogInfo();
}

/** Buttons shown under every Activity panel. */
function logButtons(): Node {
  return h("div", { class: "row", style: "margin-top:8px" },
    h("button", { class: "btn", onclick: () => invoke("open_log_file").catch((e) => toast(String(e))) }, "Open log (.txt)"),
    h("button", { class: "btn", onclick: () => invoke<string>("save_problem_report").then(() => toast("Problem report saved and opened.")).catch((e) => toast(String(e))) }, "Save problem report"));
}

function viewSettings(): Node[] {
  if (!logInfo) loadLogInfo();
  if (cacheBytes === null) invoke<number>("cache_size").then((b) => { cacheBytes = b; if (view === "settings") render(); });
  const mb = cacheBytes === null ? "…" : `${(cacheBytes / 1048576).toFixed(1)} MB`;
  return [
    h("h1", {}, "Settings & privacy"),
    h("p", { class: "lead" }, "RomCrate works without an account, collects nothing and only talks to the artwork sources listed here."),
    h("h2", {}, "Privacy"),
    h("div", { class: "card" }, h("ul", { class: "privacy", style: "margin:0;padding-left:18px" },
      h("li", {}, h("strong", {}, "No account, no telemetry. "), "There is no sign-up, no analytics and no crash reporting."),
      h("li", {}, h("strong", {}, "Only known sites. "), "RomCrate connects over HTTPS to GitHub and libretro (free artwork and game databases), the LaunchBox games database and, if enabled, screenscraper.fr — nothing else, even if a server tries to redirect it."),
      h("li", {}, h("strong", {}, "Clean files only. "), "Every image is fully decoded and re-saved as a fresh PNG, so nothing hidden in a download reaches your drive. Videos and manuals are checked to be real MP4/PDF files."),
      h("li", {}, h("strong", {}, "Your files are safe. "), "RomCrate never deletes games, never writes outside the folders you pick, and backs up gamelist.xml before changing it."))),
    h("h2", {}, "RomCrate's own files"),
    h("div", { class: "card" },
      h("div", { class: "desc" }, "RomCrate keeps two things of its own: the activity log, and downloaded databases and artwork indexes (so it doesn't re-download them every time). You choose where. Your games and artwork always go where you point RomCrate, never here."),
      logInfo ? h("div", { class: "to", style: "margin-top:8px" }, `\u200E${dataFolderText(logInfo)}\u200E`) : null,
      h("div", { class: "row", style: "margin-top:10px" },
        h("button", { class: "btn", onclick: chooseDataFolder }, "Change folder…"),
        logInfo?.dataDir ? h("button", { class: "btn", onclick: () => setDataFolder(null) }, "Use the default again") : null),
      h("div", { class: "hint", style: "margin-top:6px" }, "Changing it moves the existing log and downloads to the new folder. The default is your system's usual place for app data — it always exists and is cleaned up when you uninstall.")),
    h("h2", {}, "Activity log"),
    h("div", { class: "card" },
      logInfo ? toggle(logInfo.logEnabled, "Keep an activity log", "Everything RomCrate does — games found or not found and why, files skipped, errors — written to a daily .txt file (kept for 30 days). It stays on your computer.",
        () => { const on = !logInfo!.logEnabled; invoke("set_log_enabled", { on }).then(() => { logInfo!.logEnabled = on; render(); }).catch((e) => toast(String(e))); }) : null,
      h("div", { class: "desc", style: "margin-top:8px" }, "Something went wrong? “Save problem report” makes a short .txt with just the problems from the last week, your user name taken out of the paths — easy to share when asking for help."),
      logButtons(),
      h("div", { class: "row", style: "margin-top:8px" },
        h("button", { class: "btn", onclick: () => invoke("open_log_folder").catch((e) => toast(String(e))) }, "Open log folder"))),
    h("h2", {}, "ScreenScraper login (optional)"),
    h("div", { class: "card" },
      h("div", { style: "color:var(--muted);margin-bottom:12px;line-height:1.5" },
        catalog.screenscraperAvailable
          ? "Not needed — extras work in guest mode. If you already have a ScreenScraper account you can enter it for faster downloads. It's kept in memory for this session only and never saved."
          : "The extras source isn't included in this build yet."),
      h("div", { class: "row" },
        h("input", { class: "input", style: "max-width:240px", placeholder: "Username", value: ssUser, disabled: !catalog.screenscraperAvailable, oninput: (e: Event) => { ssUser = (e.target as HTMLInputElement).value; } }),
        h("input", { class: "input", style: "max-width:240px", type: "password", placeholder: "Password", value: ssPass, disabled: !catalog.screenscraperAvailable, oninput: (e: Event) => { ssPass = (e.target as HTMLInputElement).value; } }),
        ssUser || ssPass ? h("button", { class: "link", onclick: () => { ssUser = ""; ssPass = ""; render(); } }, "Forget") : null)),
    h("h2", {}, "Performance"),
    h("div", { class: "card" },
      h("div", { style: "font-weight:600;margin-bottom:8px" }, "Games processed at the same time"),
      seg([["1", "1"], ["2", "2"], ["4", "4"], ["6", "6"], ["8", "8"]], String(prefs.threads), (v) => { prefs.threads = Number(v); savePrefs(); render(); }),
      h("div", { class: "desc" }, "4 is a good balance. Lower it on slow connections or small handheld PCs.")),
    h("h2", {}, "Cache"),
    h("div", { class: "card" },
      h("div", { style: "color:var(--muted);margin-bottom:12px" }, `Downloaded artwork lists, databases and original images are cached so re-runs are fast and work offline. Currently ${mb}.`),
      h("div", { class: "note", style: "margin:0 0 12px" }, logInfo?.cacheDir ?? catalog.cacheDir),
      h("button", { class: "btn", onclick: async () => { await invoke("clear_cache"); cacheBytes = null; toast("Cache cleared"); render(); } }, "Clear cache")),
    h("h2", {}, "About"),
    h("div", { class: "card", style: "color:var(--muted);line-height:1.55" },
      `RomCrate ${catalog.version} — free and open source (MIT). Artwork from the libretro-thumbnails project and the LaunchBox Games Database`,
      catalog.screenscraperAvailable ? " and ScreenScraper" : "",
      ". Game identification uses the No-Intro and Redump databases published by libretro. All artwork belongs to its respective owners."),
  ];
}


// ---------------------------------------------------------------- view: import & sort

type Confidence = "sure" | "likely" | "unknown";
interface ImportItem {
  id: number; name: string; files: string[]; size: number; system?: string; systemName?: string;
  confidence: Confidence; reason: string; destName: string; destFolder?: string;
  state: { kind: "ready" | "already-there" | "conflict" | "needs-system" | "skipped"; detail?: string };
  warnings: string[];
}
interface ImportPlan {
  dest: string; mode: "copy" | "move"; items: ImportItem[];
  systems: { id: string; name: string; folder: string; games: number; bytes: number }[];
  neededBytes: number; freeBytes?: number; enoughSpace: boolean; ignoredFiles: number; notes: string[];
}
interface ImportSummary { imported: number; alreadyThere: number; failed: number; skipped: number; bytes: number; cancelled: boolean; folders: string[]; errors: string[] }

interface Finding { severity: "info" | "warn" | "error"; path: string; title: string; advice: string; check: string }
interface CheckReport { findings: Finding[]; filesChecked: number; errors: number; warnings: number }
let checkState: { running: boolean; report: CheckReport | null } = { running: false, report: null };

async function checkMyFiles() {
  checkState = { running: true, report: null };
  render();
  try {
    checkState.report = await invoke<CheckReport>("check_files", { paths: prefs.importSources });
  } catch (e) {
    toast(String(e));
  }
  checkState.running = false;
  render();
  document.getElementById("check-results")?.scrollIntoView({ block: "start", behavior: "smooth" });
}

function checkPanel(): Node[] {
  const r = checkState.report;
  if (!r) return [];
  const sevLabel = { error: "Problem", warn: "Warning", info: "OK" } as const;
  const sevClass = { error: "attention", warn: "likely", info: "sure" } as const;
  const shown = r.findings.filter((f) => f.severity !== "info" || f.check === "n64-checksum");
  return [
    h("h2", { id: "check-results" }, "Check my files"),
    h("div", { class: r.errors ? "banner warn" : "banner" },
      r.errors || r.warnings
        ? `Checked ${r.filesChecked} files: ${r.errors} problem${r.errors === 1 ? "" : "s"} and ${r.warnings} warning${r.warnings === 1 ? "" : "s"}. Files with problems won't be imported.`
        : `Checked ${r.filesChecked} files — no problems found.`),
    h("div", { class: "imp-list" }, shown.map((f) => h("div", { class: "imp" },
      h("div", {}),
      h("div", {}, h("div", { class: "n" }, f.title), h("div", { class: "to" }, `\u200E${f.path}\u200E`), h("div", { class: f.severity === "error" ? "warnline" : "why" }, f.advice)),
      h("span", { class: `badge ${sevClass[f.severity]}` }, sevLabel[f.severity])))),
  ];
}

const imp = {
  planning: false, plan: null as ImportPlan | null,
  assign: {} as Record<number, string>, exclude: new Set<number>(),
  running: false, done: 0, total: 0, current: "", log: [] as { ok: boolean; message: string }[],
  summary: null as ImportSummary | null,
};

function bytes(n: number): string {
  if (n >= 1073741824) return `${(n / 1073741824).toFixed(1)} GB`;
  if (n >= 1048576) return `${Math.round(n / 1048576)} MB`;
  return `${Math.max(1, Math.ceil(n / 1024))} KB`;
}

async function pickImportSource(zip: boolean) {
  const picked = zip
    ? await open({ multiple: true, title: "Choose a zipped games folder", filters: [{ name: "Zip archive", extensions: ["zip"] }] })
    : await open({ directory: true, multiple: true, title: "Choose the messy folder(s) to sort" });
  if (!picked) return;
  for (const p of Array.isArray(picked) ? picked : [picked]) if (!prefs.importSources.includes(p)) prefs.importSources.push(p);
  savePrefs();
  imp.plan = null; imp.summary = null;
  render();
}

async function pickImportDest() {
  const picked = await open({ directory: true, multiple: false, title: "Choose your ROMs folder (games go into console folders inside it)" });
  if (!picked || Array.isArray(picked)) return;
  prefs.importDest = picked; savePrefs();
  imp.plan = null; imp.summary = null;
  render();
}

async function previewImport() {
  imp.planning = true; imp.plan = null; imp.summary = null; imp.assign = {}; imp.exclude = new Set(); imp.log = [];
  render();
  try {
    imp.plan = await invoke<ImportPlan>("plan_import", { sources: prefs.importSources, dest: prefs.importDest, mode: prefs.importMode, rename: prefs.importRename });
  } catch (e) {
    toast(String(e));
  }
  imp.planning = false;
  render();
}

function willImport(it: ImportItem): boolean {
  if (imp.exclude.has(it.id)) return false;
  if (it.state.kind === "ready") return true;
  return it.state.kind === "needs-system" && !!imp.assign[it.id];
}

async function startImport() {
  if (!imp.plan) return;
  const exclude = imp.plan.items.filter((i) => imp.exclude.has(i.id)).map((i) => i.id);
  imp.running = true; imp.done = 0; imp.total = 0; imp.log = []; imp.summary = null;
  render();
  document.getElementById("imp-progress")?.scrollIntoView({ block: "start", behavior: "smooth" });
  try {
    await invoke("start_import", { assign: imp.assign, exclude });
  } catch (e) {
    imp.running = false;
    toast(String(e));
    render();
  }
}

function handleImportEvent(e: any) {
  switch (e.type) {
    case "started": imp.total = e.bytes; break;
    case "progress": imp.done = e.doneBytes; imp.total = e.totalBytes; imp.current = e.current; break;
    case "itemDone": imp.log.push({ ok: e.ok, message: e.ok ? `${e.name} — ${e.message}` : e.message }); break;
    case "finished": {
      imp.running = false; imp.summary = e.summary;
      // The ROMs folder you imported into is "your games" from now on — no
      // need to choose it again on the next step.
      const dest = imp.plan?.dest ?? prefs.importDest;
      const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
      const covered = prefs.roots.some((r) => norm(dest).startsWith(norm(r) + "/") || norm(r) === norm(dest));
      if (dest && !covered) { prefs.roots.push(dest); savePrefs(); void rescan(); }
      break;
    }
  }
  if (view === "import") queueRender();
}

async function scrapeImported() {
  const dest = imp.plan?.dest ?? prefs.importDest;
  const n = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  if (dest && !prefs.roots.some((r) => n(r) === n(dest) || n(dest).startsWith(n(r) + "/"))) prefs.roots.push(dest);
  savePrefs();
  go("library");
  await rescan();
  // Select just the consoles in the ROMs folder the games went into.
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const inDest = (p: string) => norm(p).startsWith(norm(dest) + "/");
  prefs.deselected = library.systems.filter((s) => !inDest(s.path)).map((s) => s.path);
  savePrefs();
  render();
  toast("Selected the consoles you just imported — press Continue to choose artwork.");
}

function itemBadge(it: ImportItem) {
  if (it.state.kind === "already-there") return h("span", { class: "badge done" }, "Already there");
  if (it.state.kind === "conflict") return h("span", { class: "badge attention" }, "Name clash");
  if (it.state.kind === "skipped") return h("span", { class: "badge attention" }, "Skipped");
  if (it.state.kind === "needs-system") return h("span", { class: "badge attention" }, "Choose console");
  return it.confidence === "sure" ? h("span", { class: "badge sure" }, "Checked inside") : h("span", { class: "badge likely" }, "Best guess");
}

function viewImport(): Node[] {
  const out: (Node | null)[] = [
    h("h1", {}, "Import & sort games"),
    h("p", { class: "lead" }, "Point RomCrate at a messy folder (or a .zip). It works out which console every game is for — by looking inside the files, not just their names — and puts each one into a console folder in your ROMs folder. You'll see everything before anything is copied."),
  ];

  // 1. From / to / how
  out.push(
    h("div", { class: "card" },
      h("div", { class: "field", style: "margin-top:0" }, h("label", {}, "Sort games from"),
        h("div", { class: "row" },
          h("button", { class: "btn", onclick: () => pickImportSource(false), disabled: imp.running }, "+ Add folder"),
          h("button", { class: "btn", onclick: () => pickImportSource(true), disabled: imp.running }, "+ Add .zip")),
        prefs.importSources.length ? h("div", { class: "chips", style: "margin-top:8px" }, prefs.importSources.map((r) => h("span", { class: "chip", title: r }, h("span", { class: "path" }, `‎${r}‎`),
          h("button", { class: "x", title: "Remove", disabled: imp.running, onclick: () => { prefs.importSources = prefs.importSources.filter((x) => x !== r); savePrefs(); imp.plan = null; render(); } }, "×")))) : null,
        prefs.importSources.some(isBroadFolder) ? h("div", { class: "banner info", style: "margin-top:10px" },
          "That's a whole personal folder (or drive), so RomCrate looks through everything in it — other programs' files and saves too. That's safe (it only reads), but slower. For a quicker, cleaner check, pick the folder your downloaded games are in.") : null),
      h("div", { class: "field" }, h("label", {}, "Into your ROMs folder"),
        h("div", { class: "row" },
          h("button", { class: "btn", onclick: pickImportDest, disabled: imp.running }, prefs.importDest ? "Change…" : "Choose folder…"),
          prefs.importDest ? h("span", { class: "chip" }, h("span", { class: "path" }, `‎${prefs.importDest}‎`)) : null),
        h("div", { class: "hint" }, "Console folders that already exist there (like “PS2” or “Nintendo 64”) are reused; others are created (snes, psx, ps2…).")),
      h("div", { class: "field" }, h("label", {}, "How"),
        seg([["copy", "Copy (keep originals)"], ["move", "Move"]], prefs.importMode, (v) => { prefs.importMode = v; savePrefs(); imp.plan = null; render(); }),
        h("div", { class: "hint" }, prefs.importMode === "copy"
          ? "Your original files stay exactly where they are."
          : "Each file is copied and checked first; the original is removed only once the copy is confirmed identical. On the same drive it's an instant move.")),
      h("div", { style: "margin-top:12px" },
        toggle(prefs.importRename, "Rename to official names", "Games identified from their contents get their official No-Intro / Redump name (single-file games only).", () => { prefs.importRename = !prefs.importRename; imp.plan = null; })),
      h("div", { class: "row", style: "margin-top:16px" },
        h("button", { class: "btn primary", disabled: !prefs.importSources.length || !prefs.importDest || imp.planning || imp.running, onclick: previewImport },
          imp.planning ? "Looking at your files…" : imp.plan ? "Preview again" : "Preview"),
        h("button", { class: "btn", disabled: !prefs.importSources.length || checkState.running || imp.running, onclick: checkMyFiles },
          checkState.running ? "Checking…" : "Check my files")),
    ),
  );
  out.push(...checkPanel());

  const p = imp.plan;
  if (!p) {
    out.push(h("div", { class: "footer-bar" },
      h("button", { class: "btn", onclick: () => go("home") }, "← Home"),
      h("button", { class: "btn", onclick: () => go("library") }, "Skip — my games are already sorted →")));
    return out.filter((x): x is Node => !!x);
  }

  // 2. Preview
  const todo = p.items.filter(willImport);
  const todoBytes = todo.reduce((n, i) => n + i.size, 0);
  const attention = p.items.filter((i) => i.state.kind === "needs-system" && !imp.assign[i.id] && !imp.exclude.has(i.id)).length;
  const already = p.items.filter((i) => i.state.kind === "already-there").length;
  const sameDriveMove = p.mode === "move" && p.neededBytes === 0;
  const needed = sameDriveMove ? 0 : todoBytes;
  const enough = p.freeBytes == null || needed === 0 || p.freeBytes >= needed + 268435456;

  out.push(
    h("h2", {}, "Preview"),
    h("div", { class: "summary-grid" },
      h("div", { class: "stat ok" }, h("div", { class: "v" }, String(todo.length)), h("div", { class: "l" }, `To ${p.mode === "move" ? "move" : "copy"} (${bytes(todoBytes)})`)),
      h("div", { class: "stat" }, h("div", { class: "v" }, String(already)), h("div", { class: "l" }, "Already in your ROMs folder")),
      h("div", { class: `stat ${attention ? "warn" : ""}` }, h("div", { class: "v" }, String(attention)), h("div", { class: "l" }, "Need you to choose a console")),
      h("div", { class: `stat ${enough ? "" : "warn"}` }, h("div", { class: "v" }, p.freeBytes == null ? "?" : bytes(p.freeBytes)), h("div", { class: "l" }, `Free on that drive · needs ${bytes(needed)}`))),
    !enough ? h("div", { class: "banner warn" }, `Not enough space: this needs ${bytes(needed)} plus a little room to spare, but only ${p.freeBytes == null ? "?" : bytes(p.freeBytes)} is free. Untick some games or free up space.`) : null,
    p.systems.length ? h("div", { class: "sys-chips" }, p.systems.map((s) => h("span", { class: "chip" }, `${s.name} · ${s.games} → ${s.folder}/`))) : null,
    ...p.notes.map((n) => h("div", { class: "desc", style: "color:var(--faint)" }, n)),
    p.ignoredFiles ? h("div", { class: "desc", style: "color:var(--faint)" }, `${p.ignoredFiles} non-game files (text, pictures, installers…) are left alone.`) : null,
  );

  // 3. Progress / result (shown right under the preview numbers)
  const prog: (Node | null)[] = [];
  if (imp.running || imp.summary) {
    const pct = imp.total ? Math.round((imp.done / imp.total) * 100) : imp.summary ? 100 : 0;
    prog.push(h("h2", { id: "imp-progress" }, imp.running ? "Importing…" : "Done"),
      h("div", { class: "progress" }, h("div", { style: `width:${pct}%` })),
      h("div", { class: "progress-label" }, h("span", {}, imp.running ? imp.current : imp.summary?.cancelled ? "Stopped" : "Finished"), h("span", {}, `${bytes(imp.done)} of ${bytes(imp.total)}`)));
    if (imp.running) prog.push(h("button", { class: "btn danger", onclick: () => invoke("cancel_scrape") }, "Stop"));
    const s = imp.summary;
    if (s) {
      prog.push(h("div", { class: s.failed ? "banner warn" : "banner" },
        `${s.imported} game${s.imported === 1 ? "" : "s"} ${p.mode === "move" ? "moved" : "copied"} and checked (${bytes(s.bytes)}). ${s.alreadyThere} already there, ${s.skipped} left out${s.failed ? `, ${s.failed} failed` : ""}.`),
        ...s.errors.map((e) => h("div", { class: "banner warn" }, e)),
        h("div", { class: "row" },
          h("button", { class: "btn primary big", onclick: scrapeImported }, "Get artwork for these games →"),
          h("button", { class: "btn", onclick: () => invoke("open_folder", { path: p.dest }).catch((e) => toast(String(e))) }, "Open ROMs folder")));
    }
    if (imp.log.length) prog.push(h("h2", {}, "Activity"), h("div", { class: "log" }, imp.log.slice(-2000).map((l) => h("div", { class: l.ok ? "" : "error" }, `${l.ok ? "✓" : "✗"} ${l.message}`))), logButtons());
  }
  if (!imp.running && !imp.summary) {
    out.push(h("div", { class: "footer-bar" }, h("span", {}),
      h("button", { class: "btn primary", disabled: !todo.length || !enough, onclick: startImport },
        `${p.mode === "move" ? "Move" : "Copy"} ${todo.length} game${todo.length === 1 ? "" : "s"} →`)));
  }
  out.push(...prog);

  const consoleOptions = [["", "Choose console…"], ["pc", "PC (Windows) discs"], ...catalog.systems.map((s) => [s.id, s.name])];
  out.push(h("div", { class: "imp-list" }, p.items.map((it) => {
    const on = willImport(it);
    const canTick = it.state.kind === "ready" || it.state.kind === "needs-system";
    const target = it.state.kind === "needs-system"
      ? (imp.assign[it.id] ? `→ ${imp.assign[it.id]}/${it.destName}` : "")
      : it.destFolder ? `→ ${it.destFolder}/${it.destName}` : "";
    return h("div", { class: `imp ${on || it.state.kind === "already-there" ? "" : "off"}` },
      h("div", {
        class: `check ${on ? "on" : ""}`, style: canTick && !imp.running ? "cursor:pointer" : "opacity:.35",
        onclick: () => { if (!canTick || imp.running) return; if (imp.exclude.has(it.id)) imp.exclude.delete(it.id); else imp.exclude.add(it.id); render(); },
      }),
      h("div", {},
        h("div", { class: "n" }, it.name, it.files.length > 1 ? h("span", { style: "color:var(--faint);font-weight:400" }, `  + ${it.files.length - 1} file${it.files.length > 2 ? "s" : ""}`) : null),
        h("div", { class: "why" }, `${it.systemName ? it.systemName + " — " : ""}${it.reason} · ${bytes(it.size)}`),
        target ? h("div", { class: "to" }, target) : null,
        it.state.detail ? h("div", { class: "warnline" }, it.state.detail) : null,
        ...it.warnings.map((w) => h("div", { class: "warnline" }, w)),
        it.state.kind === "needs-system" ? h("select", {
          class: "input", style: "margin-top:8px;max-width:280px", disabled: imp.running,
          onchange: (e: Event) => { const v = (e.target as HTMLSelectElement).value; if (v) imp.assign[it.id] = v; else delete imp.assign[it.id]; render(); },
        }, consoleOptions.map(([v, l]) => h("option", { value: v, selected: (imp.assign[it.id] ?? "") === v }, l))) : null),
      itemBadge(it));
  })));

  return out.filter((x): x is Node => !!x);
}



// ---------------------------------------------------------------- game settings

interface KeyChange { section: string | null; key: string; label: string; why: string; old: string | null; new: string; tested: boolean }
interface FileChange { path: string; exists: boolean; keys: KeyChange[] }
interface ConsoleTune { id: string; systems: string[]; emulator: string; status: string; message: string | null; changes: FileChange[]; steps: string[]; tested: boolean; source: string; packages: string[] }
interface TunePlan { screens: string; consoles: ConsoleTune[] }
interface ApplyResult { changedFiles: number; failed: number; lines: string[] }

interface LadderStep { value: string; label: string }
interface GameRow { system: string; file: string; name: string; id: string | null; kind: string; label: string; ladder: LadderStep[]; consoleValue: string; ownValue: string | null; suggestion: { value: string; why: string; tested: boolean } | null; note: string | null; settingsFile: string | null; step: string | null }

const tune = {
  tab: "console" as "console" | "game",
  games: null as GameRow[] | null, scanning: false, scanPct: 0, filter: "",
  /** file → chosen value (null = console setting); missing = unchanged */
  pick: new Map<string, string | null>(),
  plan: null as TunePlan | null, checking: false, applying: false,
  chosen: new Set<string>(), result: null as ApplyResult | null, resultKind: "" as "" | "apply" | "undo",
};

function tuneSystems(): string[] {
  return [...new Set((library?.systems ?? []).map((s) => s.system_id))];
}

async function tuneCheck() {
  tune.checking = true; tune.result = null; render();
  try {
    tune.plan = await invoke<TunePlan>("settings_plan", { serial: snd.serial, systems: tuneSystems() });
    tune.chosen = new Set(tune.plan.consoles.filter((c) => c.status === "ready").map((c) => c.id));
  } catch (e) { toast(String(e)); }
  tune.checking = false; render();
}

async function tuneApply() {
  const names = [...new Set(tune.plan!.consoles.filter((c) => tune.chosen.has(c.id)).map((c) => c.emulator.split(" · ")[0]))];
  if (!confirm(`RomCrate will close ${names.join(", ")} on the handheld before changing settings, so they can't overwrite them.\n\nFinish and save any game that's running first. Continue?`)) return;
  tune.applying = true; render();
  try {
    tune.result = await invoke<ApplyResult>("settings_apply", { ids: [...tune.chosen] }); tune.resultKind = "apply";
    toast(tune.result.failed ? `${tune.result.failed} file(s) couldn't be changed — see below` : "Settings applied. Start a game to see the difference.");
  } catch (e) { toast(String(e)); }
  tune.applying = false; tune.plan = null; render();
}

async function tuneUndo() {
  if (!confirm("Put back the emulator settings as they were before RomCrate changed them?\n\nRomCrate will close the emulators first.")) return;
  tune.applying = true; render();
  try {
    tune.result = await invoke<ApplyResult>("settings_undo", { serial: snd.serial, systems: tuneSystems() }); tune.resultKind = "undo";
    toast(tune.result.changedFiles ? "Original settings put back." : "Nothing to undo — RomCrate hadn't changed anything.");
  } catch (e) { toast(String(e)); }
  tune.applying = false; tune.plan = null; render();
}


// ---------- each game

async function gamesScan(keepResult = false) {
  tune.scanning = true; tune.scanPct = 0; if (!keepResult) tune.result = null; render();
  try {
    tune.games = await invoke<GameRow[]>("pergame_scan", { serial: snd.serial });
    tune.pick = new Map();
  } catch (e) { toast(String(e)); }
  tune.scanning = false; render();
}

/** What a game is set to now (own value, else console). */
const gameNow = (g: GameRow) => g.kind === "checklist" ? (prefs.ps2Picks[g.file] ?? null) : g.ownValue;
const gameChosen = (g: GameRow) => tune.pick.has(g.file) ? tune.pick.get(g.file)! : gameNow(g);
const stepLabel = (g: GameRow, v: string | null) => v === null ? `Console setting: ${g.ladder.find((l) => l.value === g.consoleValue)?.label ?? g.consoleValue}` : g.ladder.find((l) => l.value === v)?.label ?? v;

function gameStep(g: GameRow, dir: -1 | 1) {
  const cur = gameChosen(g) ?? g.consoleValue;
  const i = g.ladder.findIndex((l) => l.value === cur);
  const j = Math.max(0, Math.min(g.ladder.length - 1, (i < 0 ? 0 : i) + dir));
  setPick(g, g.ladder[j].value === g.consoleValue && gameNow(g) === null ? null : g.ladder[j].value);
}

function setPick(g: GameRow, v: string | null) {
  if (v === gameNow(g)) tune.pick.delete(g.file); else tune.pick.set(g.file, v);
  render();
}

async function gamesApply() {
  const changes = [...tune.pick.entries()];
  const rows = tune.games!.filter((g) => tune.pick.has(g.file));
  // PS2 (ARMSX2) choices are remembered here and shown as checklist lines.
  for (const g of rows.filter((g) => g.kind === "checklist")) {
    const v = tune.pick.get(g.file)!;
    if (v === null) delete prefs.ps2Picks[g.file]; else prefs.ps2Picks[g.file] = v;
  }
  savePrefs();
  const choices = rows.filter((g) => g.kind !== "checklist" && g.settingsFile).map((g) => ({ system: g.system, file: g.file, settingsFile: g.settingsFile!, value: tune.pick.get(g.file)! }));
  if (choices.length) {
    if (!confirm(`RomCrate will close the emulators for these ${choices.length} game(s) on the handheld before changing their settings.\n\nFinish and save any game that's running first. Continue?`)) return;
    tune.applying = true; render();
    try {
      tune.result = await invoke<ApplyResult>("pergame_apply", { serial: snd.serial, choices }); tune.resultKind = "apply";
      toast(tune.result.failed ? `${tune.result.failed} game(s) couldn't be changed — see below` : `${choices.length} game(s) updated.`);
    } catch (e) { toast(String(e)); }
    tune.applying = false;
    await gamesScan(true);
  } else {
    tune.pick = new Map(); render();
    toast(`${changes.length} PS2 choice(s) saved — set them in ARMSX2 using the lines below.`);
  }
}

function viewGames(out: (Node | null)[]) {
  if (!tune.games) {
    out.push(h("div", { class: "card" },
      h("div", { class: "desc" }, "RomCrate lists every game on the handheld that can have its own setting (PS1, N64, Wii/GameCube, PSP, PS2), reads what each is set to now, and suggests changes where there's a reason. Nothing is changed until you apply."),
      h("div", { class: "row", style: "margin-top:10px" },
        h("button", { class: "btn primary", disabled: tune.scanning, onclick: () => gamesScan() }, tune.scanning ? (tune.scanPct ? `Getting the games database… ${tune.scanPct}%` : "Reading your games…") : "Check my games"))));
    return;
  }
  const games = tune.games;
  if (!games.length) { out.push(h("div", { class: "card" }, "No games found on the handheld that can have their own settings.")); return; }
  const suggested = games.filter((g) => g.suggestion && gameChosen(g) !== g.suggestion.value);
  out.push(h("div", { class: "banner info", style: "margin-bottom:12px" }, h("div", {},
    "Each game uses its console's setting unless you give it its own. Played a game and it ", h("b", {}, "runs slow"), "? Step it down. Runs perfectly but ", h("b", {}, "could look sharper"), "? Step it up. RomCrate writes that game's own settings file and keeps everything else as it was.")));
  out.push(h("div", { class: "row", style: "margin-bottom:10px" },
    h("input", { type: "search", class: "game-search", placeholder: "Find a game…", value: tune.filter, oninput: (e: Event) => {
      // Filter in place (a redraw would take the cursor out of the box).
      tune.filter = (e.target as HTMLInputElement).value;
      const q = tune.filter.toLowerCase();
      document.querySelectorAll<HTMLElement>(".game-row").forEach((r) => { r.style.display = !q || (r.dataset.name ?? "").includes(q) ? "" : "none"; });
    } }),
    suggested.length ? h("button", { class: "btn", onclick: () => { suggested.forEach((g) => tune.pick.set(g.file, g.suggestion!.value)); render(); } }, `Use all ${suggested.length} suggestion${suggested.length === 1 ? "" : "s"}`) : null));
  const f = tune.filter.toLowerCase();
  const bySys = new Map<string, GameRow[]>();
  for (const g of games) bySys.set(g.system, [...(bySys.get(g.system) ?? []), g]);
  for (const [sys, list] of bySys) {
    out.push(h("h3", { class: "sys-h" }, `${SHORT_SYS[sys] ?? sys.toUpperCase()} · ${list[0].label}`),
      h("div", { class: "card games-card" }, list.map((g) => {
        const chosen = gameChosen(g);
        const changed = tune.pick.has(g.file);
        return h("div", { class: `game-row ${changed ? "changed" : ""}`, "data-name": g.name.toLowerCase(), style: f && !g.name.toLowerCase().includes(f) ? "display:none" : "" },
          h("div", { class: "gname" }, h("div", { class: "t" }, g.name),
            g.note ? h("div", { class: "hint" }, g.note) :
            g.suggestion ? h("div", { class: "hint sugg" }, `Suggested: ${stepLabel(g, g.suggestion.value)} — ${g.suggestion.why}${g.suggestion.tested ? " ✓ tested" : ""}`) : null,
            g.kind === "checklist" && chosen !== null ? h("div", { class: "hint" }, (g.step ?? "").replace("{label}", stepLabel(g, chosen))) : null),
          g.note ? h("span", {}) : h("div", { class: "gctl" },
            h("button", { class: "btn small", title: "Runs slow: one step lower", disabled: tune.applying, onclick: () => gameStep(g, -1) }, "Runs slow"),
            h("select", { disabled: tune.applying, onchange: (e: Event) => { const v = (e.target as HTMLSelectElement).value; setPick(g, v === "" ? null : v); } },
              h("option", { value: "", selected: chosen === null }, stepLabel(g, null)),
              g.ladder.map((l) => h("option", { value: l.value, selected: chosen === l.value }, l.label))),
            h("button", { class: "btn small", title: "Could look sharper: one step higher", disabled: tune.applying, onclick: () => gameStep(g, 1) }, "Could look sharper")));
      })));
  }
  const n = tune.pick.size;
  out.push(h("div", { class: "row", style: "margin-top:12px" },
    h("button", { class: "btn primary big", disabled: !n || tune.applying, onclick: gamesApply }, tune.applying ? "Applying…" : `Apply ${n} change${n === 1 ? "" : "s"}`),
    n ? h("button", { class: "btn", disabled: tune.applying, onclick: () => { tune.pick = new Map(); render(); } }, "Forget changes") : null,
    h("button", { class: "btn", disabled: tune.applying || tune.scanning, onclick: () => gamesScan() }, "Check again")));
  const ps2 = games.filter((g) => g.kind === "checklist" && prefs.ps2Picks[g.file]);
  if (ps2.length) out.push(h("h3", { class: "sys-h" }, "PS2 games to set in ARMSX2"), h("ul", { class: "steps-list" }, ps2.map((g) => {
    const key = `ps2game:${g.file}:${prefs.ps2Picks[g.file]}`;
    return h("li", {}, h("label", {}, h("input", { type: "checkbox", checked: !!prefs.tuneTicks[key], onchange: (e: Event) => { prefs.tuneTicks[key] = (e.target as HTMLInputElement).checked; savePrefs(); } }), " ", h("b", {}, g.name), " — ", (g.step ?? "").replace("{label}", stepLabel(g, prefs.ps2Picks[g.file]))));
  })));
}

const TUNE_BADGE: Record<string, [string, string]> = {
  ready: ["ok", "Ready to apply"], already: ["ok", "Already set"], checklist: ["warn", "Set it in the app"],
  "not-installed": ["muted", "Emulator not installed"], problem: ["bad", "Needs a step first"],
};

function viewTune(): Node[] {
  const out: (Node | null)[] = [
    h("h1", {}, "Game settings"),
    h("p", { class: "lead" }, "Graphics and performance settings for each console's emulator on your handheld — sharp, full-speed, and right for both the handheld screen and the TV. RomCrate changes only these settings, keeps a backup of every file it touches, and can put everything back."),
  ];
  if (!handheldGate(out, "Changing settings on the handheld")) return out.filter((x): x is Node => !!x);
  out.push(h("div", { class: "seg", style: "margin:18px 0 6px" },
    h("button", { class: tune.tab === "console" ? "on" : "", onclick: () => { tune.tab = "console"; render(); } }, "Each console"),
    h("button", { class: tune.tab === "game" ? "on" : "", onclick: () => { tune.tab = "game"; render(); } }, "Each game")));
  if (tune.tab === "game") {
    out.push(h("h2", {}, "2 · Your games"));
    viewGames(out);
    if (tune.result) out.push(h("h2", {}, "Applied"), h("div", { class: "log" }, tune.result.lines.map((l) => h("div", { class: l.startsWith("FAILED") ? "error" : "" }, l))), logButtons());
    return out.filter((x): x is Node => !!x);
  }
  out.push(h("h2", {}, "2 · Your emulators"));
  if (!tune.plan) {
    out.push(h("div", { class: "card" },
      h("div", { class: "desc" }, "RomCrate looks at the consoles in your game folders and on the handheld, and at the emulators installed there. Nothing is changed yet."),
      h("div", { class: "row", style: "margin-top:10px" },
        h("button", { class: "btn primary", disabled: tune.checking || tune.applying, onclick: tuneCheck }, tune.checking ? "Looking at the handheld…" : "Check my emulators"),
        h("button", { class: "btn", disabled: tune.checking || tune.applying, onclick: tuneUndo }, "Undo RomCrate's changes"))));
  } else {
    const p = tune.plan;
    out.push(h("div", { class: "banner info", style: "margin-bottom:12px" }, `${p.screens} If you later use a 4K TV, these still work — they just won't use the extra sharpness.`));
    if (!p.consoles.length) out.push(h("div", { class: "card" }, "No consoles found that RomCrate has settings for yet."));
    for (const c of p.consoles) {
      const [cls, label] = TUNE_BADGE[c.status] ?? ["muted", c.status];
      const names = c.systems.map((id) => SHORT_SYS[id] ?? id.toUpperCase()).join(" / ");
      const keys = c.changes.flatMap((f) => f.keys);
      const allTested = c.status === "checklist" ? c.tested : keys.length > 0 && keys.every((k) => k.tested);
      const someTested = keys.some((k) => k.tested);
      out.push(h("div", { class: "card tune" },
        h("div", { class: "tune-head" },
          c.status === "ready" ? h("input", { type: "checkbox", checked: tune.chosen.has(c.id), onchange: (e: Event) => { (e.target as HTMLInputElement).checked ? tune.chosen.add(c.id) : tune.chosen.delete(c.id); render(); } }) : null,
          h("div", {}, h("div", { class: "t" }, `${names} · ${c.emulator}`),
            h("div", { class: "desc" }, c.status === "ready" || c.status === "already" || c.status === "checklist" ? (allTested ? "✓ Tested on the AYN Odin 2 Portal" : someTested ? "Partly tested on the AYN Odin 2 Portal (marked ✓ below); the rest are suggestions" : "Not tested on a device yet — suggested from the emulator's own docs") : "")),
          h("span", { class: `pill ${cls}` }, label)),
        c.message ? h("div", { class: "hint", style: "margin-top:6px" }, c.message) : null,
        c.changes.length ? h("table", { class: "tune-table" },
          h("tr", {}, h("th", {}, "Setting"), h("th", {}, "Now"), h("th", {}, "Will be"), h("th", {}, "Why")),
          c.changes.flatMap((f) => f.keys.map((k) => h("tr", {},
            h("td", {}, k.label, k.tested ? h("span", { class: "tiny ok" }, " ✓ tested") : null),
            h("td", { class: "mono" }, k.old ?? "—"), h("td", { class: "mono" }, k.new), h("td", { class: "desc" }, k.why))))) : null,
        c.steps.length ? h("ul", { class: "steps-list" }, c.steps.map((st, i) => {
          const key = `${c.id}:${i}`;
          return h("li", {}, h("label", {}, h("input", { type: "checkbox", checked: !!prefs.tuneTicks[key], onchange: (e: Event) => { prefs.tuneTicks[key] = (e.target as HTMLInputElement).checked; savePrefs(); } }), " ", st));
        })) : null));
    }
    const n = tune.chosen.size;
    out.push(h("div", { class: "row", style: "margin-top:12px" },
      h("button", { class: "btn primary big", disabled: !n || tune.applying, onclick: tuneApply }, tune.applying ? "Applying…" : `Apply to handheld (${n} console${n === 1 ? "" : "s"})`),
      h("button", { class: "btn", disabled: tune.applying, onclick: tuneCheck }, "Check again"),
      h("button", { class: "btn", disabled: tune.applying, onclick: tuneUndo }, "Undo RomCrate's changes")),
      h("div", { class: "hint", style: "margin-top:6px" }, "Close the emulators first? RomCrate does it for you when you apply, so they can't overwrite the new settings. Save your game before applying."));
  }
  if (tune.result) {
    out.push(h("h2", {}, tune.resultKind === "undo" ? "Undo" : "Applied"),
      h("div", { class: "log" }, tune.result.lines.length ? tune.result.lines.map((l) => h("div", { class: l.startsWith("FAILED") ? "error" : "" }, l)) : h("div", {}, "Nothing needed changing.")),
      logButtons());
  }
  return out.filter((x): x is Node => !!x);
}

// ---------------------------------------------------------------- send to handheld

interface AdbDevice { serial: string; state: string; model: string | null }
interface Volume { path: string; label: string; sizeBytes: number; freeBytes: number; romsDir: string | null; consoleFolders: string[] }
interface SendPlan { files: { local: string; remote: string; bytes: number }[]; alreadyThere: number; conflicts: string[]; perVolume: { volume: string; label: string; bytes: number; freeBytes: number; fits: boolean }[]; folders: [string, string][]; notes: string[] }
interface SendSummary { copied: number; verified: number; failed: number; alreadyThere: number; conflicts: number; bytes: number; cancelled: boolean }

const snd = {
  status: null as null | { found: boolean; path: string | null; version: string | null; source: string | null },
  terms: "",
  installing: false, installPct: 0,
  devices: [] as AdbDevice[], serial: "", volumes: [] as Volume[], error: "",
  plan: null as SendPlan | null, planning: false,
  running: false, done: 0, total: 0, summary: null as SendSummary | null,
  log: [] as { ok: boolean; message: string }[],
  poll: 0 as number,
};

const human = (b: number) => b >= 1073741824 ? `${(b / 1073741824).toFixed(1)} GB` : b >= 1048576 ? `${Math.round(b / 1048576)} MB` : `${Math.ceil(b / 1024)} KB`;

async function sendRefresh() {
  const first = !snd.status;
  try {
    if (!snd.status) { const r = await invoke<{ status: typeof snd.status; termsUrl: string }>("adb_status"); snd.status = r.status; snd.terms = r.termsUrl; }
    if (!snd.status?.found) { if ((view === "send" || view === "tune") && first) render(); return; }
    const devs = await invoke<AdbDevice[]>("adb_devices");
    const changed = JSON.stringify(devs) !== JSON.stringify(snd.devices);
    snd.devices = devs; snd.error = "";
    const ready = devs.filter((d) => d.state === "device");
    if (!ready.some((d) => d.serial === snd.serial)) { snd.serial = ready[0]?.serial ?? ""; snd.volumes = []; snd.plan = null; }
    let volsChanged = false;
    if (snd.serial && !snd.volumes.length) { snd.volumes = await invoke<Volume[]>("adb_volumes", { serial: snd.serial }); volsChanged = true; }
    // Only redraw when something actually changed — the check runs every few
    // seconds and a redraw would close a drop-down you have open.
    if ((changed || volsChanged || first) && (view === "send" || view === "tune")) render();
  } catch (e) {
    const was = snd.error;
    snd.error = String(e);
    if ((view === "send" || view === "tune") && was !== snd.error) render();
  }
}

function startSendPolling() {
  if (snd.poll) return;
  snd.poll = window.setInterval(() => { if (view !== "send" && view !== "tune") { clearInterval(snd.poll); snd.poll = 0; return; } if (!snd.running && !snd.planning) void sendRefresh(); }, 3000);
}

async function installAdb() {
  snd.installing = true; snd.installPct = 0; render();
  try {
    await invoke<string>("adb_install");
    snd.status = null; toast("Android's USB tool is ready.");
  } catch (e) { toast(`Couldn't get the USB tool: ${e}`); }
  snd.installing = false;
  await sendRefresh();
}

/** Volume chosen for a console folder: your choice, else one that already has a ROMs folder, else the SD card, else internal. */
function volumeFor(path: string): string {
  const v = prefs.sendTargets[path];
  if (v && snd.volumes.some((x) => x.path === v)) return v;
  return (snd.volumes.find((x) => x.romsDir && !x.path.includes("emulated")) ?? snd.volumes.find((x) => x.romsDir) ?? snd.volumes.find((x) => !x.path.includes("emulated")) ?? snd.volumes[0])?.path ?? "";
}

async function planSend() {
  snd.planning = true; snd.plan = null; render();
  try {
    const targets = selectedSystems().map((s) => ({ local: s.path, systemId: s.system_id, volume: volumeFor(s.path) }));
    snd.plan = await invoke<SendPlan>("send_plan", { serial: snd.serial, targets });
  } catch (e) { toast(String(e)); }
  snd.planning = false; render();
}

async function startSend() {
  snd.running = true; snd.done = 0; snd.total = snd.plan?.files.length ?? 0; snd.summary = null; snd.log = []; render();
  try { await invoke("start_send"); } catch (e) { snd.running = false; toast(String(e)); render(); }
}

function handleSendEvent(e: any) {
  if (e.type === "started") snd.total = e.files;
  if (e.type === "file") { snd.done++; snd.log.push({ ok: e.ok, message: `${e.remote} — ${e.message}` }); }
  if (e.type === "finished") {
    snd.running = false; snd.summary = e.summary; snd.plan = null; snd.volumes = [];
    toast(e.summary.failed ? `Finished with ${e.summary.failed} problem(s) — see the log` : "All sent!");
    void sendRefresh();
  }
  if (view === "send") queueRender(); else renderNav();
}

/** The "USB tool + handheld connected" part shared by Send and Game settings.
 * Adds its cards to `out` and returns true when a handheld is ready. */
function handheldGate(out: (Node | null)[], purpose: string): boolean {
  if (!snd.status && !snd.error) void sendRefresh();
  startSendPolling();
  const st = snd.status;
  if (!st) { out.push(h("div", { class: "card" }, "Checking…")); return false; }
  if (!st.found) {
    out.push(h("h2", {}, "1 · Android's USB tool"),
      h("div", { class: "card" },
        h("div", { class: "desc" }, `${purpose} needs “adb”, Android's own USB tool from Google (about 10 MB, part of “platform-tools”). RomCrate downloads it from dl.google.com, checks it against Google's published checksum, and keeps it in RomCrate's folder. It's never bundled with RomCrate.`),
        h("div", { class: "hint", style: "margin-top:6px" }, "By downloading you accept Google's terms for platform-tools: ", h("span", { class: "path" }, snd.terms)),
        h("div", { class: "row", style: "margin-top:10px" },
          h("button", { class: "btn primary", disabled: snd.installing, onclick: installAdb }, snd.installing ? `Downloading… ${snd.installPct}%` : "Download the USB tool from Google"))));
    return false;
  }
  const ready = snd.devices.filter((d) => d.state === "device");
  const waiting = snd.devices.filter((d) => d.state !== "device");
  out.push(h("h2", {}, "1 · Your handheld"));
  if (ready.length) {
    out.push(h("div", { class: "card" },
      ready.length > 1 ? h("select", { onchange: (e: Event) => { snd.serial = (e.target as HTMLSelectElement).value; snd.volumes = []; snd.plan = null; tune.plan = null; void sendRefresh(); } },
        ready.map((d) => h("option", { value: d.serial, selected: d.serial === snd.serial }, d.model ?? d.serial))) :
        h("div", { class: "v" }, `✓ ${ready[0].model ?? ready[0].serial} connected`),
      snd.volumes.length ? h("div", { class: "desc", style: "margin-top:6px" }, snd.volumes.map((v) => `${v.label}: ${human(v.freeBytes)} free${v.romsDir ? ` · has ${v.romsDir}/` : ""}`).join("   ·   ")) : h("div", { class: "desc" }, "Reading storage…")));
    return true;
  }
  out.push(h("div", { class: "card" },
    waiting.some((d) => d.state === "unauthorized")
      ? h("div", { class: "banner warn" }, h("div", {}, h("b", {}, "Almost there: "), "unlock the handheld and tap “Allow” on the “Allow USB debugging?” message. Tick “Always allow from this computer” so it doesn't ask again."))
      : waiting.length ? h("div", { class: "banner warn" }, "The handheld is connected but not responding — unplug it and plug it back in.")
      : h("div", {}, h("div", { class: "v" }, "No handheld found yet"),
        h("ol", { class: "how" },
          h("li", {}, "On the handheld: Settings → About (phone/device) → tap “Build number” 7 times until it says you're a developer."),
          h("li", {}, "Settings → System → Developer options → turn on “USB debugging”."),
          h("li", {}, "Plug it in with a USB cable that carries data, and tap “Allow” when asked."),
          h("li", {}, "This screen notices it within a few seconds.")),
        h("div", { class: "hint" }, "USB debugging only lets this computer copy files and run Android's own commands while you allow it. You can turn it off again afterwards.")),
    snd.error ? h("div", { class: "hint", style: "color:var(--bad);margin-top:8px" }, snd.error) : null));
  return false;
}

function viewSend(): Node[] {
  const out: (Node | null)[] = [
    h("h1", {}, "Send to handheld"),
    h("p", { class: "lead" }, "Copies your games, artwork and Pegasus files to your Android handheld over the USB cable — into internal storage or the SD card, into the console folders already there. Nothing on the handheld is overwritten or deleted, and every game is checked after copying."),
  ];
  if (!handheldGate(out, "Sending")) {
    out.push(logPanelSend());
    return out.filter((x): x is Node => !!x);
  }
  // 3. What goes where.
  const systems = selectedSystems();
  out.push(h("h2", {}, "2 · What goes where"));
  if (!systems.length) {
    out.push(h("div", { class: "banner warn" }, "Choose your game folders first."), h("button", { class: "btn primary", onclick: () => go("library") }, "Choose game folders →"));
    return out.filter((x): x is Node => !!x);
  }
  out.push(h("div", { class: "card emu-list" }, systems.map((s) => {
    const vol = volumeFor(s.path);
    return h("div", { class: "emu-row" },
      h("div", {}, h("div", { class: "t" }, s.system_name), h("div", { class: "desc" }, `${s.games.length} game${s.games.length === 1 ? "" : "s"} · from ${shortPath(s.path)}`)),
      h("span", {}),
      h("select", { disabled: snd.running, onchange: (e: Event) => { prefs.sendTargets[s.path] = (e.target as HTMLSelectElement).value; savePrefs(); snd.plan = null; render(); } },
        snd.volumes.map((v) => h("option", { value: v.path, selected: v.path === vol }, v.label))));
  })));
  if (!snd.plan && !snd.running && !snd.summary)
    out.push(h("div", { class: "row", style: "margin-top:12px" }, h("button", { class: "btn primary", disabled: snd.planning || !snd.volumes.length, onclick: planSend }, snd.planning ? "Looking at the handheld…" : "Check what will be copied")));
  // 4. The plan.
  const p = snd.plan;
  if (p && !snd.running) {
    const bytes = p.files.reduce((a, f) => a + f.bytes, 0);
    const full = p.perVolume.filter((v) => !v.fits);
    out.push(h("h2", {}, "3 · Ready"),
      h("div", { class: "card" },
        h("div", { class: "v" }, p.files.length ? `${p.files.length} files to copy (${human(bytes)})` : "Everything is already on the handheld"),
        h("div", { class: "desc", style: "margin-top:4px" }, `${p.alreadyThere} already there (skipped)${p.conflicts.length ? ` · ${p.conflicts.length} left alone because a different file with the same name is on the handheld` : ""}`),
        h("ul", { class: "desc", style: "margin:8px 0 0;padding-left:18px" }, p.folders.map(([from, to]) => h("li", {}, `${shortPath(from)}  →  ${to}`))),
        ...p.notes.map((n) => h("div", { class: "hint" }, n)),
        ...full.map((v) => h("div", { class: "banner warn", style: "margin-top:8px" }, `Not enough space on ${v.label}: needs ${human(v.bytes)} plus 200 MB spare, ${human(v.freeBytes)} free. Pick the other storage for some consoles, or free up space.`)),
        h("div", { class: "row", style: "margin-top:12px" },
          h("button", { class: "btn primary big", disabled: !p.files.length || full.length > 0, onclick: startSend }, "Send to handheld"),
          h("button", { class: "btn", onclick: planSend }, "Check again"))));
  }
  if (snd.running || snd.summary) {
    const pct = snd.total ? Math.round((snd.done / snd.total) * 100) : 0;
    out.push(h("h2", {}, snd.running ? "Sending…" : "Done"),
      h("div", { class: "progress" }, h("div", { style: `width:${snd.running ? pct : 100}%` })),
      h("div", { class: "desc" }, snd.running ? `${snd.done} of ${snd.total} files` :
        `${snd.summary!.copied} copied (${snd.summary!.verified} games checked by checksum) · ${snd.summary!.alreadyThere} already there · ${snd.summary!.failed} failed${snd.summary!.cancelled ? " · stopped early" : ""}`),
      snd.running ? h("button", { class: "btn", style: "margin-top:10px", onclick: () => invoke("cancel_scrape") }, "Stop after this file") :
        h("button", { class: "btn", style: "margin-top:10px", onclick: () => { snd.summary = null; render(); } }, "Send more"));
  }
  out.push(logPanelSend());
  return out.filter((x): x is Node => !!x);
}

function logPanelSend(): Node | null {
  if (!snd.log.length) return null;
  const wrap = h("div", {});
  wrap.append(h("h2", {}, "Activity"), h("div", { class: "log" }, snd.log.slice(-2000).map((l) => h("div", { class: l.ok ? "" : "error" }, `${l.ok ? "✓" : "✗"} ${l.message}`))), logButtons());
  return wrap;
}

// ---------------------------------------------------------------- boot

async function boot() {
  catalog = await invoke<Catalog>("get_catalog");
  document.getElementById("version")!.textContent = `v${catalog.version}`;
  document.getElementById("nav-settings")!.addEventListener("click", () => go("settings"));
  document.getElementById("nav-import")!.addEventListener("click", () => go("import"));
  document.getElementById("nav-check")!.addEventListener("click", () => go("import"));
  document.getElementById("nav-art")!.addEventListener("click", () => go("library"));
  document.getElementById("nav-pegasus")!.addEventListener("click", () => go("pegasus"));
  document.getElementById("nav-home")!.addEventListener("click", () => go("home"));
  document.getElementById("nav-device")!.addEventListener("click", () => go("device"));
  view = prefs.device ? "home" : "device";
  await listen("import-event", (ev) => handleImportEvent(ev.payload));
  await listen("send-event", (ev) => handleSendEvent(ev.payload));
  await listen<{ done: number; total: number | null }>("adb-install-progress", (ev) => { snd.installPct = ev.payload.total ? Math.round((ev.payload.done / ev.payload.total) * 100) : 0; if (view === "send" || view === "tune") render(); });
  document.getElementById("nav-tune")?.addEventListener("click", () => go("tune"));
  await listen<{ done: number; total: number | null }>("pergame-progress", (ev) => { tune.scanPct = ev.payload.total ? Math.round((ev.payload.done / ev.payload.total) * 100) : 0; if (view === "tune") queueRender(); });
  document.getElementById("nav-send")?.addEventListener("click", () => go("send"));
  if (!catalog.launchers.some((l) => l.id === prefs.launcher)) prefs.launcher = "pegasus";
  await listen("scrape-event", (ev) => handleEvent(ev.payload));
  await listen<{ zip: string; doneBytes: number; totalBytes: number }>("unzip-progress", (ev) => {
    const p = ev.payload;
    unzip = { name: p.zip.split(/[\\/]/).pop() ?? "zip", pct: Math.round((p.doneBytes / Math.max(1, p.totalBytes)) * 100) };
    if (view === "library") render();
  });
  await listen<string>("scan-note", (ev) => { scanNotes.push(ev.payload); if (view === "library") render(); });
  render();
  if (prefs.roots.length) await rescan();
}

boot().catch((e) => {
  document.getElementById("main")!.replaceChildren(h("div", { class: "banner warn" }, `RomCrate failed to start: ${e}`));
});
