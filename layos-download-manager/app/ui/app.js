// The main window: download list, categories, details with the live section map, and
// the toolbar. Dialogs are in dialogs.js.

import { invoke, listen } from './backend.js';
import { STATUS_TEXT, action, bytes, eta, percent, speed } from './format.js';
import { initDialogs, openAdd, openRemove, openSettings } from './dialogs.js';

const $ = (id) => document.getElementById(id);

/** id -> download as the app stores it */
const downloads = new Map();
/** id -> { downloaded, size, speed, segments } while running */
const live = new Map();
const selected = new Set();
let lastClicked = null;
let filter = 'all';
let query = '';
export let info = { sectionPresets: [1, 2, 4, 8, 16, 24, 32, 48, 64], maxConnections: 64 };

// ---------- helpers

export function toast(text) {
  const t = $('toast');
  t.textContent = text;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { t.hidden = true; }, 2600);
}

export async function run(promise, failText) {
  try {
    return await promise;
  } catch (e) {
    toast(`${failText}: ${e?.message ?? e}`);
    return undefined;
  }
}

const kindOf = (d) => d.kind?.type ?? 'http';
const isTorrent = (d) => kindOf(d) === 'torrent';
const isActive = (s) => ['connecting', 'downloading', 'seeding', 'blocked'].includes(s);

function matches(d) {
  switch (filter) {
    case 'active': if (!isActive(d.status)) return false; break;
    case 'done': if (d.status !== 'completed' && d.status !== 'seeding') return false; break;
    case 'stopped': if (!['paused', 'failed', 'needsRefresh', 'queued'].includes(d.status)) return false; break;
    case 'torrents': if (!isTorrent(d)) return false; break;
    case 'all': break;
    default: if (isTorrent(d) || d.category !== filter) return false;
  }
  if (!query) return true;
  const q = query.toLowerCase();
  return d.fileName.toLowerCase().includes(q) || (d.request?.url ?? '').toLowerCase().includes(q);
}

function progressOf(d) {
  const l = live.get(d.id);
  const done = l && isActive(d.status) ? l.downloaded : d.downloaded;
  const size = l?.size ?? d.size;
  return { done, size, speed: l && isActive(d.status) ? l.speed : 0, segments: l?.segments ?? [] };
}

function kindLabel(d) {
  if (isTorrent(d)) return 'P2P';
  if (kindOf(d) === 'media') return 'VID';
  const ext = (d.fileName.split('.').pop() || '').slice(0, 4).toUpperCase();
  return ext && ext !== d.fileName.toUpperCase() ? ext : 'FILE';
}

function statusBadge(d) {
  const b = document.createElement('span');
  const cls = {
    completed: 'ok', seeding: 'ok', downloading: 'info', connecting: 'info',
    paused: 'plain', queued: 'plain', failed: 'bad', needsRefresh: 'warn', blocked: 'warn',
  }[d.status] ?? 'plain';
  b.className = `badge ${cls}`;
  b.textContent = STATUS_TEXT[d.status] ?? d.status;
  return b;
}

// ---------- list

function renderRow(d) {
  const tr = document.createElement('tr');
  tr.dataset.id = d.id;
  const p = progressOf(d);
  const pct = d.status === 'completed' ? 100 : percent(p.done, p.size);
  tr.className = `${d.status}${selected.has(d.id) ? ' selected' : ''}`;
  tr.setAttribute('aria-selected', selected.has(d.id) ? 'true' : 'false');

  const name = document.createElement('td');
  name.innerHTML = '<div class="name"><span class="kind"></span><span class="text"></span></div>';
  name.querySelector('.kind').textContent = kindLabel(d);
  name.querySelector('.text').textContent = d.fileName;
  name.title = d.fileName;

  const size = document.createElement('td');
  size.textContent = p.size ? bytes(p.size) : '';

  const prog = document.createElement('td');
  prog.innerHTML = '<div class="bar"><span></span></div>';
  prog.querySelector('span').style.width = `${pct ?? 0}%`;
  prog.title = pct != null ? `${pct}%` : bytes(p.done);

  const sp = document.createElement('td');
  sp.textContent = d.status === 'downloading' ? speed(p.speed) : '';
  const left = document.createElement('td');
  left.textContent = d.status === 'downloading' ? eta((p.size ?? 0) - p.done, p.speed) : '';
  const st = document.createElement('td');
  st.append(statusBadge(d));
  if (d.error && d.status !== 'completed') st.title = d.error;

  tr.append(name, size, prog, sp, left, st);
  return tr;
}

let renderQueued = false;
export function render() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    const list = [...downloads.values()].sort((a, b) => b.id - a.id);
    const shown = list.filter(matches);
    $('rows').replaceChildren(...shown.map(renderRow));
    $('empty').hidden = shown.length > 0;
    renderCounts(list);
    renderToolbar();
    renderDetails();
    renderSpeed();
    renderBanner(list);
  });
}

function renderCounts(list) {
  const count = (f) => {
    const old = filter;
    filter = f;
    const n = list.filter((d) => { const q = query; query = ''; const m = matches(d); query = q; return m; }).length;
    filter = old;
    return n;
  };
  for (const el of document.querySelectorAll('[data-count]')) {
    const n = count(el.dataset.count);
    el.textContent = n ? String(n) : '';
  }
}

function selectedDownloads() {
  return [...selected].map((id) => downloads.get(id)).filter(Boolean);
}

function renderToolbar() {
  const sel = selectedDownloads();
  $('btn-resume').disabled = !sel.some((d) => action(d.status) === 'continue');
  $('btn-pause').disabled = !sel.some((d) => action(d.status) === 'pause');
  $('btn-delete').disabled = sel.length === 0;
}

function renderSpeed() {
  let total = 0;
  for (const d of downloads.values()) if (d.status === 'downloading') total += progressOf(d).speed;
  $('total-speed').textContent = total ? speed(total) : '0 B/s';
}

function renderBanner(list) {
  const blocked = list.filter((d) => d.status === 'blocked');
  $('privacy-banner').hidden = blocked.length === 0;
  if (blocked.length) $('privacy-banner-text').textContent = blocked[0].error || 'Torrents are waiting for a VPN or proxy.';
}

// ---------- details

function renderMap(d, p) {
  const map = $('d-map');
  map.className = 'sectionmap';
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
    seg.firstChild.style.width = `${pct}%`;
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
    fill.style.width = `${done * 100}%`;
    el.append(fill);
    return el;
  }));
  const active = segs.filter((s) => s.active).length;
  map.setAttribute('aria-label', `${segs.length} sections, ${active} downloading`);
  map.title = `${segs.length} sections · ${active} connections downloading`;
}

function fillSectionSelect(select, value) {
  const presets = new Set(info.sectionPresets);
  if (value && !presets.has(value)) presets.add(value);
  select.replaceChildren(...[...presets].sort((a, b) => a - b).map((n) => {
    const o = document.createElement('option');
    o.value = String(n);
    o.textContent = n === 1 ? '1 (no splitting)' : String(n);
    return o;
  }));
  select.value = String(value);
}
export { fillSectionSelect };

function renderDetails() {
  const sel = selectedDownloads();
  const d = sel.length === 1 ? sel[0] : null;
  $('details').hidden = !d;
  if (!d) return;
  const p = progressOf(d);
  $('d-name').textContent = d.fileName;
  $('d-status').replaceWith(Object.assign(statusBadge(d), { id: 'd-status' }));
  const pct = d.status === 'completed' ? 100 : percent(p.done, p.size);
  $('d-done').textContent = p.size ? `${bytes(p.done)} of ${bytes(p.size)}${pct != null ? ` (${pct}%)` : ''}` : bytes(p.done);
  $('d-speed').textContent = d.status === 'downloading' ? speed(p.speed) || '—' : '—';
  $('d-eta').textContent = d.status === 'downloading' ? eta((p.size ?? 0) - p.done, p.speed) || '—' : '—';
  const http = kindOf(d) === 'http';
  $('d-sections-wrap').hidden = !http;
  if (http && document.activeElement !== $('d-sections')) fillSectionSelect($('d-sections'), d.connections);
  $('d-resume').textContent = isTorrent(d) ? 'Yes (pieces are kept)'
    : kindOf(d) === 'media' ? 'Yes'
      : d.size == null && d.status === 'queued' ? 'Checked when it starts'
        : d.resumable ? 'Yes' : 'No — it starts again from the beginning';
  $('d-path').textContent = `${d.saveDir}${d.saveDir.includes('\\') ? '\\' : '/'}${d.fileName}`;
  $('d-url').textContent = d.request?.url ?? '';
  $('d-error-wrap').hidden = !d.error || d.status === 'completed';
  $('d-error').textContent = d.error ?? '';
  $('d-open').disabled = d.status !== 'completed' && d.status !== 'seeding';
  renderMap(d, p);
}

// ---------- events from the app

function onEngine(ev) {
  switch (ev.type) {
    case 'added':
    case 'updated':
      downloads.set(ev.download.id, ev.download);
      if (!isActive(ev.download.status)) live.delete(ev.download.id);
      break;
    case 'progress':
      live.set(ev.id, { downloaded: ev.downloaded, size: ev.size, speed: ev.speed, segments: ev.segments });
      break;
    case 'removed':
      downloads.delete(ev.id);
      live.delete(ev.id);
      selected.delete(ev.id);
      break;
  }
  render();
}

// ---------- user actions

async function resumeSelected() {
  const ids = selectedDownloads().filter((d) => action(d.status) === 'continue').map((d) => d.id);
  if (ids.length) await run(invoke('resume', { ids }), 'Couldn\'t continue');
}

async function pauseSelected() {
  const ids = selectedDownloads().filter((d) => action(d.status) === 'pause').map((d) => d.id);
  if (ids.length) await run(invoke('pause', { ids }), 'Couldn\'t pause');
}

function onRowClick(e) {
  const tr = e.target.closest('tr[data-id]');
  if (!tr) return;
  const id = Number(tr.dataset.id);
  if (e.shiftKey && lastClicked != null) {
    const ids = [...$('rows').children].map((r) => Number(r.dataset.id));
    const [a, b] = [ids.indexOf(lastClicked), ids.indexOf(id)].sort((x, y) => x - y);
    if (!e.ctrlKey && !e.metaKey) selected.clear();
    for (const i of ids.slice(a, b + 1)) selected.add(i);
  } else if (e.ctrlKey || e.metaKey) {
    if (selected.has(id)) selected.delete(id); else selected.add(id);
    lastClicked = id;
  } else {
    selected.clear();
    selected.add(id);
    lastClicked = id;
  }
  render();
}

function onRowDoubleClick(e) {
  const tr = e.target.closest('tr[data-id]');
  if (!tr) return;
  const d = downloads.get(Number(tr.dataset.id));
  if (!d) return;
  if (d.status === 'completed') run(invoke('open_download', { id: d.id }), 'Couldn\'t open it');
  else if (action(d.status) === 'continue') run(invoke('resume', { ids: [d.id] }), 'Couldn\'t continue');
  else if (action(d.status) === 'pause') run(invoke('pause', { ids: [d.id] }), 'Couldn\'t pause');
}

function bind() {
  $('rows').addEventListener('click', onRowClick);
  $('rows').addEventListener('dblclick', onRowDoubleClick);
  for (const b of document.querySelectorAll('.nav-item')) {
    b.addEventListener('click', () => {
      document.querySelector('.nav-item.active')?.classList.remove('active');
      b.classList.add('active');
      filter = b.dataset.filter;
      render();
    });
  }
  $('search').addEventListener('input', (e) => { query = e.target.value.trim(); render(); });
  $('btn-add').addEventListener('click', () => openAdd());
  $('btn-resume').addEventListener('click', resumeSelected);
  $('btn-pause').addEventListener('click', pauseSelected);
  $('btn-delete').addEventListener('click', () => openRemove(selectedDownloads()));
  $('btn-pause-all').addEventListener('click', () => run(invoke('pause_all'), 'Couldn\'t pause'));
  $('btn-resume-all').addEventListener('click', () => run(invoke('resume_all'), 'Couldn\'t continue'));
  $('btn-settings').addEventListener('click', () => openSettings());
  $('privacy-banner-btn').addEventListener('click', () => openSettings('privacy'));
  $('credit').addEventListener('click', () => run(invoke('open_portfolio'), 'Couldn\'t open the link'));
  $('d-open').addEventListener('click', () => { const [d] = selectedDownloads(); if (d) run(invoke('open_download', { id: d.id }), 'Couldn\'t open it'); });
  $('d-folder').addEventListener('click', () => { const [d] = selectedDownloads(); if (d) run(invoke('show_in_folder', { id: d.id }), 'Couldn\'t show the folder'); });
  $('d-copy').addEventListener('click', async () => {
    const [d] = selectedDownloads();
    if (!d) return;
    try { await navigator.clipboard.writeText(d.request?.url ?? ''); toast('Address copied'); } catch { toast('Couldn\'t copy'); }
  });
  $('d-sections').addEventListener('change', async (e) => {
    const [d] = selectedDownloads();
    if (!d) return;
    const n = await run(invoke('set_connections', { id: d.id, connections: Number(e.target.value) }), 'Couldn\'t change sections');
    if (n != null) toast(isActive(d.status) ? `Now downloading in ${n} sections` : `Will download in ${n} sections`);
  });
  addEventListener('keydown', (e) => {
    if (e.target.closest('input, select, textarea, dialog')) return;
    const ctrl = e.ctrlKey || e.metaKey;
    if (ctrl && e.key.toLowerCase() === 'n') { e.preventDefault(); openAdd(); }
    else if (ctrl && e.key.toLowerCase() === 'a') { e.preventDefault(); for (const r of $('rows').children) selected.add(Number(r.dataset.id)); render(); }
    else if (ctrl && e.key.toLowerCase() === 'p') { e.preventDefault(); pauseSelected(); }
    else if (ctrl && e.key.toLowerCase() === 'r') { e.preventDefault(); resumeSelected(); }
    else if (e.key === 'Delete' && selected.size) openRemove(selectedDownloads());
    else if (e.key === 'Escape') { selected.clear(); render(); }
  });
  // Paste a link anywhere to add it.
  addEventListener('paste', (e) => {
    if (e.target.closest('input, textarea, dialog')) return;
    const text = e.clipboardData?.getData('text')?.trim();
    if (text && /^(https?:\/\/|magnet:\?)/i.test(text)) openAdd({ url: text });
  });
}

async function start() {
  bind();
  info = { ...info, ...(await run(invoke('app_info'), 'Couldn\'t start')) };
  initDialogs();
  for (const d of (await run(invoke('list'), 'Couldn\'t load downloads')) ?? []) downloads.set(d.id, d);
  await listen('engine', onEngine);
  render();
}

start();
