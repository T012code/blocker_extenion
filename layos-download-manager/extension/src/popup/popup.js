import { api } from '../shared/api.js';
import { STATUS_TEXT, action, bytes, eta, percent, speed } from '../shared/format.js';

const HOST = 'com.layo.downloadmanager';
const $ = (id) => document.getElementById(id);

// ---- Connection to the app, kept open while the pop-up is.
// The bridge answers in order, so replies are matched to requests first-in, first-out.
let port = null;
const waiting = [];

function connect() {
  try {
    port = api.runtime.connectNative(HOST);
  } catch {
    port = null;
    return;
  }
  port.onMessage.addListener((msg) => waiting.shift()?.resolve(msg));
  port.onDisconnect.addListener(() => {
    const err = new Error(api.runtime.lastError?.message ?? 'disconnected');
    port = null;
    while (waiting.length) waiting.shift().reject(err);
  });
}

function ask(message) {
  if (!port) connect();
  if (!port) return Promise.reject(new Error('not connected'));
  return new Promise((resolve, reject) => {
    waiting.push({ resolve, reject });
    port.postMessage(message);
  });
}

// ---- Status and settings

function setBadge(kind, text, detail) {
  const b = $('status-badge');
  b.className = `badge ${kind}`;
  b.textContent = text;
  $('status-detail').textContent = detail;
}

function showStatus(host, settings, appRunning) {
  if (!host.installed) {
    setBadge('bad', 'Not installed', 'Install Layó\'s Download Manager for Windows. Until then, downloads stay in the browser.');
  } else if (!settings.enabled) {
    setBadge('warn', 'Paused', 'The browser is handling downloads itself.');
  } else if (appRunning) {
    setBadge('ok', 'Connected', 'Downloads are taken over.');
  } else {
    setBadge('info', 'Ready', 'The app starts when a download comes in.');
  }
  $('open-app').disabled = !host.installed;
}

function showReturned(history) {
  const back = history.filter((h) => h.status === 'returned').slice(0, 5);
  $('returned').hidden = back.length === 0;
  $('returned-list').replaceChildren(...back.map((h) => {
    const li = document.createElement('li');
    const s = document.createElement('span');
    s.textContent = h.name || h.url;
    s.title = `${h.url}\n${h.reason ?? ''}`;
    li.append(s);
    return li;
  }));
}

// ---- Download list with pause and continue

const busy = new Set();

function row(d) {
  const li = document.createElement('li');
  const pct = percent(d.downloaded, d.size);
  li.className = `dl ${d.status}${pct == null && d.status === 'downloading' ? ' unknown' : ''}`;

  const name = document.createElement('span');
  name.className = 'name';
  name.textContent = d.fileName;
  name.title = d.fileName;

  const what = action(d.status);
  let btn = document.createElement('span');
  if (what) {
    btn = document.createElement('button');
    btn.className = `act ${what === 'continue' ? 'go' : ''}`;
    btn.textContent = what === 'pause' ? 'Pause' : 'Continue';
    btn.disabled = busy.has(d.id);
    if (what === 'pause' && !d.resumable) {
      btn.title = 'This server can\'t resume downloads. Continuing later starts the file again from the beginning.';
    }
    btn.addEventListener('click', () => toggle(d.id, what));
  }

  const bar = document.createElement('div');
  bar.className = 'bar';
  bar.setAttribute('role', 'progressbar');
  if (pct != null) bar.setAttribute('aria-valuenow', String(pct));
  bar.setAttribute('aria-label', `${d.fileName} progress`);
  const fill = document.createElement('span');
  fill.style.width = `${d.status === 'completed' ? 100 : pct ?? 0}%`;
  bar.append(fill);

  const meta = document.createElement('div');
  meta.className = 'meta';
  const parts = [STATUS_TEXT[d.status] ?? d.status];
  if (d.status !== 'completed') {
    parts.push(d.size ? `${bytes(d.downloaded)} of ${bytes(d.size)}${pct != null ? ` (${pct}%)` : ''}` : bytes(d.downloaded));
  } else if (d.size) {
    parts.push(bytes(d.size));
  }
  if (d.status === 'downloading') {
    parts.push(speed(d.speed), eta((d.size ?? 0) - d.downloaded, d.speed), `${d.connections} sections`);
  }
  if (d.status === 'needsRefresh') parts.push('download it again from the page to continue');
  meta.textContent = parts.filter(Boolean).join(' · ');
  if (!d.resumable && d.status !== 'completed' && d.status !== 'queued' && d.size !== null) {
    const tag = document.createElement('span');
    tag.className = 'badge warn';
    tag.textContent = 'Can\'t resume';
    tag.title = 'The server doesn\'t support resuming. Pausing means starting again from the beginning.';
    meta.append(tag);
  }
  if (d.error && d.status === 'failed') meta.title = d.error;

  li.append(name, btn, bar, meta);
  return li;
}

let lastItems = [];

function render(items) {
  lastItems = items;
  $('downloads').replaceChildren(...items.slice(0, 12).map(row));
  $('downloads-empty').hidden = items.length > 0;
}

async function toggle(id, what) {
  busy.add(id);
  render(lastItems);
  try {
    await ask({ type: what === 'pause' ? 'pause' : 'resume', id });
  } catch (e) {
    $('status-detail').textContent = `Couldn't ${what}: ${e.message}`;
  }
  busy.delete(id);
  await poll();
}

let appRunning = false;
let shown = null;
async function poll() {
  try {
    const r = await ask({ type: 'list' });
    if (r.type === 'downloads') {
      appRunning = r.appRunning;
      render(r.items);
    }
  } catch {
    appRunning = false;
  }
  if (shown) showStatus(shown.host, shown.settings, appRunning);
  return appRunning;
}

// ---- Start

async function load() {
  const { host, settings, history } = await api.runtime.sendMessage({ type: 'status' });
  $('enabled').checked = settings.enabled;
  const key = { alt: 'Alt', shift: 'Shift', ctrl: 'Ctrl' }[settings.bypassKey];
  $('bypass-hint').textContent = key ? `Hold ${key} while clicking a link to skip` : 'Only file types in your list';
  showReturned(history);
  shown = { host, settings };
  if (host.installed) await poll();
  showStatus(host, settings, appRunning);
  return host;
}

$('enabled').addEventListener('change', async (e) => {
  await api.runtime.sendMessage({ type: 'setEnabled', enabled: e.target.checked });
  load();
});
$('options').addEventListener('click', () => api.runtime.openOptionsPage());
$('open-app').addEventListener('click', async () => {
  const r = await api.runtime.sendMessage({ type: 'showApp' });
  if (r?.ok) window.close();
  else $('status-detail').textContent = r?.error ?? 'Could not open the app.';
});

const host = await load();
if (host.installed) setInterval(poll, 1000);
