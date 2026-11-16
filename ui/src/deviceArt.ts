// Simple line drawings of each device — drawn for RomCrate from each device's
// screen size, body shape and control layout. No photos, logos or brand marks.

type Layout = "stick-top" | "dpad-top" | "deck";
interface Shape { w: number; h: number; screen: [number, number]; layout: Layout; grips?: boolean; small?: boolean }

const SHAPES: Record<string, Shape> = {
  "odin2-portal": { w: 200, h: 88, screen: [124, 70], layout: "stick-top", grips: true },
  odin2: { w: 188, h: 84, screen: [108, 61], layout: "stick-top", grips: true },
  "odin2-mini": { w: 168, h: 78, screen: [92, 52], layout: "stick-top" },
  "retroid-pocket-5": { w: 170, h: 78, screen: [98, 55], layout: "dpad-top" },
  "anbernic-rg556": { w: 182, h: 82, screen: [100, 56], layout: "dpad-top", grips: true },
  "anbernic-h700": { w: 150, h: 74, screen: [70, 53], layout: "dpad-top", small: true },
  "steam-deck": { w: 232, h: 92, screen: [112, 70], layout: "deck", grips: true },
};

const stick = (x: number, y: number, r = 9) => `<circle cx="${x}" cy="${y}" r="${r}" class="ctl"/><circle cx="${x}" cy="${y}" r="${r * 0.55}" class="ctl-in"/>`;
const dpad = (x: number, y: number, s = 7) =>
  `<path class="ctl" d="M${x - s / 2.6} ${y - s}h${s / 1.3}v${s / 1.6}h${s / 1.6}v${s / 1.3}h${-s / 1.6}v${s / 1.6}h${-s / 1.3}v${-s / 1.6}h${-s / 1.6}v${-s / 1.3}h${s / 1.6}z"/>`;
const abxy = (x: number, y: number, d = 6.5) =>
  [[0, -d], [d, 0], [0, d], [-d, 0]].map(([dx, dy]) => `<circle cx="${x + dx}" cy="${y + dy}" r="2.8" class="btn"/>`).join("");

function handheld(s: Shape): string {
  const { w, h } = s;
  const W = 240, H = 110; // drawing area
  const ox = (W - w) / 2, oy = (H - h) / 2;
  const r = h / 2.4;
  const [sw, sh] = s.screen;
  const sx = ox + (w - sw) / 2, sy = oy + (h - sh) / 2;
  const left = (sx - ox) / 2 + ox, right = W - (left);
  const top = oy + h * 0.32, bottom = oy + h * 0.68;
  const sr = s.small ? 6.5 : 8.5;
  let ctl = "";
  if (s.layout === "stick-top") ctl = stick(left, top, sr) + dpad(left, bottom) + abxy(right, top) + stick(right, bottom, sr);
  else if (s.layout === "dpad-top") ctl = dpad(left, top) + stick(left, bottom, sr) + abxy(right, top) + stick(right, bottom, sr);
  else {
    // Wide PC handheld: sticks up top, d-pad / buttons below, touch pads at the bottom.
    ctl = stick(left - 4, oy + h * 0.24, sr) + dpad(left - 8, oy + h * 0.5) + abxy(right + 8, oy + h * 0.5) + stick(right + 4, oy + h * 0.24, sr)
      + `<rect x="${left - 13}" y="${oy + h * 0.66}" width="22" height="20" rx="5" class="ctl"/><rect x="${right - 9}" y="${oy + h * 0.66}" width="22" height="20" rx="5" class="ctl"/>`;
  }
  const grips = s.grips
    ? `<path class="body" d="M${ox + 10} ${oy + h - 6} q-6 22 14 24 q16 1 22 -18z"/><path class="body" d="M${ox + w - 10} ${oy + h - 6} q6 22 -14 24 q-16 1 -22 -18z"/>`
    : "";
  return `<svg viewBox="0 0 ${W} ${H + 18}" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">${grips}`
    + `<rect x="${ox}" y="${oy}" width="${w}" height="${h}" rx="${r}" class="body"/>`
    + `<rect x="${sx}" y="${sy}" width="${sw}" height="${sh}" rx="3" class="screen"/>`
    + `<rect x="${sx + 3}" y="${sy + 3}" width="${sw - 6}" height="${sh - 6}" rx="2" class="glow"/>`
    + ctl + `</svg>`;
}

function tv(): string {
  return `<svg viewBox="0 0 240 128" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">`
    + `<rect x="40" y="6" width="160" height="92" rx="6" class="body"/><rect x="47" y="13" width="146" height="78" rx="2" class="screen"/><rect x="50" y="16" width="140" height="72" rx="2" class="glow"/>`
    + `<rect x="112" y="98" width="16" height="10" class="body"/><rect x="92" y="106" width="56" height="5" rx="2" class="body"/>`
    + `<path class="body" d="M150 110 q-6 -12 10 -12 h34 q16 0 10 12 l-4 10 q-3 6 -9 2 l-6 -5 h-16 l-6 5 q-6 4 -9 -2z"/>`
    + `<circle cx="166" cy="106" r="3.2" class="ctl-in"/><circle cx="188" cy="106" r="3.2" class="ctl-in"/></svg>`;
}

function unsure(): string {
  return `<svg viewBox="0 0 240 128" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">`
    + `<rect x="50" y="22" width="140" height="74" rx="30" class="body dashed"/><text x="120" y="70" text-anchor="middle" class="q">?</text></svg>`;
}

/** SVG markup for a device id ("unsure" for the not-sure card). */
export function deviceArt(id: string): string {
  if (id === "pc-tv") return tv();
  const s = SHAPES[id];
  return s ? handheld(s) : unsure();
}
