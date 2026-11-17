// Line drawings of each device, made for RomCrate — no photos, logos or brand
// marks. Each is drawn from the device's published size (mm), screen size and
// control layout, all at the same scale, so the drawings also show how big the
// devices are next to each other.
//
// Sources for sizes and layouts: makers' spec sheets and reviews (iFixit,
// RetroCatalog, Retro Handhelds, DroiX, TechRadar), checked 17 Nov 2026.

type Ctl =
  | { k: "stick"; x: number; y: number; r?: number }
  | { k: "dpad"; x: number; y: number; s?: number }
  | { k: "abxy"; x: number; y: number; s?: number }
  | { k: "pad"; x: number; y: number; w: number; h: number };

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
  /** Controls, x/y in mm from the body's top-left corner (left side; mirrored for the right). */
  left: Ctl[]; right: Ctl[];
  /** Small extra marks: front speakers. */
  speakers?: "grips" | "sides";
}

const ODIN_LIKE = (w: number, h: number, inches: number, grip: number, r = 10): Spec => {
  const side = (w - screenW(inches, [16, 9])) / 2;
  return {
    w, h, inches, aspect: [16, 9], body: "grips", grip, radius: r,
    // Stick above, D-pad below on the left; buttons above, stick below on the right.
    left: [{ k: "stick", x: side * 0.5, y: h * 0.3, r: 8.5 }, { k: "dpad", x: side * 0.56, y: h * 0.7 }],
    right: [{ k: "abxy", x: side * 0.5, y: h * 0.3 }, { k: "stick", x: side * 0.56, y: h * 0.7, r: 8.5 }],
  };
};

function screenW(inches: number, [a, b]: [number, number]) {
  return inches * 25.4 * (a / Math.hypot(a, b));
}
function screenH(inches: number, [a, b]: [number, number]) {
  return inches * 25.4 * (b / Math.hypot(a, b));
}

const SPECS: Record<string, Spec> = {
  // 257 × 98.6 mm, 7" 16:9; D-pad bottom left, asymmetric sticks, grips, front speakers.
  "odin2-portal": { ...ODIN_LIKE(257, 98.6, 7.0, 9, 14), speakers: "sides" },
  // 225 × 98 mm, 6" 16:9; D-pad under the left stick, buttons above the right stick.
  odin2: ODIN_LIKE(225, 98, 6.0, 8),
  // 198 × 86 mm, 5"; Vita-like rounded ends, stick above the D-pad.
  "odin2-mini": (() => {
    const s = ODIN_LIKE(198, 86, 5.0, 0);
    return { ...s, body: "stadium" as const, grip: 0, left: s.left.map((c) => ({ ...c, x: (c as { x: number }).x + 3 })) as Ctl[], right: s.right.map((c) => ({ ...c, x: (c as { x: number }).x + 3 })) as Ctl[] };
  })(),
  // 199 × 78 mm, 5.5" 16:9; D-pad upper left, left stick to the right of and below it.
  "retroid-pocket-5": (() => {
    const w = 199, h = 78, side = (w - screenW(5.5, [16, 9])) / 2;
    return {
      w, h, inches: 5.5, aspect: [16, 9], body: "rect", radius: 22,
      left: [{ k: "dpad", x: side * 0.42, y: h * 0.33 }, { k: "stick", x: side * 0.72, y: h * 0.74, r: 7.5 }],
      right: [{ k: "abxy", x: side * 0.42, y: h * 0.33 }, { k: "stick", x: side * 0.72, y: h * 0.74, r: 7.5 }],
    } satisfies Spec;
  })(),
  // 223 × 90 mm, 5.48" 16:9; offset sticks with the D-pad at the bottom, grips, speakers at the grips.
  "anbernic-rg556": { ...ODIN_LIKE(223, 90, 5.48, 11, 12), speakers: "grips" },
  // RG35XX H: 145 × 69 mm, 3.5" 4:3; D-pad and buttons on top, two small sticks below.
  "anbernic-h700": (() => {
    const w = 145, h = 69, side = (w - screenW(3.5, [4, 3])) / 2;
    return {
      w, h, inches: 3.5, aspect: [4, 3], body: "rect", radius: 7,
      left: [{ k: "dpad", x: side * 0.45, y: h * 0.3, s: 15 }, { k: "stick", x: side * 0.6, y: h * 0.76, r: 5.5 }],
      right: [{ k: "abxy", x: side * 0.45, y: h * 0.3, s: 6.5 }, { k: "stick", x: side * 0.6, y: h * 0.76, r: 5.5 }],
    } satisfies Spec;
  })(),
  // 298 × 117 mm, 7" 16:10; D-pad and buttons on the outside, sticks inside at the top, trackpads below.
  "steam-deck": (() => {
    const w = 298, h = 117, side = (w - screenW(7.0, [16, 10])) / 2;
    return {
      w, h, inches: 7.0, aspect: [16, 10], body: "grips", grip: 12, radius: 16,
      left: [{ k: "dpad", x: side * 0.3, y: h * 0.36 }, { k: "stick", x: side * 0.66, y: h * 0.25, r: 10 },
        { k: "pad", x: side * 0.64, y: h * 0.68, w: 33, h: 33 }],
      right: [{ k: "abxy", x: side * 0.3, y: h * 0.36 }, { k: "stick", x: side * 0.66, y: h * 0.25, r: 10 },
        { k: "pad", x: side * 0.64, y: h * 0.68, w: 33, h: 33 }],
    } satisfies Spec;
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
      return `<circle class="ctl" cx="${X(c.x)}" cy="${c.y}" r="${r}"/><circle class="ctl-cap" cx="${X(c.x)}" cy="${c.y}" r="${r * 0.62}"/>`;
    }
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
  const shoulders = `<rect class="shoulder" x="${s.w * 0.06}" y="-2.6" width="${s.w * 0.17}" height="4" rx="2"/><rect class="shoulder" x="${s.w * 0.77}" y="-2.6" width="${s.w * 0.17}" height="4" rx="2"/>`;
  const dots = (cx: number, cy: number) => [-3, 0, 3].map((d) => `<circle class="grille" cx="${cx + d}" cy="${cy}" r="0.9"/>`).join("");
  let speakers = "";
  if (s.speakers === "grips") speakers = dots(s.w * 0.12, s.h + (s.grip ?? 0) * 0.55) + dots(s.w * 0.88, s.h + (s.grip ?? 0) * 0.55);
  if (s.speakers === "sides") speakers = dots(sx * 0.5, s.h * 0.92) + dots(s.w - sx * 0.5, s.h * 0.92);
  // Start / select beside the screen's top corners.
  const menu = `<rect class="menu-btn" x="${sx - 7}" y="${sy + 2}" width="4" height="2" rx="1"/><rect class="menu-btn" x="${s.w - sx + 3}" y="${sy + 2}" width="4" height="2" rx="1"/>`;
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
    <rect class="glass" x="${sx - 2.5}" y="${sy - 2.5}" width="${sw + 5}" height="${sh + 5}" rx="3.5"/>
    <rect class="screen" x="${sx}" y="${sy}" width="${sw}" height="${sh}" rx="2"/>
    <rect class="glow" x="${sx + 2}" y="${sy + 2}" width="${sw - 4}" height="${sh - 4}" rx="1.5"/>
    ${menu}${speakers}
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
