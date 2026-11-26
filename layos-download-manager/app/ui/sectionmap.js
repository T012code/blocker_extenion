// The section map: one block per section, filled as it downloads, with a marker where
// each active connection is writing. Shared by the main window and download windows.

import { percent } from './format.js';

export function renderMap(map, d, p, caption) {
  map.className = 'sectionmap';
  map.dataset.status = d.status;
  if (d.status === 'completed') {
    map.classList.add('done');
    map.replaceChildren();
    map.setAttribute('aria-label', 'Complete');
    return;
  }
  const size = p.size;
  let segs = p.segments.length ? p.segments : (d.segments ?? []).map((s) => ({ ...s, active: false }));
  if (!size || !segs.length || segs[0].end > 1e18) {
    // No section map (torrent, video, or unknown size): one bar.
    map.classList.add('whole');
    const pct = percent(p.done, size) ?? 0;
    const seg = document.createElement('div');
    seg.className = 'seg';
    seg.style.cssText = 'left:0;width:100%';
    seg.innerHTML = '<div class="fill"></div>';
    seg.firstChild.style.setProperty('--p', String(pct / 100));
    map.replaceChildren(seg);
    map.setAttribute('aria-label', `${pct}% downloaded`);
    return;
  }
  segs = [...segs].sort((a, b) => a.start - b.start);
  map.replaceChildren(...segs.map((s) => {
    const el = document.createElement('div');
    el.className = `seg${s.active ? ' active' : ''}`;
    el.style.left = `${(s.start / size) * 100}%`;
    el.style.width = `${((s.end - s.start) / size) * 100}%`;
    const done = s.end > s.start ? Math.min(1, (s.pos - s.start) / (s.end - s.start)) : 1;
    el.style.setProperty('--cursor', `calc(${done * 100}% - 3px)`);
    const fill = document.createElement('div');
    fill.className = 'fill';
    fill.style.setProperty('--p', String(done));
    el.append(fill);
    return el;
  }));
  const active = segs.filter((s) => s.active).length;
  map.setAttribute('aria-label', `${segs.length} sections, ${active} downloading`);
  if (caption) caption.textContent = `${segs.length} sections · ${active} downloading now`;
}

