// A download's own window: progress, speed, sections and Pause while it runs; Open and
// Show in folder when it's done. It can shrink to one line, or be hidden (the download
// carries on). Its label is "dl-<id>".

import { invoke, isMock, listen, windowLabel } from './backend.js';
import { STATUS_TEXT, action, bytes, eta, percent, speed } from './format.js';
import { hydrateIcons, icon } from './icons.js';
import { renderMap } from './sectionmap.js';
import { initTooltips } from './tooltip.js';
import { statusEl } from './table.js';

const $ = (id) => document.getElementById(id);
const id = Number(isMock ? new URLSearchParams(location.search).get('id') : windowLabel().slice(3));
const WIDTH = 460;
const COMPACT_KEY = 'ldm.progress.compact';

let d = null;
let live = null;
let compact = false;
try { compact = localStorage.getItem(COMPACT_KEY) === '1'; } catch { /* fine */ }

const host = (url) => { try { return new URL(url).host; } catch { return url.startsWith('magnet:') ? 'Magnet link' : ''; } };

function toast(text) {
  const t = $('toast');
  t.textContent = text;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { t.hidden = true; }, 2400);
}

const run = (p, fail) => p.catch((e) => toast(`${fail}: ${e?.message ?? e}`));

function progress() {
  const running = ['connecting', 'downloading', 'seeding', 'blocked'].includes(d.status);
  return {
    done: live && running ? live.downloaded : d.downloaded,
    size: live?.size ?? d.size,
    speed: live && running ? live.speed : 0,
    segments: live?.segments ?? [],
  };
}

/** What each stopped state means for you, in one line. */
const HINT = {
  paused: 'Paused. Continue whenever you like.',
  queued: 'Waiting to start.',
  connecting: 'Connecting to the server…',
  needsRefresh: 'The link expired. Refresh it to carry on from here.',
  blocked: 'Waiting for your VPN or proxy before it connects to anyone.',
  failed: 'It stopped with a problem. Continue to try again.',
  seeding: 'Done. Sharing with others until the share ratio is reached.',
};

let shownStatus = '';
function render() {
  if (!d) return;
  const p = progress();
  const finished = d.status === 'completed';
  $('running').hidden = finished;
  $('done').hidden = !finished;
  $('win').dataset.status = d.status;
  const pct = finished ? 100 : percent(p.done, p.size);
  const title = finished ? `Done · ${d.fileName}` : `${pct != null ? `${pct}% · ` : ''}${d.fileName}`;
  if (document.title !== title) { document.title = title; invoke('window_title', { title }).catch(() => {}); }

  if (finished) {
    $('done-name').textContent = d.fileName;
    $('done-name').title = d.fileName;
    $('done-meta').textContent = `${bytes(d.size ?? p.done)} · ${d.saveDir}`;
    $('done-meta').title = d.saveDir;
    fit();
    return;
  }
  $('name').textContent = d.fileName;
  $('name').title = d.fileName;
  $('host').textContent = host(d.request?.url ?? '');
  $('pct').textContent = pct != null ? `${pct}%` : '—';
  $('amount').textContent = p.size ? `${bytes(p.done)} of ${bytes(p.size)}` : bytes(p.done);
  $('bar').firstElementChild.style.setProperty('--p', String((pct ?? 0) / 100));
  $('bar').setAttribute('aria-valuenow', String(pct ?? 0));
  $('bar').classList.toggle('live', d.status === 'downloading');
  if (shownStatus !== d.status) {
    const s = statusEl(d);
    s.id = 'status';
    if (shownStatus) s.classList.add('changed');
    $('status').replaceWith(s);
    shownStatus = d.status;
  }
  const downloading = d.status === 'downloading';
  const left = downloading ? eta((p.size ?? 0) - p.done, p.speed) : '';
  $('speed').textContent = downloading ? speed(p.speed) || '—' : '—';
  $('eta').textContent = left ? left.replace(' left', '') : '—';
  $('line').textContent = downloading ? [speed(p.speed), left].filter(Boolean).join(' · ')
    : HINT[d.status] ?? STATUS_TEXT[d.status] ?? '';
  const http = (d.kind?.type ?? 'http') === 'http';
  const active = p.segments.filter((s) => s.active).length;
  $('sections').textContent = http ? (downloading && active ? `${active} of ${d.connections}` : String(d.connections)) : '—';
  $('resume').textContent = d.kind?.type === 'torrent' || d.kind?.type === 'media' || d.resumable ? 'Yes' : 'No';
  $('error').hidden = !d.error;
  $('error').textContent = d.error ?? '';
  renderMap($('map'), d, p);
  // Pause or Continue, as the download allows.
  const act = action(d.status);
  const b = $('pause');
  b.disabled = !act;
  const refresh = d.status === 'needsRefresh' && (d.kind?.type ?? 'http') === 'http';
  const want = refresh ? 'refresh' : act === 'continue' ? 'continue' : 'pause';
  if (b.dataset.act !== want) {
    b.dataset.act = want;
    const [ico, text] = { refresh: ['refresh', 'Refresh link'], continue: ['play', 'Continue'], pause: ['pause', 'Pause'] }[want];
    b.replaceChildren(icon(ico), Object.assign(document.createElement('span'), { className: 'label', textContent: text }));
  }
}

// ---- The same link came in again: ask what to do (as IDM does).

async function checkDuplicate() {
  const item = await invoke('get_duplicate', { id }).catch(() => null);
  $('dup').hidden = !item;
  if (!item || !d) return fit();
  const done = d.status === 'completed' || d.status === 'seeding';
  $('dup-title').textContent = done ? 'You\'ve downloaded this before' : 'This is already downloading';
  $('dup-text').textContent = done
    ? `${d.fileName} is already on this computer. Open it, or download a second copy with a numbered name.`
    : `${d.fileName} is in your list, not finished yet. Continue it, or start a second copy.`;
  $('dup-existing').textContent = done ? 'Open it' : 'Continue it';
  $('dup-again').hidden = d.kind?.type === 'torrent';
  $('dup-remember').checked = false;
  fit();
  $('dup-existing').focus();
}

async function resolveDuplicate(choice) {
  const remember = $('dup-remember').checked;
  $('dup').hidden = true;
  fit();
  await run(invoke('resolve_duplicate', { id, choice, remember }), 'Couldn\'t do that');
  // Opened the finished file, or started a second copy (which gets its own window).
  if (choice !== 'dismiss' && (d.status === 'completed' || choice === 'again')) invoke('window_close');
}

function setType() {
  const t = d.kind?.type === 'torrent' ? 'torrents' : d.kind?.type === 'media' ? 'video' : d.category ?? 'general';
  $('type').replaceChildren(icon(t, { size: 20 }));
}

// The window is as tall as its content: one line, everything, or "done".
let lastH = 0;
function fit() {
  requestAnimationFrame(() => {
    const h = Math.ceil($('win').getBoundingClientRect().height);
    if (h && Math.abs(h - lastH) > 1) {
      lastH = h;
      invoke('window_size', { width: WIDTH, height: h }).catch(() => {});
    }
  });
}

function setCompact(on) {
  compact = on;
  document.body.classList.toggle('compact', on);
  const t = $('toggle');
  t.setAttribute('aria-expanded', String(!on));
  t.title = on ? 'Show more' : 'Show less (one line)';
  t.querySelector('.label').textContent = on ? 'More' : 'Less';
  try { localStorage.setItem(COMPACT_KEY, on ? '1' : '0'); } catch { /* fine */ }
  fit();
}

async function start() {
  hydrateIcons();
  initTooltips();
  setCompact(compact);
  new ResizeObserver(fit).observe($('win'));
  try {
    d = await invoke('get', { id });
  } catch {
    document.title = 'Download removed';
    $('name').textContent = 'This download was removed.';
    return;
  }
  setType();
  render();
  await checkDuplicate();
  await listen('duplicate', (dupId) => { if (dupId === id) checkDuplicate(); });
  await listen('engine', (ev) => {
    if (ev.type === 'progress' && ev.id === id) live = ev;
    else if ((ev.type === 'updated' || ev.type === 'added') && ev.download.id === id) {
      const was = d.status;
      d = ev.download;
      if (!['connecting', 'downloading', 'seeding', 'blocked'].includes(d.status)) live = null;
      if (was !== d.status && d.status === 'completed') shownStatus = '';
      setType();
    } else if (ev.type === 'removed' && ev.id === id) {
      invoke('window_close');
      return;
    } else return;
    render();
  });
}

$('toggle').addEventListener('click', () => setCompact(!compact));
$('dup-existing').addEventListener('click', () => resolveDuplicate('existing'));
$('dup-again').addEventListener('click', () => resolveDuplicate('again'));
$('dup-dismiss').addEventListener('click', () => resolveDuplicate('dismiss'));
$('hide').addEventListener('click', () => invoke('window_close'));
$('close').addEventListener('click', () => invoke('window_close'));
$('pause').addEventListener('click', () => {
  if (!d) return;
  if ($('pause').dataset.act === 'refresh') run(invoke('request_refresh', { id: d.id }), 'Couldn\'t open the app');
  else if ($('pause').dataset.act === 'continue') run(invoke('resume', { ids: [d.id] }), 'Couldn\'t continue');
  else run(invoke('pause', { ids: [d.id] }), 'Couldn\'t pause');
});
$('open').addEventListener('click', async () => {
  await run(invoke('open_download', { id }), 'Couldn\'t open it');
  invoke('window_close');
});
$('folder').addEventListener('click', () => run(invoke('show_in_folder', { id }), 'Couldn\'t show the folder'));
$('dont-show').addEventListener('change', async (e) => {
  const s = await invoke('get_settings').catch(() => null);
  if (!s) return;
  s.showCompleteWindow = !e.target.checked;
  await run(invoke('save_settings', { settings: s }), 'Couldn\'t save that');
});
addEventListener('keydown', (e) => {
  if (e.key === 'Escape') invoke('window_close');
});
start();
