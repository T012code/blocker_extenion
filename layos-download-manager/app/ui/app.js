// The main window: download list, categories, details with the live section map, and
// the toolbar. Dialogs are in dialogs.js.

import { invoke, listen } from './backend.js';
import { action, bytes, eta, percent, speed } from './format.js';
import { initDialogs, openAdd, openRefresh, openDeleteQueue, openGrab, openQueue, openRemove, openSchedule, openSettings } from './dialogs.js';
import { hydrateIcons, icon } from './icons.js';
import { initLayout } from './layout.js';
import { initTooltips } from './tooltip.js';
import { renderMap } from './sectionmap.js';
import { COLUMNS, DownloadTable, statusEl } from './table.js';

const $ = (id) => document.getElementById(id);

/** id -> download as the app stores it */
const downloads = new Map();
/** id -> { downloaded, size, speed, segments } while running */
const live = new Map();
const selected = new Set();
let lastClicked = null;
let filter = 'all';
/** Queues: { id, name, maxActive, running } */
export let queues = [];
let query = '';
export let info = { sectionPresets: [1, 2, 4, 8, 16, 24, 32, 48, 64], maxConnections: 64 };
/** The list's columns, sorting and rows. Created in start(). */
let table;

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
/** A file download whose link stopped working can get a fresh one. */
const canRefresh = (d) => kindOf(d) === 'http' && (d.status === 'needsRefresh' || d.status === 'failed');
const isActive = (s) => ['connecting', 'downloading', 'seeding', 'blocked'].includes(s);

function matches(d) {
  switch (filter) {
    case 'active': if (!isActive(d.status)) return false; break;
    case 'done': if (d.status !== 'completed' && d.status !== 'seeding') return false; break;
    case 'stopped': if (!['paused', 'failed', 'needsRefresh', 'queued'].includes(d.status)) return false; break;
    case 'torrents': if (!isTorrent(d)) return false; break;
    case 'all': break;
    default:
      if (filter.startsWith('queue:')) { if (d.queueId !== Number(filter.slice(6))) return false; break; }
      if (isTorrent(d) || d.category !== filter) return false;
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

const queueName = (id) => (id == null ? '' : queues.find((q) => q.id === id)?.name ?? '');
const tableCtx = () => ({ progressOf, queueName, selected, cursor: lastClicked });

let renderQueued = false;
export function render() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    const list = [...downloads.values()].sort((a, b) => b.id - a.id);
    const ctx = tableCtx();
    const shown = table.sort(list.filter(matches), ctx);
    table.update(shown, ctx);
    $('empty').hidden = shown.length > 0;
    if (!shown.length) {
      const q = currentQueue();
      $('empty-title').textContent = query ? 'No matches' : q ? `"${q.name}" is empty` : list.length ? 'Nothing in this view' : 'Nothing here yet';
      $('empty-text').textContent = query ? `Nothing matches "${query}".`
        : q ? 'Add downloads to this queue, or move them here from the details of any download.'
          : 'Add a download, paste a link anywhere in this window, or download something in your browser.';
      $('empty-add').hidden = !!query;
    }
    renderQueues(list);
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

export function currentQueue() {
  return filter.startsWith('queue:') ? queues.find((q) => q.id === Number(filter.slice(6))) ?? null : null;
}

function renderQueues(list) {
  const nav = $('queue-nav');
  if (filter.startsWith('queue:') && !currentQueue()) filter = 'all';
  nav.replaceChildren(...queues.map((q) => {
    const b = document.createElement('button');
    b.className = `nav-item${filter === `queue:${q.id}` ? ' active' : ''}`;
    b.dataset.filter = `queue:${q.id}`;
    const n = document.createElement('span');
    n.className = 'label';
    const ic = document.createElement('span');
    ic.className = 'queue-icon';
    ic.append(icon('queue'));
    if (q.running) ic.append(Object.assign(document.createElement('span'), { className: 'running-mark' }));
    n.append(ic, Object.assign(document.createElement('span'), { className: 'text', textContent: q.name }));
    b.title = q.running ? `${q.name}, running` : q.name;
    const c = document.createElement('span');
    c.className = 'count';
    const count = list.filter((d) => d.queueId === q.id).length;
    c.textContent = count ? String(count) : '';
    b.append(n, c);
    return b;
  }));
  for (const b of document.querySelectorAll('#nav > .nav-item')) b.classList.toggle('active', b.dataset.filter === filter);
  const q = currentQueue();
  $('queuebar').hidden = !q;
  if (!q) return;
  $('qb-name').textContent = q.name;
  const inQueue = list.filter((d) => d.queueId === q.id);
  const running = inQueue.filter((d) => isActive(d.status)).length;
  const waiting = inQueue.filter((d) => d.status === 'queued').length;
  const sched = q.schedule?.start ? ` · starts at ${q.schedule.start}` : '';
  $('qb-state').textContent = `${q.running ? 'Running' : 'Stopped'} · ${running} downloading · ${waiting} waiting${sched}`;
  $('qb-start').disabled = q.running;
  $('qb-stop').disabled = !q.running;
  if (document.activeElement !== $('qb-max')) {
    $('qb-max').replaceChildren(...[1, 2, 3, 4, 5, 6, 8, 10].map((n) => Object.assign(document.createElement('option'), { value: String(n), textContent: String(n) })));
    if (![...$('qb-max').options].some((o) => Number(o.value) === q.maxActive)) $('qb-max').append(Object.assign(document.createElement('option'), { value: String(q.maxActive), textContent: String(q.maxActive) }));
    $('qb-max').value = String(q.maxActive);
  }
}

/** The downloads shown right now (for "remove everything in this view"). */
export function viewDownloads() {
  return [...downloads.values()].filter(matches);
}

export function allDownloads() {
  return [...downloads.values()];
}

function selectedDownloads() {
  return [...selected].map((id) => downloads.get(id)).filter(Boolean);
}

function renderToolbar() {
  const sel = selectedDownloads();
  $('btn-resume').disabled = !sel.some((d) => action(d.status) === 'continue');
  $('btn-pause').disabled = !sel.some((d) => action(d.status) === 'pause');
  // Remove is always available: it can also clear finished or failed downloads.
  $('btn-delete').disabled = downloads.size === 0;
}

function renderSpeed() {
  let total = 0;
  for (const d of downloads.values()) if (d.status === 'downloading') total += progressOf(d).speed;
  $('total-speed').textContent = total ? speed(total) : '0 B/s';
}

function renderBanner(list) {
  const blocked = list.filter((d) => d.status === 'blocked');
  $('privacy-banner').hidden = blocked.length === 0;
  if (blocked.length) {
    const n = blocked.length;
    $('privacy-banner-text').textContent = n === 1 ? 'A torrent is waiting for a VPN or proxy.' : `${n} torrents are waiting for a VPN or proxy.`;
  }
}

// ---------- details

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

let detailsSig = '';
function renderDetails() {
  const sel = selectedDownloads();
  const d = sel.length === 1 ? sel[0] : null;
  $('details').classList.toggle('empty-sel', !d);
  $('details-none').textContent = sel.length > 1 ? `${sel.length} downloads selected.` : 'Select a download to see its sections, speed and where it\'s saved.';
  if (!d) { detailsSig = ''; return; }
  const p = progressOf(d);
  $('d-name').textContent = d.fileName;
  $('d-name').title = d.fileName;
  const sig = `${d.id}:${d.status}`;
  const st = statusEl(d);
  st.id = 'd-status';
  if (detailsSig && detailsSig.split(':')[0] === String(d.id) && detailsSig !== sig) st.classList.add('changed');
  if (detailsSig !== sig) $('d-status').replaceWith(st);
  detailsSig = sig;
  $('d-map-caption').textContent = '';
  const pct = d.status === 'completed' ? 100 : percent(p.done, p.size);
  $('d-done').textContent = p.size ? `${bytes(p.done)} of ${bytes(p.size)}${pct != null ? ` (${pct}%)` : ''}` : bytes(p.done);
  $('d-speed').textContent = d.status === 'downloading' ? speed(p.speed) || '—' : '—';
  $('d-eta').textContent = d.status === 'downloading' ? eta((p.size ?? 0) - p.done, p.speed) || '—' : '—';
  const http = kindOf(d) === 'http';
  $('d-sections-wrap').hidden = !http;
  if (http && document.activeElement !== $('d-sections')) fillSectionSelect($('d-sections'), d.connections);
  $('d-resume').textContent = isTorrent(d) ? 'Yes, pieces are kept'
    : kindOf(d) === 'media' ? 'Yes'
      : d.size == null && d.status === 'queued' ? 'Checked when it starts'
        : d.resumable ? 'Yes' : 'No, it starts over';
  $('d-path').textContent = `${d.saveDir}${d.saveDir.includes('\\') ? '\\' : '/'}${d.fileName}`;
  $('d-url').textContent = d.request?.url ?? '';
  $('d-error-wrap').hidden = !d.error || d.status === 'completed';
  $('d-error').textContent = d.error ?? '';
  $('d-refresh').hidden = !canRefresh(d);
  if (document.activeElement !== $('d-queue')) {
    $('d-queue').replaceChildren(
      Object.assign(document.createElement('option'), { value: '', textContent: 'No queue' }),
      ...queues.map((q) => Object.assign(document.createElement('option'), { value: String(q.id), textContent: q.name })),
    );
    $('d-queue').value = d.queueId != null ? String(d.queueId) : '';
  }
  renderMap($('d-map'), d, p, $('d-map-caption'));
}

// ---------- events from the app

function onEngine(ev) {
  switch (ev.type) {
    case 'added':
    case 'updated':
      // Started in the background for an "add download" window: not in the list until
      // it's confirmed.
      if (ev.download.pending) { downloads.delete(ev.download.id); break; }
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
  // One expired download: continuing it means getting a fresh link.
  const sel = selectedDownloads();
  if (sel.length === 1 && sel[0].status === 'needsRefresh' && canRefresh(sel[0])) return openRefresh(sel[0]);
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
  else if (d.status === 'needsRefresh' && canRefresh(d)) openRefresh(d);
  else if (action(d.status) === 'continue') run(invoke('resume', { ids: [d.id] }), 'Couldn\'t continue');
  else if (action(d.status) === 'pause') run(invoke('pause', { ids: [d.id] }), 'Couldn\'t pause');
}

// ---------- menus

function closeMenus() {
  for (const m of document.querySelectorAll('.menu')) m.hidden = true;
  $('btn-more').setAttribute('aria-expanded', 'false');
  $('btn-columns').setAttribute('aria-expanded', 'false');
}

/** Opens a menu at a point, kept inside the window, with focus on its first item. */
function openMenu(menu, x, y) {
  closeMenus();
  menu.hidden = false;
  // Its real size (the opening animation scales it a little).
  const w = menu.offsetWidth;
  const h = menu.offsetHeight;
  menu.style.left = `${Math.max(8, Math.min(x, innerWidth - w - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(y, innerHeight - h - 8))}px`;
  menu.querySelector('button:not(:disabled)')?.focus();
}

function menuItem(label, onClick, { disabled = false, danger = false, kbd = '', ico = '' } = {}) {
  const b = document.createElement('button');
  b.setAttribute('role', 'menuitem');
  if (ico) b.append(icon(ico));
  b.append(label);
  b.disabled = disabled;
  if (danger) b.className = 'danger';
  if (kbd) b.append(Object.assign(document.createElement('span'), { className: 'kbd', textContent: kbd }));
  b.addEventListener('click', () => { closeMenus(); onClick(); });
  return b;
}

/** Right-click (or Menu key) on downloads. */
function openRowMenu(x, y) {
  const sel = selectedDownloads();
  if (!sel.length) return;
  const one = sel.length === 1 ? sel[0] : null;
  const canGo = sel.some((d) => action(d.status) === 'continue');
  const canPause = sel.some((d) => action(d.status) === 'pause');
  const items = [];
  if (one) {
    items.push(menuItem('Open', () => run(invoke('open_download', { id: one.id }), 'Couldn\'t open it'), { disabled: one.status !== 'completed' && one.status !== 'seeding', kbd: 'Enter', ico: 'external' }));
    items.push(menuItem('Show in folder', () => run(invoke('show_in_folder', { id: one.id }), 'Couldn\'t show the folder'), { ico: 'folder' }));
    items.push(menuItem('Show download window', () => showWindow(one), { ico: 'maximize' }));
    if (canRefresh(one)) items.push(menuItem('Refresh link…', () => openRefresh(one), { ico: 'refresh' }));
    items.push(document.createElement('hr'));
  }
  items.push(menuItem('Continue', resumeSelected, { disabled: !canGo, kbd: 'Ctrl+R', ico: 'play' }));
  items.push(menuItem('Pause', pauseSelected, { disabled: !canPause, kbd: 'Ctrl+P', ico: 'pause' }));
  if (queues.length) {
    items.push(document.createElement('hr'));
    for (const q of queues) {
      items.push(menuItem(`Move to "${q.name}"`, async () => {
        await run(invoke('set_queue', { ids: sel.map((d) => d.id), queueId: q.id }), 'Couldn\'t move them');
        toast(`Moved to ${q.name}`);
      }, { disabled: sel.every((d) => d.queueId === q.id), ico: 'queue' }));
    }
    if (sel.some((d) => d.queueId != null)) {
      items.push(menuItem('Take out of queue', () => run(invoke('set_queue', { ids: sel.map((d) => d.id), queueId: null }), 'Couldn\'t move them')));
    }
  }
  items.push(document.createElement('hr'));
  if (one) items.push(menuItem('Copy address', () => copyAddress(one), { ico: 'copy' }));
  items.push(menuItem(one ? 'Remove from list…' : `Remove ${sel.length} from list…`, () => openRemove(sel), { danger: true, kbd: 'Del', ico: 'trash' }));
  $('menu-row').replaceChildren(...items);
  openMenu($('menu-row'), x, y);
}

/** Exit for real (closing the window only hides it in the tray). Asks first while
 *  something is downloading. */
function askExit() {
  const running = [...downloads.values()].filter((d) => ['connecting', 'downloading'].includes(d.status)).length;
  if (!running) return run(invoke('exit_app'), 'Couldn\'t exit');
  $('exit-text').textContent = `${running === 1 ? 'A download is' : `${running} downloads are`} running. Exiting pauses ${running === 1 ? 'it' : 'them'}, keeping what's downloaded; continue any time after opening the app again.`;
  $('dlg-exit').showModal();
}

/** The download's own window (progress, sections, Pause), as in the browser's download bar. */
function showWindow(d) {
  run(invoke('open_download_window', { id: d.id }), 'Couldn\'t open the window');
}

/** Show or hide columns; also on right-click of the header. */
function openColumnsMenu(x, y) {
  const visible = table.columns.map((c) => c.key);
  const items = [Object.assign(document.createElement('p'), { className: 'menu-title', textContent: 'Columns' })];
  const ordered = table.prefs.order.map((k) => COLUMNS.find((c) => c.key === k)).filter(Boolean);
  for (const [i, c] of ordered.entries()) {
    const b = document.createElement('button');
    b.setAttribute('role', 'menuitemcheckbox');
    const on = visible.includes(c.key);
    b.setAttribute('aria-checked', String(on));
    const mark = document.createElement('span');
    mark.className = 'check-mark';
    if (on) mark.append(icon('check'));
    b.append(mark, c.label);
    if (c.key === 'name') b.disabled = true;
    b.addEventListener('click', () => {
      table.toggleColumn(c.key);
      render();
      const r = $('menu-columns').getBoundingClientRect();
      openColumnsMenu(r.left, r.top);
      $('menu-columns').querySelectorAll('[aria-checked]')[i]?.focus();
    });
    items.push(b);
  }
  items.push(document.createElement('hr'));
  items.push(menuItem('Reset columns', () => { table.resetColumns(); render(); }));
  $('menu-columns').replaceChildren(...items);
  openMenu($('menu-columns'), x, y);
  $('btn-columns').setAttribute('aria-expanded', 'true');
}

async function copyAddress(d) {
  try {
    await navigator.clipboard.writeText(d.request?.url ?? '');
    toast('Address copied');
  } catch {
    toast('Couldn\'t copy');
  }
}

/** ↑/↓ move, Shift extends, Enter opens or continues, Space pauses or continues. */
function onListKey(e) {
  const rows = [...$('rows').children];
  if (!rows.length) return;
  const ids = rows.map((r) => Number(r.dataset.id));
  const at = ids.indexOf(lastClicked);
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    const next = Math.max(0, Math.min(ids.length - 1, at < 0 ? 0 : at + (e.key === 'ArrowDown' ? 1 : -1)));
    if (!e.shiftKey) selected.clear();
    selected.add(ids[next]);
    lastClicked = ids[next];
    render();
    requestAnimationFrame(() => rows[next]?.scrollIntoView({ block: 'nearest' }));
  } else if (e.key === 'Enter' && selected.size === 1) {
    e.preventDefault();
    onRowDoubleClick({ target: rows[at] ?? rows[0] });
  } else if (e.key === ' ' && selected.size) {
    e.preventDefault();
    if (selectedDownloads().some((d) => action(d.status) === 'pause')) pauseSelected(); else resumeSelected();
  } else if ((e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) && selected.size) {
    e.preventDefault();
    const r = rows[at]?.getBoundingClientRect() ?? $('list').getBoundingClientRect();
    openRowMenu(r.left + 40, r.bottom);
  }
}

function bindExtras() {
  $('list').addEventListener('keydown', onListKey);
  $('rows').addEventListener('contextmenu', (e) => {
    const tr = e.target.closest('tr[data-id]');
    if (!tr) return;
    e.preventDefault();
    const id = Number(tr.dataset.id);
    if (!selected.has(id)) {
      selected.clear();
      selected.add(id);
      lastClicked = id;
      render();
    }
    openRowMenu(e.clientX, e.clientY);
  });
  $('btn-more').addEventListener('click', (e) => {
    e.stopPropagation();
    if (!$('menu-more').hidden) return closeMenus();
    const r = $('btn-more').getBoundingClientRect();
    openMenu($('menu-more'), r.left, r.bottom + 4);
    $('btn-more').setAttribute('aria-expanded', 'true');
  });
  $('btn-grab').addEventListener('click', () => openGrab());
  $('btn-clear-finished').addEventListener('click', () => openRemove(selectedDownloads(), 'finished'));
  $('empty-add').addEventListener('click', () => openAdd());
  $('qb-schedule').addEventListener('click', () => { const q = currentQueue(); if (q) openSchedule(q); });
  addEventListener('mousedown', (e) => { if (!e.target.closest('.menu, #btn-more, #btn-columns')) closeMenus(); });
  $('btn-columns').addEventListener('click', (e) => {
    e.stopPropagation();
    if (!$('menu-columns').hidden) return closeMenus();
    const r = $('btn-columns').getBoundingClientRect();
    openColumnsMenu(r.right - 220, r.bottom + 4);
  });
  $('table').querySelector('thead').addEventListener('contextmenu', (e) => { e.preventDefault(); openColumnsMenu(e.clientX, e.clientY); });
  $('dlg-exit').addEventListener('close', () => {
    if ($('dlg-exit').returnValue === 'exit') run(invoke('exit_app'), 'Couldn\'t exit');
  });
  $('btn-mini').addEventListener('click', () => run(invoke('show_mini'), 'Couldn\'t open the mini window'));
  $('d-refresh').addEventListener('click', () => { const [d] = selectedDownloads(); if (d) openRefresh(d); });
  listen('refresh-request', (id) => { const d = downloads.get(id); if (d) openRefresh(d); });
  $('d-window').addEventListener('click', () => { const [d] = selectedDownloads(); if (d) showWindow(d); });
  addEventListener('blur', closeMenus);
  addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && document.querySelector('.menu:not([hidden])')) { closeMenus(); e.stopPropagation(); }
    // Arrow keys inside an open menu.
    const menu = document.querySelector('.menu:not([hidden])');
    if (menu && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      e.preventDefault();
      const items = [...menu.querySelectorAll('button:not(:disabled)')];
      const i = items.indexOf(document.activeElement);
      items[(i + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
    }
  }, true);
  // The network went away (downloads were paused) or came back.
  const OFFLINE = 'Paused: the internet connection was lost';
  listen('network', (n) => {
    const b = $('network-banner');
    b.classList.toggle('alert', !n.online);
    b.classList.toggle('good', !!n.online);
    $('network-continue').hidden = !n.online || ![...downloads.values()].some((d) => d.status === 'paused' && d.error?.startsWith(OFFLINE));
    $('network-text').textContent = !n.online
      ? `No internet connection.${n.paused ? ` ${n.paused === 1 ? 'A download was' : `${n.paused} downloads were`} paused, keeping what they have; continue ${n.paused === 1 ? 'it' : 'them'} when you're back online.` : ''}`
      : 'You\'re back online.';
    b.hidden = !!n.online && $('network-continue').hidden;
  });
  $('network-continue').addEventListener('click', async () => {
    const ids = [...downloads.values()].filter((d) => d.status === 'paused' && d.error?.startsWith(OFFLINE)).map((d) => d.id);
    if (ids.length) await run(invoke('resume', { ids }), 'Couldn\'t continue');
    $('network-banner').hidden = true;
  });
  $('network-close').addEventListener('click', () => { $('network-banner').hidden = true; });
  // A finished queue asked to shut the computer down: offer to cancel.
  listen('shutdown-pending', (name) => {
    $('shutdown-text').textContent = `"${name}" is done. The computer shuts down in about a minute.`;
    $('shutdown-banner').hidden = false;
  });
  $('shutdown-cancel').addEventListener('click', async () => {
    await run(invoke('cancel_shutdown'), 'Couldn\'t cancel');
    $('shutdown-banner').hidden = true;
    toast('Shutdown cancelled');
  });
}

function bind() {
  bindExtras();
  $('rows').addEventListener('click', onRowClick);
  $('rows').addEventListener('dblclick', onRowDoubleClick);
  $('nav').addEventListener('click', (e) => {
    const b = e.target.closest('.nav-item');
    if (!b) return;
    filter = b.dataset.filter;
    selected.clear();
    render();
  });
  $('btn-new-queue').addEventListener('click', (e) => { e.stopPropagation(); openQueue(); });
  $('qb-start').addEventListener('click', () => { const q = currentQueue(); if (q) run(invoke('queue_start', { id: q.id }), 'Couldn\'t start the queue'); });
  $('qb-stop').addEventListener('click', () => { const q = currentQueue(); if (q) run(invoke('queue_stop', { id: q.id }), 'Couldn\'t stop the queue'); });
  $('qb-rename').addEventListener('click', () => { const q = currentQueue(); if (q) openQueue(q); });
  $('qb-delete').addEventListener('click', () => { const q = currentQueue(); if (q) openDeleteQueue(q, viewDownloads().length); });
  $('qb-max').addEventListener('change', (e) => {
    const q = currentQueue();
    if (q) run(invoke('queue_update', { id: q.id, name: null, maxActive: Number(e.target.value) }), 'Couldn\'t change the queue');
  });
  $('d-queue').addEventListener('change', async (e) => {
    const ids = [...selected];
    const queueId = e.target.value ? Number(e.target.value) : null;
    await run(invoke('set_queue', { ids, queueId }), 'Couldn\'t move it');
    toast(queueId ? `Moved to ${queues.find((q) => q.id === queueId)?.name}` : 'Taken out of the queue');
  });
  $('search').addEventListener('input', (e) => { query = e.target.value.trim(); render(); });
  $('btn-add').addEventListener('click', () => openAdd());
  $('btn-resume').addEventListener('click', resumeSelected);
  $('btn-pause').addEventListener('click', pauseSelected);
  $('btn-delete').addEventListener('click', () => openRemove(selectedDownloads()));
  $('btn-delete').disabled = false;
  $('btn-pause-all').addEventListener('click', () => run(invoke('pause_all'), 'Couldn\'t pause'));
  $('btn-resume-all').addEventListener('click', () => run(invoke('resume_all'), 'Couldn\'t continue'));
  $('btn-settings').addEventListener('click', () => openSettings());
  $('privacy-banner-btn').addEventListener('click', () => openSettings('privacy'));
  $('credit').addEventListener('click', () => run(invoke('open_portfolio'), 'Couldn\'t open the link'));
  $('btn-exit').addEventListener('click', askExit);
  $('d-folder').addEventListener('click', () => { const [d] = selectedDownloads(); if (d) run(invoke('show_in_folder', { id: d.id }), 'Couldn\'t show the folder'); });
  $('d-copy').addEventListener('click', () => { const [d] = selectedDownloads(); if (d) copyAddress(d); });
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
    else if (ctrl && e.key === ',') { e.preventDefault(); openSettings(); }
    else if (ctrl && e.key.toLowerCase() === 'q') { e.preventDefault(); askExit(); }
    else if (ctrl && e.key.toLowerCase() === 'a') { e.preventDefault(); for (const r of $('rows').children) selected.add(Number(r.dataset.id)); render(); }
    else if (ctrl && e.key.toLowerCase() === 'p') { e.preventDefault(); pauseSelected(); }
    else if (ctrl && e.key.toLowerCase() === 'r') { e.preventDefault(); resumeSelected(); }
    else if (e.key === 'Delete') openRemove(selectedDownloads());
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
  hydrateIcons();
  table = new DownloadTable({ table: $('table'), onChange: render });
  initLayout();
  initTooltips();
  bind();
  info = { ...info, ...(await run(invoke('app_info'), 'Couldn\'t start')) };
  // The saved theme, for this window and the small ones (they read it from localStorage).
  invoke('get_settings').then((s) => window.ldmTheme?.set(s?.theme ?? 'system')).catch(() => {});
  initDialogs();
  for (const d of (await run(invoke('list'), 'Couldn\'t load downloads')) ?? []) if (!d.pending) downloads.set(d.id, d);
  queues = (await run(invoke('queues_list'), 'Couldn\'t load queues')) ?? [];
  await listen('engine', onEngine);
  await listen('queues', (list) => { queues = list; render(); });
  render();
}

start();
