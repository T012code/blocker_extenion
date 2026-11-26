// The mini window: everything downloading at a glance, above other windows. Expand it
// to see each download; click one to open its window. It hides itself a few seconds
// after the last download finishes, and comes back when the next one starts.

import { invoke, isMock, listen } from './backend.js';
import { bytes, eta, percent, speed } from './format.js';
import { hydrateIcons } from './icons.js';
import { initTooltips } from './tooltip.js';

const $ = (id) => document.getElementById(id);
const WIDTH = 320;
const RUNNING = ['connecting', 'downloading'];
const downloads = new Map();
const live = new Map();
let expanded = false;
let hideTimer = null;

const progressOf = (d) => {
  const l = live.get(d.id);
  return { done: l ? l.downloaded : d.downloaded, size: l?.size ?? d.size, speed: l ? l.speed : 0 };
};

function render() {
  const active = [...downloads.values()].filter((d) => RUNNING.includes(d.status));
  const waiting = [...downloads.values()].filter((d) => d.status === 'queued').length;
  let done = 0; let size = 0; let total = 0; let known = true;
  for (const d of active) {
    const p = progressOf(d);
    done += p.done;
    total += p.speed;
    if (p.size) size += p.size; else known = false;
  }
  document.body.classList.toggle('active', active.length > 0);
  document.body.classList.toggle('idle', active.length === 0);
  const pct = known && size ? percent(done, size) : null;
  $('summary').textContent = active.length === 0 ? 'All downloads finished'
    : `${active.length} downloading${pct != null ? ` · ${pct}%` : ''}`;
  const left = known && size ? eta(size - done, total) : '';
  $('sub').textContent = active.length === 0 ? (waiting ? `${waiting} waiting` : 'Nothing left to download')
    : [speed(total) || 'Starting…', left, waiting ? `${waiting} waiting` : ''].filter(Boolean).join(' · ');
  $('total').firstElementChild.style.setProperty('--p', String(active.length ? (pct ?? 0) / 100 : 1));
  document.title = active.length ? `${active.length} downloading` : 'Downloads';

  // The list, shown when expanded. Rows are kept so their bars animate smoothly.
  const list = $('items');
  const want = active.slice(0, 12);
  const keep = new Set(want.map((d) => String(d.id)));
  for (const li of [...list.children]) if (!keep.has(li.dataset.id)) li.remove();
  for (const d of want) {
    let li = list.querySelector(`li[data-id="${d.id}"]`);
    if (!li) {
      li = document.createElement('li');
      li.dataset.id = String(d.id);
      li.innerHTML = '<button type="button"><span class="n"></span><span class="s"></span><span class="bar"><span></span></span></button>';
      li.firstChild.addEventListener('click', () => invoke('open_download_window', { id: d.id }));
      list.append(li);
    }
    const p = progressOf(d);
    const dp = percent(p.done, p.size);
    li.className = d.status;
    li.querySelector('.n').textContent = d.fileName;
    li.firstChild.title = `${d.fileName}. Click to open its window.`;
    li.querySelector('.s').textContent = [dp != null ? `${dp}%` : bytes(p.done), speed(p.speed)].filter(Boolean).join(' · ');
    li.querySelector('.bar > span').style.setProperty('--p', String((dp ?? 0) / 100));
  }
  $('expand').disabled = active.length === 0;
  if (!active.length && expanded) setExpanded(false);

  // Nothing left: say so briefly, then step aside.
  if (!active.length && !hideTimer && !isMock) hideTimer = setTimeout(() => invoke('window_hide'), 4000);
  if (active.length && hideTimer) { clearTimeout(hideTimer); hideTimer = null; }
  fit();
}

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

function setExpanded(on) {
  expanded = on;
  $('items').hidden = !on;
  $('expand').setAttribute('aria-expanded', String(on));
  $('expand').title = on ? 'Show the summary only' : 'Show each download';
  $('expand').setAttribute('aria-label', $('expand').title);
  fit();
}

async function start() {
  hydrateIcons();
  initTooltips();
  setExpanded(false);
  for (const d of (await invoke('list').catch(() => [])) ?? []) downloads.set(d.id, d);
  render();
  await listen('engine', (ev) => {
    if (ev.type === 'progress') live.set(ev.id, ev);
    else if (ev.type === 'added' || ev.type === 'updated') {
      downloads.set(ev.download.id, ev.download);
      if (!RUNNING.includes(ev.download.status)) live.delete(ev.download.id);
    } else if (ev.type === 'removed') { downloads.delete(ev.id); live.delete(ev.id); }
    render();
  });
}

$('expand').addEventListener('click', () => setExpanded(!expanded));
$('app').addEventListener('click', () => invoke('show_app'));
$('dismiss').addEventListener('click', () => invoke('mini_dismiss'));
start();
