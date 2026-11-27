// Dialogs: add a download, downloads sent by the browser, remove, and settings.

import { invoke, listen, pick } from './backend.js';
import { bytes } from './format.js';
import { icon } from './icons.js';
import { allDownloads, currentQueue, fillSectionSelect, info, queues, run, toast, viewDownloads } from './app.js';

const $ = (id) => document.getElementById(id);

/** A status word with its icon, in the same colours as the list. */
function badge(tone, ico, text) {
  const s = document.createElement('span');
  s.className = `status tone-${tone} changed`;
  s.append(icon(ico), Object.assign(document.createElement('span'), { textContent: text }));
  return s;
}

function showError(el, text) {
  el.textContent = text ?? '';
  el.hidden = !text;
}

const isMagnet = (u) => /^magnet:\?/i.test(u);
const looksLikeFile = (u) => /\.[a-z0-9]{2,5}(?:[?#]|$)/i.test(new URL(u, 'https://x').pathname);

// ---------------------------------------------------------------- folder fields

/** The × in a folder field shows only when a folder was chosen (empty = automatic). */
function syncPathfields() {
  for (const id of ['add-dir', 'browser-dir']) $(`${id}-clear`).hidden = !$(id).value.trim();
}

/** Says where "automatic" saves; the full folder is in the tooltip. */
function autoPlaceholder(s) {
  if (!s?.downloadDir) return;
  const sep = s.downloadDir.includes('\\') ? '\\' : '/';
  const last = s.downloadDir.split(sep).filter(Boolean).pop() ?? s.downloadDir;
  const text = s.categoryFolders ? 'Automatic, by type (Video, Music, Documents…)' : `Automatic: …${sep}${last}`;
  for (const id of ['add-dir', 'browser-dir']) {
    $(id).placeholder = text;
    $(id).title = s.categoryFolders ? `${s.downloadDir}${sep}<type of file>` : s.downloadDir;
  }
}

async function chooseFolder(input, title) {
  const dir = await pick({ directory: true, title });
  if (dir) {
    input.value = dir;
    input.dispatchEvent(new Event('input', { bubbles: true }));
  }
}

// ---------------------------------------------------------------- folders for each type of file

const TYPE_FOLDERS = [
  ['video', 'Video'], ['music', 'Music'], ['documents', 'Documents'], ['compressed', 'Compressed'],
  ['programs', 'Programs'], ['torrents', 'Torrents'], ['general', 'General'],
];

/** One folder field per type: empty means the default (<download folder>\<Type>). */
function fillTypeFolders() {
  const box = $('type-folders');
  const chosen = current?.categoryDirs ?? {};
  box.replaceChildren(...TYPE_FOLDERS.map(([key, name]) => {
    const row = document.createElement('div');
    row.className = 'type-folder';
    const label = document.createElement('span');
    label.className = 'type-folder-name';
    label.append(icon(key === 'general' ? 'general' : key), name);
    const field = document.createElement('div');
    field.className = 'pathfield';
    const input = Object.assign(document.createElement('input'), { type: 'text', value: chosen[key] ?? '', spellcheck: false });
    input.dataset.category = key;
    input.setAttribute('aria-label', `Folder for ${name.toLowerCase()}`);
    const clear = Object.assign(document.createElement('button'), { type: 'button', className: 'pathfield-clear', title: 'Back to the default folder' });
    clear.append(icon('x'));
    const choose = Object.assign(document.createElement('button'), { type: 'button', className: 'pathfield-btn' });
    choose.append(icon('folder-open'), 'Choose…');
    const sync = () => { clear.hidden = !input.value.trim(); };
    input.addEventListener('input', sync);
    clear.addEventListener('click', () => { input.value = ''; sync(); input.focus(); });
    choose.addEventListener('click', () => chooseFolder(input, `Folder for ${name.toLowerCase()}`).then(sync));
    sync();
    field.append(input, clear, choose);
    row.append(label, field);
    return row;
  }));
  syncTypePlaceholders();
}

// ---------------------------------------------------------------- file extensions of each type

const EXT_TYPES = [['video', 'Video'], ['music', 'Music'], ['documents', 'Documents'], ['compressed', 'Compressed'], ['programs', 'Programs']];
let extDefaults = null;

const parseExts = (text) => [...new Set(text.split(/[\s,;]+/).map((e) => e.trim().replace(/^\./, '').toLowerCase()).filter((e) => /^[a-z0-9]+$/.test(e)))];
const sameList = (a, b) => a.length === b.length && a.every((x) => b.includes(x));

/** One editable list per type, filled with the user's list or the built-in one. */
async function fillTypeExtensions() {
  extDefaults ??= await invoke('category_defaults').catch(() => ({}));
  const box = $('type-exts');
  const chosen = current?.categoryExtensions ?? {};
  box.replaceChildren(...EXT_TYPES.map(([key, name]) => {
    const row = document.createElement('div');
    row.className = 'type-ext';
    const label = document.createElement('label');
    label.className = 'type-folder-name';
    label.htmlFor = `exts-${key}`;
    label.append(icon(key), name);
    const input = Object.assign(document.createElement('textarea'), { id: `exts-${key}`, rows: 2, spellcheck: false });
    input.dataset.category = key;
    input.value = (chosen[key] ?? extDefaults[key] ?? []).join(' ');
    const reset = Object.assign(document.createElement('button'), { type: 'button', className: 'btn small subtle', textContent: 'Reset' });
    reset.title = `Back to the built-in list for ${name.toLowerCase()}`;
    const sync = () => { reset.disabled = sameList(parseExts(input.value), extDefaults[key] ?? []); };
    input.addEventListener('input', sync);
    reset.addEventListener('click', () => { input.value = (extDefaults[key] ?? []).join(' '); sync(); input.focus(); });
    sync();
    row.append(label, input, reset);
    return row;
  }));
}

/** Only the lists that differ from the built-in ones are saved. */
function readTypeExtensions() {
  const out = {};
  for (const input of document.querySelectorAll('#type-exts textarea')) {
    const list = parseExts(input.value);
    if (!sameList(list, extDefaults?.[input.dataset.category] ?? [])) out[input.dataset.category] = list;
  }
  return out;
}

/** The default folder each type uses, shown when no folder is chosen. */
function syncTypePlaceholders() {
  const f = form();
  const dir = f.downloadDir.value.trim();
  const sep = dir.includes('\\') ? '\\' : '/';
  for (const input of document.querySelectorAll('#type-folders input')) {
    const name = TYPE_FOLDERS.find(([k]) => k === input.dataset.category)[1];
    input.placeholder = f.categoryFolders.checked ? `${dir}${sep}${name}` : dir;
  }
}

// ---------------------------------------------------------------- refresh an expired link

let refreshing = null;

/** "Refresh link": tries the same link, else opens the download's page in the browser and
 *  waits for the file to be downloaded there again, then continues with that fresh link. */
export async function openRefresh(d) {
  refreshing = d;
  const pct = d.size ? Math.floor((d.downloaded / d.size) * 100) : null;
  $('refresh-name').textContent = d.fileName;
  $('refresh-name').title = d.fileName;
  $('refresh-pct').textContent = pct != null ? `${pct}%` : 'where it stopped';
  $('refresh-checking').hidden = false;
  $('refresh-waiting').hidden = true;
  $('refresh-paste').open = false;
  $('refresh-url').value = '';
  showError($('refresh-error'), '');
  if (!$('dlg-refresh').open) $('dlg-refresh').showModal();
  try {
    const r = await invoke('refresh_link', { id: d.id, open: true });
    if (refreshing?.id !== d.id) return;
    if (r.resumed) {
      closeRefresh();
      toast(`The link works again. Continuing ${d.fileName}`);
      return;
    }
    let host = r.pageUrl;
    try { host = new URL(r.pageUrl).host; } catch { /* keep it */ }
    $('refresh-page').textContent = host;
    $('refresh-page').title = r.pageUrl;
    $('refresh-reopen').dataset.url = r.pageUrl;
    $('refresh-checking').hidden = true;
    $('refresh-waiting').hidden = false;
  } catch (e) {
    $('refresh-checking').hidden = true;
    showError($('refresh-error'), `Couldn't refresh it: ${e?.message ?? e}`);
  }
}

/** Closing the dialog doesn't stop the wait: download the file in the browser within
 *  15 minutes and it still continues here. */
function closeRefresh() {
  refreshing = null;
  if ($('dlg-refresh').open) $('dlg-refresh').close();
}

// ---------------------------------------------------------------- add

let addState = { media: null, torrentFile: null, bg: null, bgUrl: '', dup: null };

/** The download started in the background for the Add window is no longer wanted. */
function dropBackground() {
  if (addState.bg != null) invoke('discard_download', { id: addState.bg }).catch(() => {});
  addState.bg = null;
  addState.bgUrl = '';
}

/** A checked file link: if it was downloaded before, say so; otherwise start it in the
 *  background (as IDM does) so it's already downloading when you press Start. */
async function startEarly(url) {
  if (addState.bgUrl === url) return;
  dropBackground();
  showDuplicate(null);
  const dup = await invoke('find_duplicate', { url }).catch(() => null);
  if ($('add-url').value.trim() !== url) return;
  if (dup) return showDuplicate(dup);
  try {
    addState.bg = await invoke('start_background', { url });
    addState.bgUrl = url;
    $('add-info').append(Object.assign(document.createElement('div'), { className: 'muted small early', textContent: 'Already downloading in the background while you choose.' }));
  } catch { /* it starts when you press Start */ }
}

const DONE_STATES = ['completed', 'seeding'];
function showDuplicate(d) {
  addState.dup = d;
  $('add-dup').hidden = !d;
  if (d) {
    const done = DONE_STATES.includes(d.status);
    $('add-dup-title').textContent = done ? 'You\'ve downloaded this before' : 'This is already in your list';
    $('add-dup-text').textContent = `${d.fileName} · ${done ? 'finished' : 'not finished'}. Download it again to get a second copy with a numbered name.`;
    $('add-dup-use').textContent = done ? 'Open it' : 'Continue it';
  }
  syncAddButtons();
}

export function openAdd({ url = '' } = {}) {
  dropBackground();
  addState = { media: null, torrentFile: null, bg: null, bgUrl: '', dup: null };
  showDuplicate(null);
  $('add-form').reset();
  $('add-url').value = url;
  $('add-info').hidden = true;
  $('add-formats').hidden = true;
  showError($('add-error'), '');
  run(invoke('get_settings'), 'Couldn\'t read settings').then((s) => {
    fillSectionSelect($('add-sections'), s?.defaultConnections ?? 8);
    autoPlaceholder(s);
  });
  syncPathfields();
  $('add-queue').replaceChildren(
    Object.assign(document.createElement('option'), { value: '', textContent: 'No queue (start now)' }),
    ...queues.map((q) => Object.assign(document.createElement('option'), { value: String(q.id), textContent: `${q.name}${q.running ? ' (running)' : ''}` })),
  );
  // Adding while looking at a queue puts it in that queue.
  $('add-queue').value = currentQueue() ? String(currentQueue().id) : '';
  syncAddButtons();
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
  $('add-check').classList.add('busy');
  $('add-check').setAttribute('aria-label', 'Checking');
  try {
    if (looksLikeFile(url)) {
      const p = await invoke('probe', { url });
      showFile(p);
      await startEarly(url);
    } else {
      // A web page: maybe a video. Ask for formats, fall back to a plain download.
      try {
        const f = await invoke('media_formats', { url });
        showFormats(url, f);
      } catch {
        showFile(await invoke('probe', { url }));
        await startEarly(url);
      }
    }
  } catch (e) {
    showError($('add-error'), `Couldn't check the address: ${e?.message ?? e}`);
  } finally {
    $('add-check').disabled = false;
    $('add-check').classList.remove('busy');
    $('add-check').removeAttribute('aria-label');
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
    if (addState.bg != null && addState.bgUrl === url && !args.media && !args.torrentFile) {
      // Already downloading: confirm it with what was chosen.
      const id = addState.bg;
      addState.bg = null;
      await invoke('confirm_download', { id, args: { fileName: args.fileName, saveDir: args.saveDir, connections: args.connections, queueId: args.queueId, start } });
    } else {
      await invoke('add', { args });
    }
    $('dlg-add').close();
    toast(args.queueId ? `Added to ${queues.find((q) => q.id === args.queueId)?.name}` : start ? 'Download started' : 'Added to the list');
  } catch (e) {
    showError($('add-error'), String(e?.message ?? e));
  }
}

// ---------------------------------------------------------------- from the browser

let browserItems = [];

function syncBrowserCount() {
  const boxes = [...$('browser-items').querySelectorAll('input')];
  const n = boxes.filter((b) => b.checked).length;
  const note = $('browser-count').dataset.note;
  $('browser-count').textContent = `${n} of ${boxes.length} selected${note ? ` · ${note}` : ''}`;
  $('browser-toggle').textContent = n === boxes.length ? 'Select none' : 'Select all';
  $('browser-start').disabled = n === 0;
  $('browser-later').disabled = n === 0;
}

let browserQueue = null;
/** Per item: the download already started in the background, if any. */
let browserIds = [];
let browserAnswered = false;

function openBrowser({ items, source, title, note, queueId = null, ids = [] }) {
  browserItems = items;
  browserIds = ids;
  browserAnswered = false;
  browserQueue = queueId;
  const several = items.length > 1;
  $('browser-title').textContent = title ?? (several ? `Download ${items.length} files from your browser` : 'Download from your browser');
  $('browser-count').dataset.note = note ?? '';
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
  syncBrowserCount();
  $('browser-dir').value = '';
  syncPathfields();
  invoke('get_settings').then(autoPlaceholder).catch(() => {});
  void source;
  $('dlg-browser').showModal();
}

async function submitBrowser(start) {
  browserAnswered = true;
  const boxes = [...$('browser-items').querySelectorAll('input')];
  // Not picked, but already started in the background: those go away.
  for (const c of boxes.filter((b) => !b.checked)) {
    const id = browserIds[Number(c.dataset.index)];
    if (id != null) invoke('discard_download', { id }).catch(() => {});
  }
  for (const c of boxes.filter((b) => b.checked)) {
    const i = Number(c.dataset.index);
    const it = browserItems[i];
    if (browserIds[i] != null) {
      await run(invoke('confirm_download', {
        id: browserIds[i],
        args: { saveDir: $('browser-dir').value.trim() || null, connections: Number($('browser-sections').value) || null, queueId: browserQueue, start },
      }), 'Couldn\'t start a download');
      continue;
    }
    await run(invoke('add', {
      args: {
        url: it.url, fileName: it.fileName ?? null, referer: it.referer ?? null, cookies: it.cookies ?? null,
        userAgent: it.userAgent ?? null, description: it.description ?? null, media: it.media ?? null,
        saveDir: $('browser-dir').value.trim() || null, connections: Number($('browser-sections').value) || null, start,
        queueId: browserQueue,
      },
    }), 'Couldn\'t add a download');
  }
  $('dlg-browser').close();
  const picked = boxes.filter((b) => b.checked).length;
  if (picked) toast(`${picked} download${picked > 1 ? 's' : ''} ${start ? 'started' : 'added'}`);
}

// ---------------------------------------------------------------- remove

let removeScopes = {};

/** Remove downloads: the selection, everything finished, everything failed, or the whole view. */
export function openRemove(selection, preset = null) {
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
  const first = [preset, 'selected', 'finished', 'failed', 'view'].find((k) => k && removeScopes[k].length);
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
  line.replaceChildren(why ? badge('held', 'vpn', 'Torrents waiting') : badge('good', 'done', 'Protected'),
    ` ${why ?? 'Torrents can run with these settings.'}`);
}

export async function openSettings(tab = 'general') {
  current = await run(invoke('get_settings'), 'Couldn\'t read settings');
  if (!current) return;
  const f = form();
  f.downloadDir.value = current.downloadDir;
  f.duplicates.value = current.duplicates ?? 'ask';
  fillTypeFolders();
  await fillTypeExtensions();
  f.categoryFolders.checked = current.categoryFolders;
  syncTypePlaceholders();
  f.askForBrowserDownloads.checked = current.askForBrowserDownloads;
  f.closeToTray.checked = current.closeToTray;
  f.minimizeToTray.checked = current.minimizeToTray;
  f.launchOnStartup.checked = current.launchOnStartup;
  f.startMinimized.checked = current.startMinimized;
  f.startMinimized.disabled = !current.launchOnStartup;
  f.notifyOnComplete.checked = current.notifyOnComplete;
  f.showProgressWindow.checked = current.showProgressWindow ?? true;
  f.showCompleteWindow.checked = current.showCompleteWindow ?? true;
  f.showMiniWindow.checked = current.showMiniWindow ?? true;
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
    duplicates: f.duplicates.value,
    categoryExtensions: readTypeExtensions(),
    categoryDirs: Object.fromEntries([...document.querySelectorAll('#type-folders input')]
      .map((i) => [i.dataset.category, i.value.trim()]).filter(([, v]) => v)),
    categoryFolders: f.categoryFolders.checked,
    askForBrowserDownloads: f.askForBrowserDownloads.checked,
    closeToTray: f.closeToTray.checked,
    minimizeToTray: f.minimizeToTray.checked,
    launchOnStartup: f.launchOnStartup.checked,
    startMinimized: f.startMinimized.checked,
    notifyOnComplete: f.notifyOnComplete.checked,
    showProgressWindow: f.showProgressWindow.checked,
    showCompleteWindow: f.showCompleteWindow.checked,
    showMiniWindow: f.showMiniWindow.checked,
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

/** Marks dialogs whose content is taller than the window (they show dividers). */
function watchDialogScrolling() {
  const check = (form) => form.classList.toggle('scrolls', form.scrollHeight > form.clientHeight + 1);
  const ro = new ResizeObserver((entries) => {
    for (const e of entries) {
      const form = e.target.closest('.dialog');
      if (form) check(form);
    }
  });
  for (const form of document.querySelectorAll('dialog > .dialog')) {
    ro.observe(form);
    for (const child of form.children) ro.observe(child);
    new MutationObserver(() => check(form)).observe(form, { childList: true, subtree: true, attributes: true, attributeFilter: ['hidden', 'open'] });
  }
  addEventListener('resize', () => document.querySelectorAll('dialog[open] > .dialog').forEach(check));
}

export function initDialogs() {
  watchDialogScrolling();
  // Add
  $('add-check').addEventListener('click', check);
  $('add-url').addEventListener('change', check);
  // Folder fields: Choose… opens the folder picker; × goes back to automatic.
  $('add-browse').addEventListener('click', () => chooseFolder($('add-dir'), 'Save this download in'));
  $('browser-browse').addEventListener('click', () => chooseFolder($('browser-dir'), 'Save these downloads in'));
  for (const id of ['add-dir', 'browser-dir']) {
    $(id).addEventListener('input', syncPathfields);
    $(`${id}-clear`).addEventListener('click', () => { $(id).value = ''; syncPathfields(); $(id).focus(); });
  }

  // Refresh an expired link.
  $('refresh-cancel').addEventListener('click', () => closeRefresh());
  $('dlg-refresh').addEventListener('close', () => { refreshing = null; });
  $('refresh-reopen').addEventListener('click', () => run(invoke('open_page', { url: $('refresh-reopen').dataset.url }), 'Couldn\'t open the page'));
  $('refresh-form').addEventListener('submit', (e) => e.preventDefault());
  $('refresh-url').addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); $('refresh-use').click(); } });
  $('refresh-use').addEventListener('click', async () => {
    const url = $('refresh-url').value.trim();
    if (!/^https?:\/\//i.test(url)) return showError($('refresh-error'), 'Paste a link that starts with http:// or https://.');
    const d = refreshing;
    if (!d) return;
    try {
      await invoke('refresh_address', { id: d.id, url });
      closeRefresh();
      toast(`New link set. Continuing ${d.fileName}`);
    } catch (e) {
      showError($('refresh-error'), `That link didn't work: ${e?.message ?? e}`);
    }
  });
  listen('link-refreshed', (r) => {
    if (refreshing?.id === r.id) closeRefresh();
    toast(`Got a fresh link. Continuing ${r.fileName}`);
  });
  $('add-torrent').addEventListener('click', async () => {
    const file = await pick({ title: 'Open a .torrent file', filters: [{ name: 'Torrent files', extensions: ['torrent'] }] });
    if (!file) return;
    addState.torrentFile = file;
    $('add-url').value = '';
    infoBox([['Torrent file:', file.split(/[\\/]/).pop()], ['Privacy:', await privacyText()]]);
    $('add-sections').disabled = true;
  });
  // Enter starts the download, or adds it to the chosen queue.
  $('add-form').addEventListener('submit', (e) => { e.preventDefault(); submitAdd(!$('add-queue').value); });
  $('add-later').addEventListener('click', () => submitAdd(false));

  // Add: label follows the queue picker
  $('add-queue').addEventListener('change', syncAddButtons);

  // Schedule
  $('schedule-form').addEventListener('input', syncSchedule);
  $('schedule-form').addEventListener('change', syncSchedule);
  $('schedule-form').addEventListener('submit', (e) => { e.preventDefault(); saveSchedule(); });
  $('sch-cancel').addEventListener('click', () => $('dlg-schedule').close());

  // Grab a site
  $('grab-form').addEventListener('submit', (e) => { e.preventDefault(); runGrab(); });
  $('grab-cancel').addEventListener('click', () => $('dlg-grab').close());

  // Every dialog closes on a click outside it (on the dimmed backdrop).
  for (const d of document.querySelectorAll('dialog')) {
    d.addEventListener('mousedown', (e) => { if (e.target === d) d.close('cancel'); });
  }

  // Browser
  $('browser-items').addEventListener('change', syncBrowserCount);
  $('browser-toggle').addEventListener('click', () => {
    const boxes = [...$('browser-items').querySelectorAll('input')];
    const all = boxes.every((b) => b.checked);
    for (const b of boxes) b.checked = !all;
    syncBrowserCount();
  });
  $('browser-form').addEventListener('submit', (e) => { e.preventDefault(); submitBrowser(true); });
  $('browser-later').addEventListener('click', () => submitBrowser(false));
  $('browser-cancel').addEventListener('click', () => $('dlg-browser').close());
  // Closed without an answer: downloads started in the background go away.
  $('dlg-browser').addEventListener('close', () => {
    if (browserAnswered) return;
    for (const id of browserIds) if (id != null) invoke('discard_download', { id }).catch(() => {});
    browserIds = [];
  });
  $('dlg-add').addEventListener('close', dropBackground);
  $('add-url').addEventListener('input', () => {
    if (addState.bgUrl && $('add-url').value.trim() !== addState.bgUrl) dropBackground();
    if (addState.dup) showDuplicate(null);
  });
  $('add-dup-use').addEventListener('click', async () => {
    const d = addState.dup;
    if (!d) return;
    if (DONE_STATES.includes(d.status)) await run(invoke('open_download', { id: d.id }), 'Couldn\'t open it');
    else await run(invoke('resume', { ids: [d.id] }), 'Couldn\'t continue it');
    $('dlg-add').close();
  });
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
  form().downloadDir.addEventListener('input', syncTypePlaceholders);
  form().categoryFolders.addEventListener('change', syncTypePlaceholders);
  $('set-browse').addEventListener('click', async () => {
    const dir = await pick({ directory: true, title: 'Download folder' });
    if (dir) { form().downloadDir.value = dir; syncTypePlaceholders(); }
  });
  $('set-refresh-adapters').addEventListener('click', () => loadAdapters(form().vpnInterface.value));
  $('set-test-proxy').addEventListener('click', async () => {
    const url = form().proxyUrl.value.trim();
    const line = $('privacy-status');
    line.textContent = 'Testing the proxy…';
    try {
      await invoke('test_proxy', { url });
      line.replaceChildren(badge('good', 'done', 'Proxy works'), ' It answered as a SOCKS5 proxy.');
    } catch (e) {
      line.replaceChildren(badge('bad', 'failed', 'Not reachable'), ` ${e?.message ?? e}`);
    }
  });
  $('settings-form').addEventListener('submit', (e) => { e.preventDefault(); saveSettings(); });
  $('settings-cancel').addEventListener('click', () => $('dlg-settings').close());
  $('about-credit').addEventListener('click', () => run(invoke('open_portfolio'), 'Couldn\'t open the link'));
}

/** "Add to list" or "Add to queue", depending on the queue picked. */
function syncAddButtons() {
  const inQueue = !!$('add-queue').value;
  $('add-start').textContent = addState.dup ? 'Download again' : 'Start download';
  $('add-later').textContent = inQueue ? 'Add to queue' : 'Add to list';
  $('add-start').hidden = inQueue;
  // In a queue, "Add to queue" is the only way forward, so it's the main button.
  $('add-later').classList.toggle('primary', inQueue);
}

// ---------------------------------------------------------------- schedule

const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
let schedulingQueue = null;

function scheduleFromForm() {
  const days = [...$('sch-days').querySelectorAll('input:checked')].map((c) => Number(c.value));
  return {
    start: $('sch-start').value || null,
    stop: $('sch-stop').value || null,
    days: days.length === 7 ? [] : days,
    whenDone: $('sch-done').value,
  };
}

function describeSchedule(sch) {
  if (!$('sch-on').checked) return 'Runs only when you start it.';
  const days = sch.days.length === 0 ? 'every day'
    : sch.days.join() === '0,1,2,3,4' ? 'on weekdays'
      : sch.days.join() === '5,6' ? 'at weekends'
        : `on ${sch.days.map((d) => DAYS[d]).join(', ')}`;
  const done = { nothing: '', quit: ' When it\'s done, the app closes.', sleep: ' When it\'s done, the computer sleeps.', shutDown: ' When it\'s done, the computer shuts down after a minute.' }[sch.whenDone];
  if (!sch.start) return `Not scheduled to start.${done}`;
  return `Starts at ${sch.start} ${days}${sch.stop ? `, stops at ${sch.stop}` : ''}.${done}`;
}

function syncSchedule() {
  const on = $('sch-on').checked;
  for (const el of $('schedule-form').querySelectorAll('#sch-start, #sch-stop, #sch-days input, #sch-done')) el.disabled = !on;
  $('sch-summary').textContent = describeSchedule(scheduleFromForm());
}

export function openSchedule(q) {
  schedulingQueue = q;
  const s = q.schedule;
  $('schedule-title').textContent = `Schedule "${q.name}"`;
  $('sch-on').checked = !!s;
  $('sch-start').value = s?.start ?? '22:00';
  $('sch-stop').value = s?.stop ?? '';
  $('sch-done').value = s?.whenDone ?? 'nothing';
  $('sch-days').replaceChildren(...DAYS.map((d, i) => {
    const l = document.createElement('label');
    const c = Object.assign(document.createElement('input'), { type: 'checkbox', value: String(i) });
    c.checked = !s || s.days.length === 0 || s.days.includes(i);
    c.setAttribute('aria-label', ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'][i]);
    l.append(c, Object.assign(document.createElement('span'), { textContent: d }));
    return l;
  }));
  showError($('sch-error'), '');
  syncSchedule();
  $('dlg-schedule').showModal();
}

async function saveSchedule() {
  const sch = scheduleFromForm();
  if ($('sch-on').checked && !sch.start && !sch.stop) return showError($('sch-error'), 'Choose a start or stop time.');
  if ($('sch-on').checked && $('sch-days').querySelectorAll('input:checked').length === 0) return showError($('sch-error'), 'Choose at least one day.');
  try {
    await invoke('queue_schedule', { id: schedulingQueue.id, schedule: $('sch-on').checked ? sch : null });
    $('dlg-schedule').close();
    toast($('sch-on').checked ? describeSchedule(sch) : 'Schedule turned off');
  } catch (e) {
    showError($('sch-error'), String(e?.message ?? e));
  }
}

// ---------------------------------------------------------------- grab a site

export function openGrab() {
  $('grab-form').reset();
  $('grab-status').hidden = true;
  showError($('grab-error'), '');
  $('dlg-grab').showModal();
  $('grab-url').focus();
}

async function runGrab() {
  const url = $('grab-url').value.trim();
  const types = $('grab-types').value.split(/[\s,;]+/).map((t) => t.replace(/^\./, '')).filter(Boolean);
  if (!url) return showError($('grab-error'), 'Enter the page to start from.');
  if (!types.length) return showError($('grab-error'), 'Enter at least one file type.');
  showError($('grab-error'), '');
  $('grab-go').disabled = true;
  $('grab-go').classList.add('busy');
  $('grab-status').hidden = false;
  $('grab-status').textContent = 'Reading the site…';
  try {
    const r = await invoke('grab_site', { options: { startUrl: url, depth: Number($('grab-depth').value), fileTypes: types, sameSite: $('grab-same').checked, maxPages: 150 } });
    if (!r.files.length) {
      $('grab-status').textContent = `No ${types.join(', ')} files found on ${r.pagesRead} page${r.pagesRead === 1 ? '' : 's'}.`;
      return;
    }
    $('dlg-grab').close();
    const note = [`${r.pagesRead} page${r.pagesRead === 1 ? '' : 's'} read`];
    if (r.pagesSkippedByRobots) note.push(`${r.pagesSkippedByRobots} left out at the site's request`);
    if (r.stoppedEarly) note.push('stopped at 150 pages');
    openBrowser({
      items: r.files.map((f) => ({ url: f.url, fileName: f.fileName, referer: f.pageUrl })),
      source: 'allLinks',
      title: `${r.files.length} file${r.files.length === 1 ? '' : 's'} found`,
      note: note.join(' · '),
      // Grabbing from a queue's view adds the files to that queue, like Add does.
      queueId: currentQueue()?.id ?? null,
    });
  } catch (e) {
    showError($('grab-error'), String(e?.message ?? e));
    $('grab-status').hidden = true;
  } finally {
    $('grab-go').disabled = false;
    $('grab-go').classList.remove('busy');
  }
}
