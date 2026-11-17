// Simple line icons drawn for RomCrate (24×24, stroke = currentColor).

const P: Record<string, string> = {
  home: `<path d="M4 11 12 4l8 7"/><path d="M6 10v9h12v-9"/><path d="M10 19v-5h4v5"/>`,
  check: `<circle cx="11" cy="11" r="6.5"/><path d="m16 16 4 4"/><path d="m8.3 11 1.9 1.9 3.6-3.8"/>`,
  import: `<path d="M12 4v10"/><path d="m8 10 4 4 4-4"/><path d="M4 14v4.5A1.5 1.5 0 0 0 5.5 20h13a1.5 1.5 0 0 0 1.5-1.5V14"/>`,
  art: `<rect x="4" y="4" width="16" height="16" rx="2"/><circle cx="9" cy="9.5" r="1.6"/><path d="m4.5 17 4.5-4.5 3.5 3.5 2.5-2.5 4.5 4.5"/>`,
  pegasus: `<rect x="3.5" y="5" width="17" height="12" rx="2"/><path d="m10.5 8.5 4 2.5-4 2.5z"/><path d="M8 20h8"/>`,
  send: `<rect x="7" y="2.5" width="10" height="7" rx="1.5"/><path d="M10 5.5h.01M14 5.5h.01"/><path d="M12 9.5v6"/><rect x="8" y="15.5" width="8" height="6" rx="2"/>`,
  tune: `<path d="M5 4v6M5 14v6M12 4v3M12 11v9M19 4v9M19 17v3"/><circle cx="5" cy="12" r="2"/><circle cx="12" cy="9" r="2"/><circle cx="19" cy="15" r="2"/>`,
  settings: `<circle cx="12" cy="12" r="3"/><path d="M12 3v2.5M12 18.5V21M3 12h2.5M18.5 12H21M5.6 5.6l1.8 1.8M16.6 16.6l1.8 1.8M5.6 18.4l1.8-1.8M16.6 7.4l1.8-1.8"/>`,
};

export function icon(name: string): string {
  return `<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${P[name] ?? ""}</svg>`;
}
