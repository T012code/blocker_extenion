// Dialogs: add a download, downloads sent by the browser, remove, and settings.

import { invoke, listen, pick } from './backend.js';
import { bytes } from './format.js';
import { allDownloads, currentQueue, fillSectionSelect, info, queues, run, toast, viewDownloads } from './app.js';

const $ = (id) => document.getElementById(id);

function showError(el, text) {
  el.textContent = text ?? '';
  el.hidden = !text;
}

const isMagnet = (u) => /^magnet:\?/i.test(u);
const looksLikeFile = (u) => /\.[a-z0-9]{2,5}(?:[?#]|$)/i.test(new URL(u, 'https://x').pathname);

// ---------------------------------------------------------------- add

let addState = { media: null, torrentFile: null };

export function openAdd({ url = '' } = {}) {
  addState = { media: null, torrentFile: null };
  $('add-form').reset();
  $('add-url').value = url;
  $('add-info').hidden = true;
  $('add-formats').hidden = true;
  showError($('add-error'), '');
  run(invoke('get_settings'), 'Couldn\'t read settings').then((s) => {
    fillSectionSelect($('add-sections'), s?.defaultConnections ?? 8);
  });
  $('add-queue').replaceChildren(
    Object.assign(document.createElement('option'), { value: '', textContent: 'No queue (start now)' }),
    ...queues.map((q) => Object.assign(document.createElement('option'), { value: String(q.id), textContent: `${q.name}${q.running ? ' (running)' : ''}` })),
  );
  // Adding while looking at a queue puts it in that queue.
  $('add-queue').value = currentQueue() ? String(currentQueue().id) : '';
  $('dlg-add').showModal();
  $('add-url').focus();
  if (url) check();
}

function infoBox(lines) {
  const box = $('add-info');
  box.replaceChildren(...lines.map(([label, value]) => {
    const p = document.createElement('div');
    const b = document.createElement('strong');
    b.textContent = label;
    p.append(b, ` ${value}`);
    return p;
  }));
  box.hidden = false;
}

async function check() {
  const url = $('add-url').value.trim();
  showError($('add-error'), '');
  addState.media = null;
  $('add-formats').hidden = true;
  if (!url) return;
  if (isMagnet(url)) {
    const name = new URLSearchParams(url.slice(url.indexOf('?'))).get('dn');
    infoBox([['Torrent:', name || 'magnet link'], ['Privacy:', (await privacyText()) ]]);
    $('add-sections').disabled = true;
    return;
  }
  $('add-sections').disabled = false;
  $('add-check').disabled = true;
  $('add-check').textContent = 'Checking…';
  try {
    if (looksLikeFile(url)) {
      const p = await invoke('probe', { url });
      showFile(p);
    } else {
      // A web page: maybe a video. Ask for formats, fall back to a plain download.
      try {
        const f = await invoke('media_formats', { url });
        showFormats(url, f);
      } catch {
        showFile(await invoke('probe', { url }));
      }
    }
  } catch (e) {
    showError($('add-error'), `Couldn't check the address: ${e?.message ?? e}`);
  } finally {
    $('add-check').disabled = false;
    $('add-check').textContent = 'Check';
  }
}

function showFile(p) {
  if (!$('add-name').value) $('add-name').placeholder = p.fileName;
  infoBox([
    ['File:', p.fileName],
    ['Size:', p.size != null ? bytes(p.size) : 'unknown'],
    ['Pause and continue:', p.resumable ? 'yes' : 'no — the server can\'t resume, so it would start again'],
  ]);
}

function showFormats(url, f) {
  infoBox([['Video:', f.title]]);
  const box = $('add-formats');
  box.replaceChildren(...f.choices.map((c, i) => {
    const l = document.createElement('label');
    const r = document.createElement('input');
    r.type = 'radio';
    r.name = 'format';
    r.value = String(i);
    r.checked = i === 0;
    const t = document.createElement('span');
    t.textContent = c.label;
    const s = document.createElement('span');
    s.className = 'size';
    s.textContent = c.size ? bytes(c.size) : '';
    l.append(r, t, s);
    r.addEventListener('change', () => { addState.media = { pageUrl: url, format: c.format, label: c.label, title: f.title }; });
    return l;
  }));
  box.hidden = f.choices.length === 0;
  if (f.choices[0]) addState.media = { pageUrl: url, format: f.choices[0].format, label: f.choices[0].label, title: f.title };
}

async function privacyText() {
  const why = await invoke('privacy_status').catch(() => null);
  return why ? `waiting — ${why}` : 'protected by your settings';
}

async function submitAdd(start) {
  const url = $('add-url').value.trim();
  if (!url && !addState.torrentFile) {
    showError($('add-error'), 'Enter a download address');
    return;
  }
  const args = {
    url,
    fileName: $('add-name').value.trim() || null,
    saveDir: $('add-dir').value.trim() || null,
    connections: Number($('add-sections').value) || null,
    start,
    media: addState.media,
    torrentFile: addState.torrentFile,
    queueId: $('add-queue').value ? Number($('add-queue').value) : null,
  };
  try {
    await invoke('add', { args });
    $('dlg-add').close();
    toast(args.queueId ? `Added to ${queues.find((q) => q.id === args.queueId)?.name}` : start ? 'Download started' : 'Added to the list');
  } catch (e) {
    showError($('add-error'), String(e?.message ?? e));
  }
}

// ---------------------------------------------------------------- from the browser

let browserItems = [];

function openBrowser({ items, source }) {
  browserItems = items;
  const several = items.length > 1;
  $('browser-title').textContent = several ? `Download ${items.length} files from your browser` : 'Download from your browser';
  $('browser-items').replaceChildren(...items.map((it, i) => {
    const li = document.createElement('li');
    const l = document.createElement('label');
    const c = document.createElement('input');
    c.type = 'checkbox';
    c.checked = true;
    c.dataset.index = String(i);
    const t = document.createElement('div');
    const n = document.createElement('div');
    n.textContent = it.fileName || it.description || decodeURIComponent(it.url.split('/').pop() || it.url).split('?')[0] || it.url;
    const u = document.createElement('div');
    u.className = 'url';
    u.textContent = `${it.fileSize ? `${bytes(it.fileSize)} · ` : ''}${it.url}`;
    t.append(n, u);
    l.append(c, t);
    li.append(l);
    return li;
  }));
  run(invoke('get_settings'), '').then((s) => fillSectionSelect($('browser-sections'), s?.defaultConnections ?? 8));
  $('browser-dir').value = '';
  void source;
  $('dlg-browser').showModal();
}

async function submitBrowser(start) {
  const picked = [...$('browser-items').querySelectorAll('input:checked')].map((c) => browserItems[Number(c.dataset.index)]);
  for (const it of picked) {
    await run(invoke('add', {
      args: {
        url: it.url, fileName: it.fileName ?? null, referer: it.referer ?? null, cookies: it.cookies ?? null,
        userAgent: it.userAgent ?? null, description: it.description ?? null, media: it.media ?? null,
        saveDir: $('browser-dir').value.trim() || null, connections: Number($('browser-sections').value) || null, start,
      },
    }), 'Couldn\'t add a download');
  }
  $('dlg-browser').close();
  if (picked.length) toast(`${picked.length} download${picked.length > 1 ? 's' : ''} ${start ? 'started' : 'added'}`);
}

// ---------------------------------------------------------------- remove

let removeScopes = {};

/** Remove downloads: the selection, everything finished, everything failed, or the whole view. */
export function openRemove(selection) {
  const all = allDownloads();
  removeScopes = {
    selected: selection,
    finished: all.filter((d) => d.status === 'completed'),
    failed: all.filter((d) => d.status === 'failed' || d.status === 'needsRefresh'),
    view: viewDownloads(),
  };
  const label = (n) => `${n} download${n === 1 ? '' : 's'}`;
  $('scope-selected').textContent = selection.length === 1 ? `Selected: ${selection[0].fileName}` : `Selected (${label(selection.length)})`;
  $('scope-finished').textContent = `All finished (${label(removeScopes.finished.length)})`;
  $('scope-failed').textContent = `All failed or expired (${label(removeScopes.failed.length)})`;
  $('scope-view').textContent = `Everything in this view (${label(removeScopes.view.length)})`;
  for (const r of $('dlg-remove').querySelectorAll('input[name="scope"]')) {
    r.disabled = removeScopes[r.value].length === 0;
  }
  const first = ['selected', 'finished', 'failed', 'view'].find((k) => removeScopes[k].length);
  if (!first) return toast('Nothing to remove');
  $('dlg-remove').querySelector(`input[name="scope"][value="${first}"]`).checked = true;
  $('remove-files').checked = false;
  $('dlg-remove').showModal();
}

// ---------------------------------------------------------------- queues

let editingQueue = null;

export function openQueue(q = null) {
  editingQueue = q;
  $('queue-title').textContent = q ? 'Rename queue' : 'New queue';
  $('queue-ok').textContent = q ? 'Save' : 'Create';
  $('queue-name').value = q?.name ?? '';
  $('queue-max').replaceChildren(...[1, 2, 3, 4, 5, 6, 8, 10].map((n) => Object.assign(document.createElement('option'), { value: String(n), textContent: String(n) })));
  $('queue-max').value = String(q?.maxActive ?? 2);
  $('dlg-queue').showModal();
  $('queue-name').focus();
}

let deletingQueue = null;

export function openDeleteQueue(q, count) {
  deletingQueue = q;
  $('delqueue-title').textContent = `Delete "${q.name}"`;
  $('delqueue-text').textContent = count ? `It has ${count} download${count === 1 ? '' : 's'}.` : 'It has no downloads.';
  $('delqueue-form').elements.dq.value = 'keep';
  $('delqueue-files').checked = false;
  $('delqueue-files').disabled = true;
  $('dlg-delqueue').showModal();
}

// ---------------------------------------------------------------- settings

let current = null;

function form() {
  return $('settings-form').elements;
}

function showTab(name) {
  for (const t of document.querySelectorAll('.tab')) {
    const on = t.dataset.tab === name;
    t.classList.toggle('active', on);
    t.setAttribute('aria-selected', on ? 'true' : 'false');
  }
  for (const p of document.querySelectorAll('.tabpanel')) p.hidden = p.dataset.panel !== name;
}

function modeName(m) {
  return m?.type ?? 'private';
}

function syncModeDetails() {
  const mode = form().mode.value;
  for (const d of document.querySelectorAll('.mode-detail')) d.hidden = d.dataset.for !== mode;
  // In protected modes, "never without a VPN" is a given.
  $('require-wrap').hidden = mode === 'proxy' || mode === 'vpn';
  $('dht-wrap').hidden = mode === 'proxy';
}

async function loadAdapters(selectedName) {
  const names = (await run(invoke('network_adapters'), 'Couldn\'t list network adapters')) ?? [];
  const sel = $('set-adapters');
  const all = selectedName && !names.includes(selectedName) ? [selectedName, ...names] : names;
  sel.replaceChildren(...all.map((n) => Object.assign(document.createElement('option'), { value: n, textContent: n })));
  if (!all.length) sel.append(Object.assign(document.createElement('option'), { value: '', textContent: 'No adapters found' }));
  if (selectedName) sel.value = selectedName;
}

async function refreshPrivacyStatus() {
  const why = await invoke('privacy_status').catch(() => null);
  const line = $('privacy-status');
  line.replaceChildren(Object.assign(document.createElement('span'), {
    className: `badge ${why ? 'warn' : 'ok'}`,
    textContent: why ? 'Torrents waiting' : 'Protected',
  }), ` ${why ?? 'Torrents can run with these settings.'}`);
}

export async function openSettings(tab = 'general') {
  current = await run(invoke('get_settings'), 'Couldn\'t read settings');
  if (!current) return;
  const f = form();
  f.downloadDir.value = current.downloadDir;
  f.categoryFolders.checked = current.categoryFolders;
  f.askForBrowserDownloads.checked = current.askForBrowserDownloads;
  f.closeToTray.checked = current.closeToTray;
  f.minimizeToTray.checked = current.minimizeToTray;
  f.launchOnStartup.checked = current.launchOnStartup;
  f.startMinimized.checked = current.startMinimized;
  f.startMinimized.disabled = !current.launchOnStartup;
  f.notifyOnComplete.checked = current.notifyOnComplete;
  f.openTorrentFiles.checked = current.openTorrentFiles;
  f.handleTorrentLinks.checked = current.handleTorrentLinks;
  fillSectionSelect($('set-sections'), current.defaultConnections);
  f.globalSpeedLimitKB.value = Math.round((current.globalSpeedLimit || 0) / 1024);
  const p = current.privacy;
  f.mode.value = modeName(p.mode);
  f.proxyUrl.value = p.mode?.url ?? '';
  await loadAdapters(p.mode?.interface ?? '');
  f.requireProtection.checked = p.requireProtection;
  f.dht.checked = p.dht;
  f.blocklist.value = p.blocklist ?? '';
  f.seedRatio.value = current.seedRatio;
  syncModeDetails();
  $('about-version').textContent = `Version ${info.version ?? ''}`;
  $('about-tools').replaceChildren(
    Object.assign(document.createElement('li'), { textContent: `Video downloader (yt-dlp): ${info.ytdlp ? 'installed' : 'missing'}` }),
    Object.assign(document.createElement('li'), { textContent: `Video joiner (ffmpeg): ${info.ffmpeg ? 'installed' : 'missing'}` }),
  );
  showError($('settings-error'), '');
  showTab(tab);
  refreshPrivacyStatus();
  $('dlg-settings').showModal();
}

function readSettings() {
  const f = form();
  const mode = f.mode.value;
  const privacyMode = mode === 'proxy' ? { type: 'proxy', url: f.proxyUrl.value.trim() }
    : mode === 'vpn' ? { type: 'vpn', interface: f.vpnInterface.value }
      : { type: mode };
  return {
    ...current,
    downloadDir: f.downloadDir.value.trim(),
    categoryFolders: f.categoryFolders.checked,
    askForBrowserDownloads: f.askForBrowserDownloads.checked,
    closeToTray: f.closeToTray.checked,
    minimizeToTray: f.minimizeToTray.checked,
    launchOnStartup: f.launchOnStartup.checked,
    startMinimized: f.startMinimized.checked,
    notifyOnComplete: f.notifyOnComplete.checked,
    openTorrentFiles: f.openTorrentFiles.checked,
    handleTorrentLinks: f.handleTorrentLinks.checked,
    defaultConnections: Number($('set-sections').value),
    globalSpeedLimit: Math.max(0, Number(f.globalSpeedLimitKB.value) || 0) * 1024,
    seedRatio: Math.max(0, Number(f.seedRatio.value) || 0),
    privacy: {
      mode: privacyMode,
      requireProtection: f.requireProtection.checked,
      dht: f.dht.checked,
      blocklist: f.blocklist.value.trim() || null,
    },
  };
}

async function saveSettings() {
  const s = readSettings();
  if (s.privacy.mode.type === 'proxy' && !s.privacy.mode.url) {
    showTab('privacy');
    return showError($('settings-error'), 'Enter your SOCKS5 proxy, or choose another mode.');
  }
  if (s.privacy.mode.type === 'vpn' && !s.privacy.mode.interface) {
    showTab('privacy');
    return showError($('settings-error'), 'Choose your VPN\'s network adapter, or choose another mode.');
  }
  try {
    await invoke('save_settings', { settings: s });
    $('dlg-settings').close();
    toast('Settings saved');
  } catch (e) {
    showError($('settings-error'), String(e?.message ?? e));
  }
}

// ---------------------------------------------------------------- wiring

export function initDialogs() {
  // Add
  $('add-check').addEventListener('click', check);
  $('add-url').addEventListener('change', check);
  $('add-browse').addEventListener('click', async () => {
    const dir = await pick({ directory: true, title: 'Save downloads to' });
    if (dir) $('add-dir').value = dir;
  });
  $('add-torrent').addEventListener('click', async () => {
    const file = await pick({ title: 'Open a .torrent file', filters: [{ name: 'Torrent files', extensions: ['torrent'] }] });
    if (!file) return;
    addState.torrentFile = file;
    $('add-url').value = '';
    infoBox([['Torrent file:', file.split(/[\\/]/).pop()], ['Privacy:', await privacyText()]]);
    $('add-sections').disabled = true;
  });
  $('add-form').addEventListener('submit', (e) => { e.preventDefault(); submitAdd(true); });
  $('add-later').addEventListener('click', () => submitAdd(false));

  // Browser
  $('browser-form').addEventListener('submit', (e) => { e.preventDefault(); submitBrowser(true); });
  $('browser-later').addEventListener('click', () => submitBrowser(false));
  $('browser-cancel').addEventListener('click', () => $('dlg-browser').close());
  listen('browser-download', openBrowser);

  // Remove
  $('dlg-remove').addEventListener('close', async () => {
    if ($('dlg-remove').returnValue !== 'remove') return;
    const scope = $('dlg-remove').querySelector('input[name="scope"]:checked')?.value ?? 'selected';
    const ids = (removeScopes[scope] ?? []).map((d) => d.id);
    if (!ids.length) return;
    await run(invoke('remove', { ids, deleteFiles: $('remove-files').checked }), 'Couldn\'t remove');
    toast(`Removed ${ids.length} download${ids.length === 1 ? '' : 's'}`);
  });

  // Queues
  $('queue-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const name = $('queue-name').value.trim();
    const maxActive = Number($('queue-max').value);
    if (editingQueue) await run(invoke('queue_update', { id: editingQueue.id, name, maxActive }), 'Couldn\'t rename the queue');
    else await run(invoke('queue_create', { name, maxActive }), 'Couldn\'t create the queue');
    $('dlg-queue').close();
  });
  $('queue-cancel').addEventListener('click', () => $('dlg-queue').close());
  for (const r of $('delqueue-form').elements.dq) {
    r.addEventListener('change', () => {
      const remove = $('delqueue-form').elements.dq.value === 'remove';
      $('delqueue-files').disabled = !remove;
      if (!remove) $('delqueue-files').checked = false;
    });
  }
  $('dlg-delqueue').addEventListener('close', async () => {
    if ($('dlg-delqueue').returnValue !== 'delete' || !deletingQueue) return;
    const removeDownloads = $('delqueue-form').elements.dq.value === 'remove';
    await run(invoke('queue_delete', { id: deletingQueue.id, removeDownloads, deleteFiles: removeDownloads && $('delqueue-files').checked }), 'Couldn\'t delete the queue');
    toast(`Deleted queue "${deletingQueue.name}"`);
  });

  // Settings
  for (const t of document.querySelectorAll('.tab')) t.addEventListener('click', () => showTab(t.dataset.tab));
  for (const r of form().mode) r.addEventListener('change', syncModeDetails);
  form().launchOnStartup.addEventListener('change', (e) => { form().startMinimized.disabled = !e.target.checked; });
  $('set-browse').addEventListener('click', async () => {
    const dir = await pick({ directory: true, title: 'Download folder' });
    if (dir) form().downloadDir.value = dir;
  });
  $('set-refresh-adapters').addEventListener('click', () => loadAdapters(form().vpnInterface.value));
  $('set-test-proxy').addEventListener('click', async () => {
    const url = form().proxyUrl.value.trim();
    const line = $('privacy-status');
    line.textContent = 'Testing the proxy…';
    try {
      await invoke('test_proxy', { url });
      line.replaceChildren(Object.assign(document.createElement('span'), { className: 'badge ok', textContent: 'Proxy works' }), ' It answered as a SOCKS5 proxy.');
    } catch (e) {
      line.replaceChildren(Object.assign(document.createElement('span'), { className: 'badge bad', textContent: 'Not reachable' }), ` ${e?.message ?? e}`);
    }
  });
  $('settings-form').addEventListener('submit', (e) => { e.preventDefault(); saveSettings(); });
  $('settings-cancel').addEventListener('click', () => $('dlg-settings').close());
  $('about-credit').addEventListener('click', () => run(invoke('open_portfolio'), 'Couldn\'t open the link'));
}
