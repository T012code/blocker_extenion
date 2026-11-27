// A pretend backend for trying the window in a browser (open index.html?mock).
// Downloads progress in sections like the real engine; nothing touches the network.

const listeners = new Map();
const downloads = new Map();
let nextId = 1;
let queues = [];
const emitQueues = () => emit('queues', queues);
function fillQueues() {
  for (const q of queues.filter((x) => x.running)) {
    const mine = [...downloads.values()].filter((d) => d.queueId === q.id);
    let free = q.maxActive - mine.filter((d) => d.status === 'downloading').length;
    for (const d of mine.filter((x) => x.status === 'queued').sort((a, b) => a.id - b.id)) {
      if (free-- <= 0) break;
      update(d, { status: 'downloading' });
    }
  }
}
let settings = {
  downloadDir: 'C:\\Users\\Layo\\Downloads\\Layó\'s Download Manager',
  categoryFolders: true,
  categoryDirs: {},
  duplicates: 'ask',
  defaultConnections: 8,
  globalSpeedLimit: 0,
  askForBrowserDownloads: false,
  closeToTray: true,
  minimizeToTray: false,
  launchOnStartup: true,
  startMinimized: true,
  notifyOnComplete: true,
  showProgressWindow: true,
  showCompleteWindow: true,
  showMiniWindow: true,
  openTorrentFiles: true,
  handleTorrentLinks: true,
  seedRatio: 1,
  privacy: { mode: { type: 'private' }, requireProtection: true, dht: true, blocklist: null },
};

function emit(event, payload) {
  for (const h of listeners.get(event) ?? []) h(structuredClone(payload));
}

function category(name) {
  const ext = name.split('.').pop().toLowerCase();
  if (['mp4', 'mkv', 'webm', 'mov'].includes(ext)) return 'video';
  if (['mp3', 'flac', 'm4a'].includes(ext)) return 'music';
  if (['zip', 'rar', '7z', 'iso'].includes(ext)) return 'compressed';
  if (['pdf', 'docx', 'epub'].includes(ext)) return 'documents';
  if (['exe', 'msi'].includes(ext)) return 'programs';
  return 'general';
}

const protectedNow = () => {
  const p = settings.privacy;
  return p.mode.type === 'proxy' || p.mode.type === 'vpn' || !p.requireProtection;
};

function make({ url, fileName, connections, start, size, kind = 'http', status }) {
  const id = nextId++;
  const name = fileName || decodeURIComponent(url.split('/').pop().split('?')[0]) || 'download';
  const d = {
    id, request: { url }, fileName: name, saveDir: `${settings.downloadDir}\\${category(name)}`,
    category: category(name), size: size ?? 48 * 1024 * 1024, resumable: kind === 'http', status: status ?? (start ? 'downloading' : 'queued'),
    error: null, downloaded: 0, segments: [], connections: connections || settings.defaultConnections,
    kind: { type: kind }, createdAt: Date.now(), queueId: null,
  };
  downloads.set(id, d);
  emit('engine', { type: 'added', download: d });
  return d;
}

/** Splits the file into sections and lets idle ones take over halves of slow ones. */
function plan(d) {
  const n = d.connections;
  const part = Math.floor(d.size / n);
  d.live = Array.from({ length: n }, (_, k) => ({
    start: k * part, end: k === n - 1 ? d.size : (k + 1) * part, pos: k * part, active: true,
    rate: 60_000 + Math.random() * 90_000,
  }));
}

function tick() {
  fillQueues();
  for (const d of downloads.values()) {
    if (d.status === 'blocked' && protectedNow()) update(d, { status: 'downloading', error: null });
    if (d.status !== 'downloading') continue;
    if (d.kind.type === 'torrent' && !protectedNow()) {
      update(d, { status: 'blocked', error: 'Waiting for a VPN or proxy. Set one up in Settings → Privacy, or allow torrents without one.' });
      continue;
    }
    if (!d.live) plan(d);
    for (const s of d.live) {
      if (!s.active) continue;
      s.pos = Math.min(s.end, s.pos + s.rate);
      if (s.pos >= s.end) {
        s.active = false;
        // Take over half of the section with the most left.
        const big = d.live.filter((x) => x.active).sort((a, b) => (b.end - b.pos) - (a.end - a.pos))[0];
        if (big && big.end - big.pos > 400_000) {
          const mid = big.pos + Math.floor((big.end - big.pos) / 2);
          d.live.push({ start: mid, end: big.end, pos: mid, active: true, rate: s.rate });
          big.end = mid;
        }
      }
    }
    const done = d.live.reduce((n, s) => n + (s.pos - s.start), 0);
    const speed = d.live.filter((s) => s.active).reduce((n, s) => n + s.rate, 0) * 4;
    d.downloaded = done;
    emit('engine', { type: 'progress', id: d.id, downloaded: done, size: d.size, speed, segments: d.live.map(({ start, end, pos, active }) => ({ start, end, pos, active })) });
    if (done >= d.size) {
      delete d.live;
      update(d, { status: 'completed', downloaded: d.size });
    }
  }
}
setInterval(tick, 250);

function update(d, patch) {
  Object.assign(d, patch);
  emit('engine', { type: 'updated', download: { ...d, live: undefined } });
}

// A few downloads to start with.
const seed = () => {
  make({ url: 'https://releases.example.org/ubuntu-24.04-desktop-amd64.iso', start: true, size: 5.8 * 1024 ** 3 / 40, connections: 16 });
  const v = make({ url: 'https://cdn.example.com/talks/keynote-2026.mp4', size: 320 * 1024 * 1024 });
  update(v, { status: 'paused', downloaded: 118 * 1024 * 1024 });
  const t = make({ url: 'magnet:?xt=urn:btih:0123&dn=Big+Buck+Bunny+1080p', fileName: 'Big Buck Bunny 1080p', kind: 'torrent', start: true, size: 690 * 1024 * 1024 });
  void t;
  const f = make({ url: 'https://files.example.net/report-q3.pdf', size: 3.2 * 1024 * 1024 });
  update(f, { status: 'completed', downloaded: f.size });
  const e = make({ url: 'https://dl.example.com/old/setup.exe?token=a1', size: 92 * 1024 * 1024 });
  e.request.referer = 'https://dl.example.com/downloads/';
  update(e, { status: 'needsRefresh', downloaded: 40 * 1024 * 1024, error: 'the server refused the link (HTTP 403); it may have expired' });
};
seed();
// ?pending: a download from the browser, started in the background, waiting for "Start" (id 6).
if (new URLSearchParams(location.search).has('pending')) {
  const p = make({ url: 'https://cdn.example.com/installers/studio-setup-2.4.exe', start: true, size: 180 * 1024 * 1024, connections: 8 });
  p.pending = true;
}
// ?many=N adds N finished and waiting downloads, to try a long list.
const many = Number(new URLSearchParams(location.search).get('many')) || 0;
for (let i = 0; i < many; i++) {
  const names = ['holiday-photos', 'podcast-episode', 'invoice', 'dataset', 'firmware', 'lecture', 'album-track', 'backup'];
  const exts = ['zip', 'mp3', 'pdf', 'csv', 'bin', 'mp4', 'flac', '7z'];
  const k = i % names.length;
  const d = make({ url: `https://files.example.org/${names[k]}-${String(i + 1).padStart(3, '0')}.${exts[k]}`, size: (5 + ((i * 37) % 900)) * 1024 * 1024 });
  d.createdAt = Date.now() - (many - i) * 3_600_000;
  if (i % 3 === 0) update(d, { status: 'completed', downloaded: d.size, completedAt: d.createdAt + 600_000 });
  else if (i % 3 === 1) update(d, { status: 'paused', downloaded: Math.floor(d.size * ((i * 13) % 97) / 100) });
}

/** For tests: calls that would change windows, which a browser can't show. */
window.__ldmCalls = [];

export async function invoke(cmd, args) {
  await new Promise((r) => setTimeout(r, 30));
  switch (cmd) {
    case 'app_info': return { version: '0.2.0', ytdlp: true, ffmpeg: true, maxConnections: 64, sectionPresets: [1, 2, 4, 8, 16, 24, 32, 48, 64] };
    case 'list': return [...downloads.values()].map((d) => ({ ...d, live: undefined }));
    case 'add': {
      const a = args.args;
      const kind = /^magnet:/.test(a.url) || a.torrentFile ? 'torrent' : a.media ? 'media' : 'http';
      const d = make({ url: a.url || a.torrentFile, fileName: a.fileName || a.media?.title, connections: a.connections, start: a.start && !a.queueId, kind });
      if (a.queueId) update(d, { queueId: a.queueId });
      return d.id;
    }
    case 'probe': return { finalUrl: args.url, size: 734003200, resumable: true, fileName: decodeURIComponent(args.url.split('/').pop()), contentType: 'application/octet-stream' };
    case 'media_formats': return { title: 'Keynote 2026', choices: [
      { format: '299+140', label: '1080p60 MP4', kind: 'video', ext: 'mp4', height: 1080, size: 412_000_000 },
      { format: '22', label: '720p MP4', kind: 'video', ext: 'mp4', height: 720, size: 198_000_000 },
      { format: '140', label: 'Audio only M4A', kind: 'audio', ext: 'm4a', size: 31_000_000 },
    ] };
    case 'pause':
      for (const id of args.ids) { const d = downloads.get(id); if (d) update(d, { status: 'paused' }); }
      return null;
    case 'resume':
      for (const id of args.ids) { const d = downloads.get(id); if (d) update(d, { status: 'downloading', error: null }); }
      return null;
    case 'pause_all':
      for (const d of downloads.values()) if (['downloading', 'blocked', 'seeding'].includes(d.status)) update(d, { status: 'paused' });
      return null;
    case 'resume_all':
      for (const d of downloads.values()) if (d.status === 'paused') update(d, { status: 'downloading' });
      return null;
    case 'remove':
      for (const id of args.ids) { downloads.delete(id); emit('engine', { type: 'removed', id }); }
      return null;
    case 'set_connections': {
      const d = downloads.get(args.id);
      d.connections = args.connections;
      if (d.live) { delete d.live; d.downloaded = d.downloaded || 0; }
      update(d, {});
      return args.connections;
    }
    case 'queues_list': return structuredClone(queues);
    case 'queue_create': {
      const q = { id: queues.reduce((m, x) => Math.max(m, x.id), 0) + 1, name: args.name || `Queue ${queues.length + 1}`, maxActive: args.maxActive, running: false };
      queues.push(q); emitQueues(); return q;
    }
    case 'queue_update': {
      const q = queues.find((x) => x.id === args.id);
      if (args.name) q.name = args.name;
      if (args.maxActive) q.maxActive = args.maxActive;
      emitQueues(); return q;
    }
    case 'queue_start': {
      const q = queues.find((x) => x.id === args.id);
      q.running = true;
      for (const d of downloads.values()) if (d.queueId === q.id && d.status === 'paused') update(d, { status: 'queued' });
      emitQueues(); fillQueues(); return null;
    }
    case 'queue_stop': {
      const q = queues.find((x) => x.id === args.id);
      q.running = false;
      for (const d of downloads.values()) if (d.queueId === q.id && d.status === 'downloading') update(d, { status: 'paused' });
      emitQueues(); return null;
    }
    case 'queue_delete': {
      queues = queues.filter((x) => x.id !== args.id);
      for (const d of [...downloads.values()]) {
        if (d.queueId !== args.id) continue;
        if (args.removeDownloads) { downloads.delete(d.id); emit('engine', { type: 'removed', id: d.id }); }
        else update(d, { queueId: null, status: d.status === 'downloading' ? 'paused' : d.status });
      }
      emitQueues(); return null;
    }
    case 'queue_schedule': {
      const q = queues.find((x) => x.id === args.id);
      q.schedule = args.schedule;
      emitQueues(); return q;
    }
    case 'grab_site': {
      await new Promise((r) => setTimeout(r, 400));
      const site = new URL(args.options.startUrl).origin;
      const files = ['annual-report-2026.pdf', 'brand-assets.zip', 'product-tour.mp4', 'price-list.pdf', 'setup-1.4.2.exe']
        .filter((f) => args.options.fileTypes.includes(f.split('.').pop()))
        .map((f) => ({ url: `${site}/files/${f}`, fileName: f, pageUrl: `${site}/downloads/` }));
      return { files, pagesRead: 12, pagesSkippedByRobots: 1, stoppedEarly: false };
    }
    case 'cancel_shutdown': return null;
    case 'set_queue':
      for (const id of args.ids) { const d = downloads.get(id); if (d) update(d, { queueId: args.queueId }); }
      return null;
    case 'category_defaults': return {
      video: ['mp4', 'm4v', 'mkv', 'webm', 'avi', 'mov', 'wmv', 'flv', 'mpg', 'mpeg', '3gp', 'ts'],
      music: ['mp3', 'm4a', 'aac', 'flac', 'wav', 'ogg', 'opus', 'wma'],
      documents: ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'csv', 'epub'],
      compressed: ['zip', 'rar', '7z', 'tar', 'gz', 'iso', 'img', 'dmg'],
      programs: ['exe', 'msi', 'msix', 'apk', 'jar', 'deb', 'rpm'],
    };
    case 'get_settings': return structuredClone(settings);
    case 'save_settings': settings = structuredClone(args.settings); return null;
    case 'network_adapters': return ['Ethernet', 'Wi-Fi', 'Mullvad', 'NordLynx'];
    case 'privacy_status': return protectedNow() ? null : 'Waiting for a VPN or proxy. Set one up in Settings → Privacy, or allow torrents without one.';
    case 'test_proxy': if (/1080/.test(args.url)) return null; throw new Error('no answer');
    case 'open_download': case 'show_in_folder': case 'open_portfolio': return null;
    case 'refresh_link': {
      const d = downloads.get(args.id);
      window.__ldmCalls.push({ cmd, args: structuredClone(args) });
      // The same link still fails; the page opens in the browser.
      return { resumed: false, pageUrl: d.request.referer ?? d.request.url };
    }
    case 'refresh_address': {
      const d = downloads.get(args.id);
      update(d, { request: { ...d.request, url: args.url }, status: 'downloading', error: null });
      return null;
    }
    case 'open_page': case 'cancel_refresh':
      window.__ldmCalls.push({ cmd, args: structuredClone(args) });
      return null;
    case 'request_refresh':
      window.__ldmCalls.push({ cmd, args: structuredClone(args) });
      emit('refresh-request', args.id);
      return null;
    case 'start_background': {
      const d = make({ url: args.url, start: true, size: 64 * 1024 * 1024 });
      d.pending = true;
      update(d, { pending: true });
      window.__ldmCalls.push({ cmd, args: structuredClone(args) });
      return d.id;
    }
    case 'confirm_download': {
      const d = downloads.get(args.id);
      const a = args.args;
      window.__ldmCalls.push({ cmd, args: structuredClone(args) });
      update(d, {
        pending: false,
        fileName: a.fileName || d.fileName,
        saveDir: a.saveDir || d.saveDir,
        connections: a.connections || d.connections,
        queueId: a.queueId ?? null,
        status: a.queueId ? 'queued' : a.start ? d.status : 'paused',
      });
      return null;
    }
    case 'discard_download': {
      window.__ldmCalls.push({ cmd, args: structuredClone(args) });
      const d = downloads.get(args.id);
      if (d?.pending) { downloads.delete(d.id); emit('engine', { type: 'removed', id: d.id }); }
      return null;
    }
    case 'find_duplicate': {
      const cut = (u) => (u || '').split('#')[0];
      return [...downloads.values()].filter((d) => !d.pending).find((d) => cut(d.request.url) === cut(args.url)) ?? null;
    }
    case 'get_duplicate': return dupItems.get(args.id) ?? null;
    case 'resolve_duplicate': {
      window.__ldmCalls.push({ cmd, args: structuredClone(args) });
      const item = dupItems.get(args.id);
      dupItems.delete(args.id);
      if (args.remember && args.choice !== 'dismiss') settings.duplicates = args.choice;
      if (args.choice === 'again') return make({ url: item?.url ?? downloads.get(args.id).request.url, start: true }).id;
      return null;
    }
    case 'get': { const d = downloads.get(args.id); if (!d) throw new Error('no such download'); return { ...d, live: undefined }; }
    case 'open_download_window': case 'show_mini': case 'mini_dismiss': case 'window_hide': case 'window_close':
    case 'window_size': case 'window_title': case 'show_app': case 'exit_app':
      window.__ldmCalls.push({ cmd, args: structuredClone(args) });
      return null;
    default: throw new Error(`mock: ${cmd} isn't simulated`);
  }
}

export function listen(event, handler) {
  if (!listeners.has(event)) listeners.set(event, []);
  listeners.get(event).push(handler);
  return Promise.resolve(() => {});
}

export async function pick({ directory }) {
  return directory ? 'D:\\Media' : 'C:\\Users\\Layo\\Downloads\\linux.torrent';
}

/** For tests: this download finishes now (so tests don't wait on simulated speed). */
window.__ldmComplete = (id) => {
  const d = downloads.get(id);
  delete d.live;
  update(d, { status: 'completed', downloaded: d.size, completedAt: Date.now() });
};

const dupItems = new Map();
/** For tests: the browser sent a link that's already in the list. */
window.__ldmDuplicate = (id, item) => { dupItems.set(id, item); emit('duplicate', id); };
/** For tests: the network went away (downloads pause) or came back. */
window.__ldmNetwork = (online) => {
  let paused = 0;
  if (!online) {
    for (const d of downloads.values()) {
      if (d.status !== 'downloading') continue;
      paused++;
      delete d.live;
      update(d, { status: 'paused', error: 'Paused: the internet connection was lost. Continue it when you\'re back online.' });
    }
  }
  emit('network', online ? { online: true } : { online: false, paused });
};

/** For tests: the browser sent the fresh link an expired download was waiting for. */
window.__ldmFreshLink = (id) => {
  const d = downloads.get(id);
  update(d, { request: { ...d.request, url: d.request.url.replace(/token=[^&]*/, 'token=fresh') }, status: 'downloading', error: null });
  emit('link-refreshed', { id, fileName: d.fileName });
};

/** For tests: pretend the browser sent downloads. */
window.__ldmBrowserDownload = (items, source = 'capture', ids = []) => emit('browser-download', { items, source, ids });
