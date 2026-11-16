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
  importMode: "copy" | "move";
  importRename: boolean;
}

const DEFAULT_PREFS: Prefs = {
  roots: [], deselected: [], launcher: "pegasus", theme: {}, folders: {}, kinds: {},
  size: "standard", upscale: "smart", aiBinary: "", regions: ["usa", "world", "europe", "australia", "uk", "japan"],
  identify: true, overwrite: false, metadata: true, extras: true, render3d: true, threads: 4, pegasusLayout: "auto", launchbox: true, gameInfo: true,
  pegasusOn: "none", device: "", emulators: {}, fixLaunch: false,
  importDest: "", importSources: [], importMode: "copy", importRename: false,
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
let view: "home" | "device" | "pegasus" | "library" | "launcher" | "artwork" | "scrape" | "settings" | "import" = "home";
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
        || (st.id === "scrape" && !!run.summary && !run.running);
      return h("button", { class: `step ${view === st.id ? "active" : ""} ${done ? "done" : ""}`, onclick: () => go(st.id) },
        h("div", { class: "num" }, done ? "✓" : String(i + 1)),
        h("div", {}, h("div", { class: "t" }, st.t), h("div", { class: "s" }, st.s())));
    }),
  );
}

function go(v: typeof view) {
  view = v;
  render();
  document.getElementById("main")!.scrollTop = 0;
}

function render() {
  renderNav();
  const main = document.getElementById("main")!;
  const views = { home: viewHome, device: viewDevice, pegasus: viewPegasus, library: viewLibrary, launcher: viewLauncher, artwork: viewArtwork, scrape: viewScrape, settings: viewSettings, import: viewImport };
  document.getElementById("nav-home")?.classList.toggle("active", view === "home");
  document.getElementById("nav-pegasus")?.classList.toggle("active", view === "pegasus");
  const d = currentDevice();
  document.getElementById("nav-device")!.replaceChildren(
    h("div", { class: "k" }, "Games are for"),
    h("div", { class: "v" }, d ? d.name : prefs.device === "unsure" ? "Not sure — safe defaults" : "Choose your device…"));
  const keep = main.dataset.view === view ? main.scrollTop : 0;
  main.replaceChildren(...views[view]());
  main.dataset.view = view;
  main.scrollTop = keep;
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

function viewDevice(): Node[] {
  const card = (d: DeviceInfo) => h("button", { class: `device ${prefs.device === d.id ? "on" : ""}`, onclick: () => applyDevice(d.id) },
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
        h("div", { class: "n" }, "Not sure / something else"),
        h("div", { class: "f" }, "Use safe defaults"),
        h("div", { class: "spec" }, "Works for most setups. Pick a device later for better defaults."))),
    h("div", { class: "note", style: "margin-top:16px" }, "Only devices marked “Tested on this device” have been checked on a real unit. Specs for the others come from the makers and reviews."),
  ];
}

function viewHome(): Node[] {
  const d = currentDevice();
  const tasks: [string, string, string, (() => void) | null][] = [
    ["🩺 Check my files", "Find broken downloads, damaged files and disc copy protection.", "check", () => { go("import"); }],
    ["📥 Import & sort", "Put games from a messy folder into console folders.", "import", () => go("import")],
    ["🖼️ Get artwork", "Covers and screenshots — e.g. for a game you just downloaded.", "art", () => go("library")],
    ["🚀 Pegasus launch lines", "Make Pegasus start your games, without re-scraping.", "pegasus", () => go("pegasus")],
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
  if (["library", "launcher", "pegasus", "artwork", "scrape"].includes(view)) render(); else renderNav();
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
    case "log": if (e.level !== "debug") run.log.push({ level: e.level, message: e.message }); break;
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
      s ? h("button", { class: "btn primary", onclick: () => { run.summary = null; run.results = []; render(); } }, "Scrape again") : null,
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
    out.push(h("h2", {}, "Activity"), h("div", { class: "log" }, run.log.slice(-200).map((l) => h("div", { class: l.level }, l.message))));
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

let logDir = "";

function viewSettings(): Node[] {
  if (!logDir) invoke<{ dir: string }>("log_info").then((i) => { logDir = i.dir; if (view === "settings") render(); });
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
    h("h2", {}, "Activity log"),
    h("div", { class: "card" },
      h("div", { class: "desc" }, "Everything RomCrate does — games found or not found and why, files skipped, errors — is written to a daily log on this computer (kept for 30 days). Handy for checking what happened, or for sharing when asking for help. It stays on your computer."),
      logDir ? h("div", { class: "to", style: "margin-top:8px" }, `\u200E${logDir}\u200E`) : null,
      h("div", { class: "row", style: "margin-top:10px" },
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
      h("div", { class: "note", style: "margin:0 0 12px" }, catalog.cacheDir),
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
    if (imp.log.length) prog.push(h("h2", {}, "Activity"), h("div", { class: "log" }, imp.log.slice(-300).map((l) => h("div", { class: l.ok ? "" : "error" }, `${l.ok ? "✓" : "✗"} ${l.message}`))));
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
