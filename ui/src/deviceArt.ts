// Line drawings of each device, made for RomCrate — no photos, logos or brand
// marks. Each is drawn from the device's published size (mm), screen size and
// control layout, all at the same scale, so the drawings also show how big the
// devices are next to each other.
//
// Sources for sizes and layouts: makers' spec sheets and reviews (iFixit,
// RetroCatalog, Retro Handhelds, DroiX, TechRadar), checked 17 Nov 2026.

type Ctl =
  | { k: "stick"; x: number; y: number; r?: number; ring?: boolean }
  | { k: "dpad"; x: number; y: number; s?: number }
  | { k: "abxy"; x: number; y: number; s?: number }
  | { k: "pad"; x: number; y: number; w: number; h: number }
  /** a small pill button (select / start / view / menu) */
  | { k: "pill"; x: number; y: number }
  /** a small round button (home / back / function) */
  | { k: "dot"; x: number; y: number }
  /** a speaker grille: a short row of holes */
  | { k: "grille"; x: number; y: number };

interface Spec {
  /** Body width × height in mm. */
  w: number; h: number;
  /** Screen diagonal (inches) and aspect. */
  inches: number; aspect: [number, number];
  /** Outline: rounded rectangle, "stadium" (fully round ends), or with hand grips below. */
  body: "rect" | "stadium" | "grips";
  radius?: number;
  /** How far the grips hang below the body (mm). */
  grip?: number;
  /** One sheet of glass over the whole front (Portal, Retroid Pocket 5). */
  glass?: boolean;
  /** Controls, x/y in mm from the body's top-left corner. Right-side x is measured from the right edge. */
  left: Ctl[]; right: Ctl[];
}

function screenW(inches: number, [a, b]: [number, number]) {
  return inches * 25.4 * (a / Math.hypot(a, b));
}
function screenH(inches: number, [a, b]: [number, number]) {
  return inches * 25.4 * (b / Math.hypot(a, b));
}
/** Width of the area beside the screen, and the top edge of the screen. */
function sideOf(w: number, h: number, inches: number, aspect: [number, number]) {
  return { side: (w - screenW(inches, aspect)) / 2, sy: (h - screenH(inches, aspect)) / 2 };
}

const SPECS: Record<string, Spec> = {
  // AYN Odin 2 Portal — 257 × 98.6 mm, 7" 16:9 OLED under one curved sheet of glass.
  // Left stick above the D-pad ("D-pad bottom left"), buttons above the right stick,
  // Select/Start and Home/Back at the screen's corners, front-facing speakers, rear grips.
  "odin2-portal": (() => {
    const w = 257, h = 98.6, { side, sy } = sideOf(w, h, 7, [16, 9]);
    const corner = (y: number): Ctl => ({ k: "pill", x: side - 6.5, y });
    return {
      w, h, inches: 7, aspect: [16, 9], body: "grips", grip: 9, radius: 15, glass: true,
      left: [{ k: "stick", x: 24, y: 27, r: 9, ring: true }, { k: "dpad", x: 26, y: 68 }, corner(sy + 5), { k: "dot", x: side - 6.5, y: h - sy - 5 }, { k: "grille", x: 26, y: 89 }],
      right: [{ k: "abxy", x: 24, y: 27 }, { k: "stick", x: 26, y: 68, r: 9, ring: true }, corner(sy + 5), { k: "dot", x: side - 6.5, y: h - sy - 5 }, { k: "grille", x: 26, y: 89 }],
    };
  })(),
  // AYN Odin 2 — 225 × 98 mm, 6" 16:9. D-pad under the left stick, buttons above the right
  // stick, RGB rings round the sticks, Select/Start top corners, Home/Back bottom corners,
  // stereo speakers along the lower edge, big rear grips.
  odin2: (() => {
    const w = 225, h = 98, { side, sy } = sideOf(w, h, 6, [16, 9]);
    return {
      w, h, inches: 6, aspect: [16, 9], body: "grips", grip: 10, radius: 11,
      left: [{ k: "stick", x: 23, y: 29, r: 9, ring: true }, { k: "dpad", x: 24, y: 68 }, { k: "pill", x: side - 6, y: sy + 4 }, { k: "dot", x: side - 6, y: h - sy - 4 }, { k: "grille", x: 23, y: 91 }],
      right: [{ k: "abxy", x: 23, y: 29 }, { k: "stick", x: 24, y: 68, r: 9, ring: true }, { k: "pill", x: side - 6, y: sy + 4 }, { k: "dot", x: side - 6, y: h - sy - 4 }, { k: "grille", x: 23, y: 91 }],
    };
  })(),
  // AYN Odin 2 Mini — 198 × 86 mm, 5". Vita-like rounded ends and a flat back (no grips);
  // stick above the D-pad on the left, buttons above the stick on the right.
  "odin2-mini": (() => {
    const w = 198, h = 86, { side, sy } = sideOf(w, h, 5, [16, 9]);
    return {
      w, h, inches: 5, aspect: [16, 9], body: "stadium",
      left: [{ k: "stick", x: 25, y: 27, r: 8 }, { k: "dpad", x: 26, y: 60, s: 16 }, { k: "pill", x: side - 5, y: sy + 4 }, { k: "dot", x: side - 5, y: h - sy - 4 }],
      right: [{ k: "abxy", x: 25, y: 27, s: 7 }, { k: "stick", x: 26, y: 60, r: 8 }, { k: "pill", x: side - 5, y: sy + 4 }, { k: "dot", x: side - 5, y: h - sy - 4 }],
    };
  })(),
  // Retroid Pocket 5 — 199 × 78.5 mm, 5.5" 16:9 OLED under one sheet of glass. D-pad up top
  // on the left with the stick inboard and lower; buttons up top on the right with the stick
  // inboard and lower. Speakers fire downwards, so none on the front.
  "retroid-pocket-5": (() => {
    const w = 199, h = 78.5;
    return {
      w, h, inches: 5.5, aspect: [16, 9], body: "rect", radius: 20, glass: true,
      left: [{ k: "dpad", x: 16, y: 24, s: 16 }, { k: "stick", x: 27, y: 56, r: 7 }, { k: "pill", x: 12, y: 70 }],
      right: [{ k: "abxy", x: 16, y: 24, s: 6.8 }, { k: "stick", x: 27, y: 56, r: 7 }, { k: "pill", x: 12, y: 70 }],
    };
  })(),
  // Anbernic RG556 — 223 × 90 mm, 5.48" 16:9 OLED. Small Switch-style sticks: left stick above
  // the D-pad, buttons above the right stick. Function/Return under the D-pad, Select/Start
  // under the right stick. Large hand grips with the speakers at their base.
  "anbernic-rg556": (() => {
    const w = 223, h = 90;
    return {
      w, h, inches: 5.48, aspect: [16, 9], body: "grips", grip: 14, radius: 12,
      left: [{ k: "stick", x: 25, y: 25, r: 7.5 }, { k: "dpad", x: 27, y: 58, s: 17 }, { k: "dot", x: 20, y: 80 }, { k: "dot", x: 33, y: 80 }, { k: "grille", x: 22, y: 98 }],
      right: [{ k: "abxy", x: 25, y: 25 }, { k: "stick", x: 27, y: 58, r: 7.5 }, { k: "pill", x: 20, y: 80 }, { k: "pill", x: 33, y: 80 }, { k: "grille", x: 22, y: 98 }],
    };
  })(),
  // Anbernic RG35XX H — 145 × 69 mm, 3.5" 4:3. D-pad top left, buttons top right, small sticks
  // set into the bottom corners, Select left of the screen and Start right of it.
  "anbernic-h700": (() => {
    const w = 145, h = 69, { side } = sideOf(w, h, 3.5, [4, 3]);
    return {
      w, h, inches: 3.5, aspect: [4, 3], body: "rect", radius: 7,
      left: [{ k: "dpad", x: 17, y: 22, s: 15 }, { k: "stick", x: 18, y: 52, r: 6 }, { k: "pill", x: side - 6, y: 40 }],
      right: [{ k: "abxy", x: 17, y: 22, s: 6.2 }, { k: "stick", x: 18, y: 52, r: 6 }, { k: "pill", x: side - 6, y: 40 }],
    };
  })(),
  // Steam Deck — 298 × 117 mm, 7" 16:10. D-pad and buttons on the outside, sticks inboard at
  // the top, square trackpads below them, View/Menu above, Steam and Quick Access below.
  "steam-deck": (() => {
    const w = 298, h = 117;
    return {
      w, h, inches: 7, aspect: [16, 10], body: "grips", grip: 8, radius: 18,
      left: [{ k: "dpad", x: 21, y: 36, s: 20 }, { k: "pill", x: 35, y: 13 }, { k: "stick", x: 53, y: 30, r: 10.5 },
        { k: "pad", x: 51, y: 81, w: 33, h: 33 }, { k: "dot", x: 66, y: 107 }],
      right: [{ k: "abxy", x: 21, y: 36, s: 8 }, { k: "pill", x: 35, y: 13 }, { k: "stick", x: 53, y: 30, r: 10.5 },
        { k: "pad", x: 51, y: 81, w: 33, h: 33 }, { k: "dot", x: 66, y: 107 }],
    };
  })(),
};

/** The drawing frame (mm) every device is placed in — wide enough for the Steam Deck. */
const FRAME_W = 310, FRAME_H = 146;

function outline(s: Spec): string {
  const { w, h } = s;
  const r = Math.min(s.radius ?? 10, h / 2);
  if (s.body === "stadium") return `<rect x="0" y="0" width="${w}" height="${h}" rx="${h / 2}"/>`;
  if (s.body === "rect" || !s.grip) return `<rect x="0" y="0" width="${w}" height="${h}" rx="${r}"/>`;
  // Body with hand grips hanging below the outer thirds.
  const g = s.grip, gw = w * 0.22;
  return `<path d="M${r} 0 H${w - r} Q${w} 0 ${w} ${r} V${h * 0.6}
    C${w} ${h + g * 0.7} ${w - gw * 0.25} ${h + g} ${w - gw * 0.55} ${h + g}
    C${w - gw * 0.85} ${h + g} ${w - gw} ${h + g * 0.35} ${w - gw - 5} ${h}
    H${gw + 5}
    C${gw} ${h + g * 0.35} ${gw * 0.85} ${h + g} ${gw * 0.55} ${h + g}
    C${gw * 0.25} ${h + g} 0 ${h + g * 0.7} 0 ${h * 0.6}
    V${r} Q0 0 ${r} 0 Z"/>`;
}

function control(c: Ctl, mirrorW: number | null): string {
  const X = (x: number) => (mirrorW === null ? x : mirrorW - x);
  switch (c.k) {
    case "stick": {
      const r = c.r ?? 8;
      const ring = c.ring ? `<circle class="ring" cx="${X(c.x)}" cy="${c.y}" r="${r + 1.8}"/>` : "";
      return `${ring}<circle class="ctl" cx="${X(c.x)}" cy="${c.y}" r="${r}"/><circle class="ctl-cap" cx="${X(c.x)}" cy="${c.y}" r="${r * 0.62}"/>`;
    }
    case "pill": return `<rect class="menu-btn" x="${X(c.x) - 2.4}" y="${c.y - 1.1}" width="4.8" height="2.2" rx="1.1"/>`;
    case "dot": return `<circle class="menu-btn" cx="${X(c.x)}" cy="${c.y}" r="1.7"/>`;
    case "grille": return [-4, -2, 0, 2, 4].map((d) => `<circle class="grille" cx="${X(c.x) + d}" cy="${c.y}" r="0.75"/>`).join("");
    case "dpad": {
      const s = c.s ?? 18, a = s / 2, t = s / 6;
      const x = X(c.x), y = c.y;
      return `<path class="ctl" d="M${x - t} ${y - a} h${2 * t} v${a - t} h${a - t} v${2 * t} h${-(a - t)} v${a - t} h${-2 * t} v${-(a - t)} h${-(a - t)} v${-2 * t} h${a - t} Z"/>`;
    }
    case "abxy": {
      const s = c.s ?? 7.5, r = s * 0.42;
      const x = X(c.x), y = c.y;
      return [[0, -s], [s, 0], [0, s], [-s, 0]].map(([dx, dy]) => `<circle class="face" cx="${x + dx}" cy="${y + dy}" r="${r}"/>`).join("");
    }
    case "pad": {
      const x = X(c.x) - c.w / 2, y = c.y - c.h / 2;
      return `<rect class="ctl" x="${x}" y="${y}" width="${c.w}" height="${c.h}" rx="5"/>`;
    }
  }
}

function handheld(s: Spec, id: string): string {
  const sw = screenW(s.inches, s.aspect), sh = screenH(s.inches, s.aspect);
  const sx = (s.w - sw) / 2, sy = (s.h - sh) / 2;
  const ox = (FRAME_W - s.w) / 2, oy = (FRAME_H - s.h - (s.grip ?? 0)) / 2 + 2;
  // Shoulder buttons sit on the flat part of the top edge, clear of rounded corners.
  const corner = s.body === "stadium" ? s.h / 2 : (s.radius ?? 10);
  const sx0 = Math.max(s.w * 0.06, corner * 0.8), sw0 = s.w * 0.17;
  const shoulders = `<rect class="shoulder" x="${sx0}" y="-2.6" width="${sw0}" height="4" rx="2"/><rect class="shoulder" x="${s.w - sx0 - sw0}" y="-2.6" width="${sw0}" height="4" rx="2"/>`;
  const g = `rc-${id}-${Math.random().toString(36).slice(2, 7)}`;
  const bottom = s.h + (s.grip ?? 0);
  return `<svg viewBox="0 0 ${FRAME_W} ${FRAME_H}" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
  <defs>
    <linearGradient id="${g}" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" style="stop-color:var(--art-top)"/><stop offset="1" style="stop-color:var(--art-bottom)"/>
    </linearGradient>
  </defs>
  <g transform="translate(${ox} ${oy})">
    <ellipse class="floor" cx="${s.w / 2}" cy="${bottom + 3}" rx="${s.w * 0.42}" ry="3"/>
    ${shoulders}
    <g class="body" style="fill:url(#${g})">${outline(s)}</g>
    ${s.glass ? `<rect class="front-glass" x="2.2" y="2.2" width="${s.w - 4.4}" height="${s.h - 4.4}" rx="${Math.max(2, (s.radius ?? 10) - 2.2)}"/>` : ""}
    <rect class="glass" x="${sx - 2.5}" y="${sy - 2.5}" width="${sw + 5}" height="${sh + 5}" rx="3.5"/>
    <rect class="screen" x="${sx}" y="${sy}" width="${sw}" height="${sh}" rx="2"/>
    <rect class="glow" x="${sx + 2}" y="${sy + 2}" width="${sw - 4}" height="${sh - 4}" rx="1.5"/>
    ${s.left.map((c) => control(c, null)).join("")}
    ${s.right.map((c) => control(c, s.w)).join("")}
  </g></svg>`;
}

/** A TV with a game controller in front of it. */
function tv(): string {
  return `<svg viewBox="0 0 ${FRAME_W} ${FRAME_H}" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
  <g transform="translate(70 6)">
    <g class="body"><rect x="0" y="0" width="170" height="98" rx="4"/></g>
    <rect class="screen" x="5" y="5" width="160" height="88" rx="1.5"/><rect class="glow" x="7" y="7" width="156" height="84" rx="1"/>
    <g class="body"><path d="M78 98 h14 v10 h-14 z"/><rect x="58" y="107" width="54" height="4" rx="2"/></g>
  </g>
  <g transform="translate(176 96)">
    <g class="body"><path d="M10 6 Q20 0 40 0 Q60 0 70 6 Q80 14 78 30 Q76 40 68 38 Q60 30 52 28 H28 Q20 30 12 38 Q4 40 2 30 Q0 14 10 6 Z"/></g>
    <path class="ctl" d="M15 11 h4 v4 h4 v4 h-4 v4 h-4 v-4 h-4 v-4 h4 z"/>
    ${[[60, 11], [64, 15], [60, 19], [56, 15]].map(([x, y]) => `<circle class="face" cx="${x}" cy="${y}" r="1.7"/>`).join("")}
    <circle class="ctl" cx="30" cy="22" r="4.5"/><circle class="ctl" cx="50" cy="22" r="4.5"/>
  </g></svg>`;
}

function unsure(): string {
  return `<svg viewBox="0 0 ${FRAME_W} ${FRAME_H}" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
  <g transform="translate(60 22)"><g class="body dashed"><rect x="0" y="0" width="190" height="88" rx="22"/></g>
  <text class="q" x="95" y="58" text-anchor="middle">?</text></g></svg>`;
}

/** SVG markup for a device id ("unsure" for the not-sure card). */
export function deviceArt(id: string): string {
  if (id === "pc-tv") return tv();
  const s = SPECS[id];
  return s ? handheld(s, id) : unsure();
}

/** Body size in mm, for showing under the device name. */
export function deviceSize(id: string): string | null {
  const s = SPECS[id];
  return s ? `${Math.round(s.w)} × ${Math.round(s.h)} mm` : null;
}
