// Icons from Lucide (https://lucide.dev), lucide-static 1.81.0.
// Copied from the package's SVG files (only the icons this app uses), under the licences below.
/*
ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

---

The following Lucide icons are derived from the Feather project:

airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out

The MIT License (MIT) (for the icons listed above)

Copyright (c) 2013-present Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

const PATHS = {
 "downloading": "<path d=\"M12 17V3\" /> <path d=\"m6 11 6 6 6-6\" /> <path d=\"M19 21H5\" />",
 "connecting": "<path d=\"M21 12a9 9 0 1 1-6.219-8.56\" />",
 "done": "<circle cx=\"12\" cy=\"12\" r=\"10\" /> <path d=\"m16 9-5.5 5.5L8 12\" />",
 "failed": "<circle cx=\"12\" cy=\"12\" r=\"10\" /> <path d=\"m15 9-6 6\" /> <path d=\"m9 9 6 6\" />",
 "paused": "<circle cx=\"12\" cy=\"12\" r=\"10\" /> <line x1=\"10\" x2=\"10\" y1=\"15\" y2=\"9\" /> <line x1=\"14\" x2=\"14\" y1=\"15\" y2=\"9\" />",
 "queued": "<circle cx=\"12\" cy=\"12\" r=\"10\" /> <path d=\"M12 6v6l4 2\" />",
 "expired": "<path d=\"M9 17H7A5 5 0 0 1 7 7\" /> <path d=\"M15 7h2a5 5 0 0 1 4 8\" /> <line x1=\"8\" x2=\"12\" y1=\"12\" y2=\"12\" /> <line x1=\"2\" x2=\"22\" y1=\"2\" y2=\"22\" />",
 "vpn": "<path d=\"M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z\" /> <path d=\"M12 8v4\" /> <path d=\"M12 16h.01\" />",
 "seeding": "<path d=\"m18 9-6-6-6 6\" /> <path d=\"M12 3v14\" /> <path d=\"M5 21h14\" />",
 "plus": "<path d=\"M5 12h14\" /> <path d=\"M12 5v14\" />",
 "play": "<path d=\"M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z\" />",
 "pause": "<rect x=\"14\" y=\"3\" width=\"5\" height=\"18\" rx=\"1\" /> <rect x=\"5\" y=\"3\" width=\"5\" height=\"18\" rx=\"1\" />",
 "trash": "<path d=\"M10 11v6\" /> <path d=\"M14 11v6\" /> <path d=\"M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6\" /> <path d=\"M3 6h18\" /> <path d=\"M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2\" />",
 "search": "<path d=\"m21 21-4.34-4.34\" /> <circle cx=\"11\" cy=\"11\" r=\"8\" />",
 "settings": "<path d=\"M14 17H5\" /> <path d=\"M19 7h-9\" /> <circle cx=\"17\" cy=\"17\" r=\"3\" /> <circle cx=\"7\" cy=\"7\" r=\"3\" />",
 "chevron-down": "<path d=\"m6 9 6 6 6-6\" />",
 "chevron-up": "<path d=\"m18 15-6-6-6 6\" />",
 "folder": "<path d=\"m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2\" />",
 "external": "<path d=\"M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6\" /> <path d=\"m21 3-9 9\" /> <path d=\"M15 3h6v6\" />",
 "copy": "<rect width=\"14\" height=\"14\" x=\"8\" y=\"8\" rx=\"2\" ry=\"2\" /> <path d=\"M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2\" />",
 "x": "<path d=\"M18 6 6 18\" /> <path d=\"m6 6 12 12\" />",
 "columns": "<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\" /> <path d=\"M9 3v18\" /> <path d=\"M15 3v18\" />",
 "sidebar": "<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\" /> <path d=\"M9 3v18\" />",
 "details": "<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\" /> <path d=\"M3 15h18\" />",
 "schedule": "<path d=\"M16 2v3\" /> <path d=\"M17 15.5v1.7l1 .625\" /> <path d=\"M21 8.936V5a2 2 0 00-2-2H5a2 2 0 00-2 2v14a2 2 0 002 2h3.935\" /> <path d=\"M3 9h9.873\" /> <path d=\"M8 2v3\" /> <circle cx=\"17\" cy=\"17\" r=\"5\" />",
 "globe": "<circle cx=\"12\" cy=\"12\" r=\"10\" /> <path d=\"M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20\" /> <path d=\"M2 12h20\" />",
 "list": "<path d=\"M3 5h.01\" /> <path d=\"M3 12h.01\" /> <path d=\"M3 19h.01\" /> <path d=\"M8 5h13\" /> <path d=\"M8 12h13\" /> <path d=\"M8 19h13\" />",
 "minimize": "<path d=\"m14 10 7-7\" /> <path d=\"M20 10h-6V4\" /> <path d=\"m3 21 7-7\" /> <path d=\"M4 14h6v6\" />",
 "maximize": "<path d=\"M15 3h6v6\" /> <path d=\"m21 3-7 7\" /> <path d=\"m3 21 7-7\" /> <path d=\"M9 21H3v-6\" />",
 "check": "<path d=\"M20 6 9 17l-5-5\" />",
 "power": "<path d=\"M12 2v10\" /> <path d=\"M18.4 6.6a9 9 0 1 1-12.77.04\" />",
 "magnet": "<path d=\"m12 15 4 4\" /> <path d=\"M2.352 10.648a1.205 1.205 0 0 0 0 1.704l2.296 2.296a1.205 1.205 0 0 0 1.704 0l6.029-6.029a1 1 0 1 1 3 3l-6.029 6.029a1.205 1.205 0 0 0 0 1.704l2.296 2.296a1.205 1.205 0 0 0 1.704 0l6.365-6.367A1 1 0 0 0 8.716 4.282z\" /> <path d=\"m5 8 4 4\" />",
 "arrow-up": "<path d=\"m5 12 7-7 7 7\" /> <path d=\"M12 19V5\" />",
 "arrow-down": "<path d=\"M12 5v14\" /> <path d=\"m19 12-7 7-7-7\" />",
 "layers": "<path d=\"M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z\" /> <path d=\"M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12\" /> <path d=\"M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17\" />",
 "inbox": "<polyline points=\"22 12 16 12 14 15 10 15 8 12 2 12\" /> <path d=\"M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z\" />",
 "more": "<circle cx=\"12\" cy=\"12\" r=\"1\" /> <circle cx=\"19\" cy=\"12\" r=\"1\" /> <circle cx=\"5\" cy=\"12\" r=\"1\" />",
 "all": "<rect width=\"7\" height=\"7\" x=\"3\" y=\"3\" rx=\"1\" /> <rect width=\"7\" height=\"7\" x=\"3\" y=\"14\" rx=\"1\" /> <path d=\"M14 4h7\" /> <path d=\"M14 9h7\" /> <path d=\"M14 15h7\" /> <path d=\"M14 20h7\" />",
 "active": "<path d=\"M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2\" />",
 "finished": "<path d=\"M21.801 10A10 10 0 1 1 17 3.335\" /> <path d=\"m9 11 3 3L22 4\" />",
 "stopped": "<circle cx=\"12\" cy=\"12\" r=\"10\" /> <line x1=\"10\" x2=\"10\" y1=\"15\" y2=\"9\" /> <line x1=\"14\" x2=\"14\" y1=\"15\" y2=\"9\" />",
 "video": "<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\" /> <path d=\"M7 3v18\" /> <path d=\"M3 7.5h4\" /> <path d=\"M3 12h18\" /> <path d=\"M3 16.5h4\" /> <path d=\"M17 3v18\" /> <path d=\"M17 7.5h4\" /> <path d=\"M17 16.5h4\" />",
 "music": "<path d=\"M21 16V3a1 1 0 0 0-1.28-.96l-10 2.917a1 1 0 0 0-.72.96V19\" /> <circle cx=\"18\" cy=\"16\" r=\"3\" /> <circle cx=\"6\" cy=\"19\" r=\"3\" />",
 "documents": "<path d=\"M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z\" /> <path d=\"M14 2v5a1 1 0 0 0 1 1h5\" /> <path d=\"M10 9H8\" /> <path d=\"M16 13H8\" /> <path d=\"M16 17H8\" />",
 "compressed": "<path d=\"M13.659 22H18a2 2 0 0 0 2-2V8a2.4 2.4 0 0 0-.706-1.706l-3.588-3.588A2.4 2.4 0 0 0 14 2H6a2 2 0 0 0-2 2v11.5\" /> <path d=\"M14 2v5a1 1 0 0 0 1 1h5\" /> <path d=\"M8 12v-1\" /> <path d=\"M8 18v-2\" /> <path d=\"M8 7V6\" /> <circle cx=\"8\" cy=\"20\" r=\"2\" />",
 "programs": "<rect x=\"2\" y=\"4\" width=\"20\" height=\"16\" rx=\"2\" /> <path d=\"M10 4v4\" /> <path d=\"M2 8h20\" /> <path d=\"M6 4v4\" />",
 "torrents": "<path d=\"m12 15 4 4\" /> <path d=\"M2.352 10.648a1.205 1.205 0 0 0 0 1.704l2.296 2.296a1.205 1.205 0 0 0 1.704 0l6.029-6.029a1 1 0 1 1 3 3l-6.029 6.029a1.205 1.205 0 0 0 0 1.704l2.296 2.296a1.205 1.205 0 0 0 1.704 0l6.365-6.367A1 1 0 0 0 8.716 4.282z\" /> <path d=\"m5 8 4 4\" />",
 "general": "<path d=\"M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z\" /> <path d=\"M14 2v5a1 1 0 0 0 1 1h5\" />",
 "queue": "<path d=\"M11 5h10\" /> <path d=\"M11 12h10\" /> <path d=\"M11 19h10\" /> <path d=\"M4 4h1v5\" /> <path d=\"M4 9h2\" /> <path d=\"M6.5 20H3.4c0-1 2.6-1.925 2.6-3.5a1.5 1.5 0 0 0-2.6-1.02\" />",
 "folder-open": "<path d=\"m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2\" />",
 "refresh": "<path d=\"M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8\" /> <path d=\"M21 3v5h-5\" /> <path d=\"M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16\" /> <path d=\"M8 16H3v5\" />",
 "link": "<path d=\"M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71\" /> <path d=\"M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71\" />",
 "retry": "<path d=\"M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8\" /> <path d=\"M3 3v5h5\" />",
 "sidebar-close": "<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\" /> <path d=\"M9 3v18\" /> <path d=\"m16 15-3-3 3-3\" />",
 "sidebar-open": "<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\" /> <path d=\"M9 3v18\" /> <path d=\"m14 9 3 3-3 3\" />",
 "grip": "<circle cx=\"9\" cy=\"12\" r=\"1\" /> <circle cx=\"9\" cy=\"5\" r=\"1\" /> <circle cx=\"9\" cy=\"19\" r=\"1\" /> <circle cx=\"15\" cy=\"12\" r=\"1\" /> <circle cx=\"15\" cy=\"5\" r=\"1\" /> <circle cx=\"15\" cy=\"19\" r=\"1\" />",
 "offline": "<path d=\"M12 20h.01\" /> <path d=\"M8.5 16.429a5 5 0 0 1 7 0\" /> <path d=\"M5 12.859a10 10 0 0 1 5.17-2.69\" /> <path d=\"M19 12.859a10 10 0 0 0-2.007-1.523\" /> <path d=\"M2 8.82a15 15 0 0 1 4.177-2.643\" /> <path d=\"M22 8.82a15 15 0 0 0-11.288-3.764\" /> <path d=\"m2 2 20 20\" />",
 "online": "<path d=\"M12 20h.01\" /> <path d=\"M2 8.82a15 15 0 0 1 20 0\" /> <path d=\"M5 12.859a10 10 0 0 1 14 0\" /> <path d=\"M8.5 16.429a5 5 0 0 1 7 0\" />",
 "image": "<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\" ry=\"2\" /> <circle cx=\"9\" cy=\"9\" r=\"2\" /> <path d=\"m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21\" />"
};

/** An icon as an <svg> element. Decorative unless a label is given. */
export function icon(name, { size = 16, label = '', className = '' } = {}) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', String(size));
  svg.setAttribute('height', String(size));
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('class', `icon ${className}`.trim());
  if (label) { svg.setAttribute('role', 'img'); svg.setAttribute('aria-label', label); } else svg.setAttribute('aria-hidden', 'true');
  // No whitespace between the shapes, so labels' text stays clean.
  svg.innerHTML = (PATHS[name] ?? PATHS.general).replace(/>\s+</g, '><');
  return svg;
}

/** Replaces every <i data-icon="name"> in the page with its icon. */
export function hydrateIcons(root = document) {
  for (const el of root.querySelectorAll('i[data-icon]')) {
    el.replaceWith(icon(el.dataset.icon, { size: Number(el.dataset.size) || 16, className: el.className }));
  }
}

export const ICON_NAMES = Object.keys(PATHS);
