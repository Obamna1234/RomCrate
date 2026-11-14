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
}
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
}

const DEFAULT_PREFS: Prefs = {
  roots: [], deselected: [], launcher: "pegasus", theme: {}, folders: {}, kinds: {},
  size: "standard", upscale: "smart", aiBinary: "", regions: ["usa", "world", "europe", "australia", "uk", "japan"],
  identify: true, overwrite: false, metadata: true, extras: true, render3d: true, threads: 4,
};

function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem("shelfart.prefs");
    if (raw) return { ...DEFAULT_PREFS, ...JSON.parse(raw) };
  } catch { /* storage unavailable */ }
  return { ...DEFAULT_PREFS };
}
function savePrefs() {
  try { localStorage.setItem("shelfart.prefs", JSON.stringify(prefs)); } catch { /* ignore */ }
}

let catalog!: Catalog;
let prefs = loadPrefs();
let library: Library = { systems: [] };
let scanning = false;
let view: "library" | "launcher" | "artwork" | "scrape" | "settings" = "library";
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
  { id: "library", t: "Your games", s: () => (library.systems.length ? `${selectedGames()} games · ${selectedSystems().length} systems` : "Choose your ROM folders") },
  { id: "launcher", t: "Launcher & theme", s: () => `${launcher().name.split(" (")[0]} · ${themeOf(launcher()).name}` },
  { id: "artwork", t: "Artwork", s: () => `${kindsFor(launcher()).length} types · ${catalog.sizePresets.find((p) => p[0] === prefs.size)?.[1].split(" —")[0]}` },
  { id: "scrape", t: "Scrape", s: () => (run.running ? `${run.processed}/${run.total}` : run.summary ? "Finished" : "Ready when you are") },
] as const;

function renderNav() {
  const nav = document.getElementById("steps")!;
  nav.replaceChildren(
    ...STEPS.map((st, i) => {
      const cur = view === "settings" ? STEPS.length : STEPS.findIndex((x) => x.id === view);
      const done = (st.id === "library" && library.systems.length > 0 && view !== "library")
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
  const views = { library: viewLibrary, launcher: viewLauncher, artwork: viewArtwork, scrape: viewScrape, settings: viewSettings };
  main.replaceChildren(...views[view]());
}

function footer(back: typeof view | null, next: typeof view | null, nextLabel = "Continue", nextDisabled = false) {
  return h("div", { class: "footer-bar" },
    back ? h("button", { class: "btn", onclick: () => go(back) }, "← Back") : h("span"),
    next ? h("button", { class: "btn primary", disabled: nextDisabled, onclick: () => go(next) }, `${nextLabel} →`) : h("span"));
}

// ---------------------------------------------------------------- view: library

async function addRoots() {
  const picked = await open({ directory: true, multiple: true, title: "Choose your ROM folder(s)" });
  if (!picked) return;
  const list = Array.isArray(picked) ? picked : [picked];
  for (const p of list) if (!prefs.roots.includes(p)) prefs.roots.push(p);
  savePrefs();
  await rescan();
}

async function rescan() {
  scanning = true;
  render();
  try {
    library = await invoke<Library>("scan_library", { roots: prefs.roots });
  } catch (e) {
    toast(`Scan failed: ${e}`);
  }
  scanning = false;
  render();
}

let manualPath = "";
let manualSystem = "";

function viewLibrary(): Node[] {
  const out: Node[] = [
    h("h1", {}, "Your games"),
    h("p", { class: "lead" }, "Point ShelfArt at the folder that holds your games. It recognises system folders like snes, psx, gba or megadrive automatically, and identifies each game by its contents — even badly named files."),
    h("div", { class: "row" },
      h("button", { class: "btn primary", onclick: addRoots }, "+ Add ROM folder"),
      prefs.roots.length ? h("button", { class: "btn", onclick: rescan, disabled: scanning }, scanning ? "Scanning…" : "Rescan") : null),
  ];
  if (prefs.roots.length) {
    out.push(h("div", { class: "chips", style: "margin-top:14px" },
      prefs.roots.map((r) => h("span", { class: "chip", title: r }, h("span", { class: "path" }, r),
        h("button", { class: "x", title: "Remove", onclick: async () => { prefs.roots = prefs.roots.filter((x) => x !== r); savePrefs(); await rescan(); } }, "×")))));
  }

  if (!prefs.roots.length) {
    out.push(h("div", { class: "empty", style: "margin-top:24px" },
      h("div", { class: "big-icon" }, "🗂️"),
      h("div", { style: "font-weight:600;color:var(--text);margin-bottom:6px" }, "No folders yet"),
      h("div", {}, "Add the folder that contains your system folders (for example ROMs/, Emulation/roms/ or a USB drive).")));
  } else if (!library.systems.length && !scanning) {
    out.push(h("div", { class: "banner warn", style: "margin-top:18px" },
      h("div", {}, "No systems recognised in these folders. Name each system's folder after the console (snes, psx, gba…), or assign a folder manually below.")));
  }

  if (library.systems.length) {
    const allOn = selectedSystems().length === library.systems.length;
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
              h("div", { class: "meta" }, `${s.games.length} game${s.games.length === 1 ? "" : "s"} · ${s.folder_name}/`)));
        })),
    );
  }

  if (prefs.roots.length) {
    out.push(
      h("h2", {}, "Folder not recognised?"),
      h("div", { class: "card" },
        h("div", { class: "row" },
          h("button", { class: "btn", onclick: async () => { const p = await open({ directory: true, title: "Choose a system folder" }); if (p && !Array.isArray(p)) { manualPath = p; render(); } } }, manualPath ? "Change folder" : "Choose folder"),
          manualPath ? h("span", { class: "chip" }, h("span", { class: "path" }, manualPath)) : h("span", { style: "color:var(--faint)" }, "Pick a folder, then tell ShelfArt which system it is."),
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
    h("p", { class: "lead" }, "Where should the artwork go? ShelfArt saves files with the exact names and folders your launcher expects, so they show up without any renaming."),
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
  out.push(h("div", { class: "note" }, `→ ${l.where_note}`));
  out.push(footer("library", "artwork", "Continue", !folderReady()));
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
      "Videos, manuals, back covers, cartridges and fan art come from the optional ScreenScraper extras source, which this build doesn't include yet. Box art, screenshots, title screens, logos, mix images and 3D boxes all work.")));
  }
  out.push(
    h("div", { class: "kinds" },
      catalog.kinds.map((k) => {
        const supported = l.supported.includes(k.id);
        const avail = supported && kindAvailable(k.id);
        const on = chosen.includes(k.id);
        const src = k.source === "free" ? ["free", "Free"] : k.source === "generated" ? ["generated", "Made by ShelfArt"] : k.source === "extras+generated" ? ["generated", "Extras or made by ShelfArt"] : ["extras", catalog.screenscraperAvailable ? "Extras" : "Coming soon"];
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
        h("div", { class: "hint" }, "Free download from github.com/xinntao/Real-ESRGAN (releases → ncnn-vulkan). If it isn't set, ShelfArt falls back to Smart."))));
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
    h("div", { class: "desc" }, "When a game has artwork from several regions, ShelfArt picks the first one on this list that exists."),
    h("h2", {}, "Options"),
    h("div", { class: "card toggles" },
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
    if (view === "scrape") render(); else renderNav();
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
      h("p", { class: "lead" }, "Everything's set. ShelfArt only ever saves into the folders below and never deletes your files."),
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

  const shown = run.results.filter((r) => run.filter === "all" || r.status === run.filter).slice(-400).reverse();
  out.push(h("div", { class: "results" }, shown.map((r) => h("div", { class: "res", title: [r.matched ? `Matched: ${r.matched}${r.method ? ` (${r.method})` : ""}` : "No match", r.missing.length ? `Not available: ${r.missing.join(", ")}` : "", r.message ?? ""].filter(Boolean).join("\n") },
    h("div", { class: "img" }, r.thumb ? h("img", { src: r.thumb, alt: "" }) : r.status === "missing" ? "No artwork found" : "…"),
    h("div", { class: "cap" },
      h("div", { class: "g" }, h("span", { class: `dot ${r.status}` }), r.game),
      h("div", { class: "m" }, r.matched ? `${r.method === "hash" ? "✓ identified · " : ""}${r.saved.length} saved` : r.status === "missing" ? "Not in the collections" : r.message ?? ""))))));

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
  const text = `Games ShelfArt couldn't find artwork for (${new Date().toLocaleString()}):\n\n${run.summary.missingGames.join("\n")}\n`;
  await invoke<boolean>("save_missing_report", { text }).then((ok) => ok && toast("Saved")).catch((e) => toast(String(e)));
}

// ---------------------------------------------------------------- view: settings

let cacheBytes: number | null = null;

function viewSettings(): Node[] {
  if (cacheBytes === null) invoke<number>("cache_size").then((b) => { cacheBytes = b; if (view === "settings") render(); });
  const mb = cacheBytes === null ? "…" : `${(cacheBytes / 1048576).toFixed(1)} MB`;
  return [
    h("h1", {}, "Settings & privacy"),
    h("p", { class: "lead" }, "ShelfArt works without an account, collects nothing and only talks to the artwork sources listed here."),
    h("h2", {}, "Privacy"),
    h("div", { class: "card" }, h("ul", { class: "privacy", style: "margin:0;padding-left:18px" },
      h("li", {}, h("strong", {}, "No account, no telemetry. "), "There is no sign-up, no analytics and no crash reporting."),
      h("li", {}, h("strong", {}, "Only known sites. "), "ShelfArt connects over HTTPS to GitHub and libretro (free artwork and game databases) and, if enabled, screenscraper.fr — nothing else, even if a server tries to redirect it."),
      h("li", {}, h("strong", {}, "Clean files only. "), "Every image is fully decoded and re-saved as a fresh PNG, so nothing hidden in a download reaches your drive. Videos and manuals are checked to be real MP4/PDF files."),
      h("li", {}, h("strong", {}, "Your files are safe. "), "ShelfArt never deletes games, never writes outside the folders you pick, and backs up gamelist.xml before changing it."))),
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
      `ShelfArt ${catalog.version} — free and open source (MIT). Artwork from the libretro-thumbnails project`,
      catalog.screenscraperAvailable ? " and ScreenScraper" : "",
      ". Game identification uses the No-Intro and Redump databases published by libretro. All artwork belongs to its respective owners."),
  ];
}

// ---------------------------------------------------------------- boot

async function boot() {
  catalog = await invoke<Catalog>("get_catalog");
  document.getElementById("version")!.textContent = `v${catalog.version}`;
  document.getElementById("nav-settings")!.addEventListener("click", () => go("settings"));
  if (!catalog.launchers.some((l) => l.id === prefs.launcher)) prefs.launcher = "pegasus";
  await listen("scrape-event", (ev) => handleEvent(ev.payload));
  render();
  if (prefs.roots.length) await rescan();
}

boot().catch((e) => {
  document.getElementById("main")!.replaceChildren(h("div", { class: "banner warn" }, `ShelfArt failed to start: ${e}`));
});
