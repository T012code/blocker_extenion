// Decides which browser downloads Layó's Download Manager takes over.
// Pure functions only, so they can be tested outside the browser.

/** File types taken over by default. Users can edit the list in the options page. */
export const DEFAULT_FILE_TYPES = [
  // Archives and disk images
  '7z', 'arj', 'bz2', 'cab', 'gz', 'img', 'iso', 'lz', 'lzh', 'rar', 'tar', 'tbz2', 'tgz', 'txz', 'xz', 'z', 'zip', 'zst',
  // Programs and packages
  'apk', 'appx', 'appxbundle', 'bin', 'deb', 'dmg', 'exe', 'jar', 'msi', 'msix', 'msixbundle', 'msu', 'pkg', 'rpm',
  // Video
  '3gp', 'avi', 'flv', 'm2ts', 'm4v', 'mkv', 'mov', 'mp4', 'mpeg', 'mpg', 'mts', 'ogv', 'rm', 'rmvb', 'ts', 'vob', 'webm', 'wmv',
  // Audio
  'aac', 'aif', 'aiff', 'ape', 'flac', 'm4a', 'm4b', 'mka', 'mp3', 'oga', 'ogg', 'opus', 'wav', 'wma',
  // Documents and books
  'azw3', 'djvu', 'epub', 'mobi', 'pdf', 'ppt', 'pptx', 'doc', 'docx', 'xls', 'xlsx',
  // Other large files
  'torrent',
];

/** Content types worth taking over when the file name has no known extension. */
const CAPTURE_MIME_PREFIXES = ['video/', 'audio/'];
const CAPTURE_MIMES = new Set([
  'application/zip', 'application/x-zip-compressed', 'application/x-rar-compressed', 'application/vnd.rar',
  'application/x-7z-compressed', 'application/gzip', 'application/x-gzip', 'application/x-tar', 'application/x-bzip2',
  'application/x-xz', 'application/zstd', 'application/x-iso9660-image', 'application/x-msdownload',
  'application/x-msdos-program', 'application/x-msi', 'application/x-ms-installer', 'application/vnd.microsoft.portable-executable',
  'application/vnd.android.package-archive', 'application/x-apple-diskimage', 'application/java-archive',
  'application/vnd.debian.binary-package', 'application/x-rpm', 'application/pdf', 'application/epub+zip',
  'application/x-bittorrent',
]);

export const DEFAULT_SETTINGS = Object.freeze({
  /** Take over browser downloads at all. */
  enabled: true,
  /** Space- or comma-separated extensions. */
  fileTypes: DEFAULT_FILE_TYPES.join(' '),
  /** Take over every download, whatever its type. */
  captureAll: false,
  /** Leave files smaller than this (KB) to the browser. 0 = no minimum. */
  minSizeKB: 0,
  /** Sites whose downloads stay in the browser, one per line. `example.com` covers its subdomains. */
  skipSites: '',
  /** Hold this key while clicking a link to let the browser download it: alt, shift, ctrl or none. */
  bypassKey: 'alt',
  captureIncognito: true,
  /** Show the app's "add download" window first ('app' = follow the app's own setting). */
  ask: 'app',
  /** Show a notification when a download goes back to the browser. */
  notifyFallback: true,
  /** Show the "Download" button over videos. */
  videoButton: true,
  /** Open magnet links in Layó's Download Manager. */
  captureMagnets: true,
});

export function parseFileTypes(text) {
  return new Set(
    String(text ?? '')
      .toLowerCase()
      .split(/[\s,;]+/)
      .map((t) => t.replace(/^\*?\./, '').trim())
      .filter(Boolean),
  );
}

export function parseSites(text) {
  return String(text ?? '')
    .split(/[\s,;]+/)
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean)
    .map((s) => s.replace(/^[a-z]+:\/\//, '').replace(/\/.*$/, '').replace(/^\*\./, ''));
}

/** `example.com` matches `example.com` and `dl.example.com`, not `badexample.com`. */
export function hostMatches(host, site) {
  host = String(host ?? '').toLowerCase().replace(/\.$/, '');
  return host === site || host.endsWith('.' + site);
}

export function hostOf(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return '';
  }
}

/** The last path part of a file name or URL, without query or fragment. */
export function baseName(nameOrUrl) {
  let s = String(nameOrUrl ?? '');
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(s)) {
    try {
      s = decodeURIComponent(new URL(s).pathname);
    } catch {
      s = s.split(/[?#]/)[0];
    }
  }
  return s.split(/[\\/]/).filter(Boolean).pop() ?? '';
}

export function extensionOf(nameOrUrl) {
  const name = baseName(nameOrUrl);
  const dot = name.lastIndexOf('.');
  return dot > 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : '';
}

function isWebUrl(url) {
  return /^https?:\/\//i.test(String(url ?? ''));
}

function mimeWanted(mime) {
  const m = String(mime ?? '').toLowerCase().split(';')[0].trim();
  if (!m) return false;
  // Streaming playlists are pages of links, not files; the video grabber handles those.
  if (m === 'application/vnd.apple.mpegurl' || m === 'application/x-mpegurl' || m === 'application/dash+xml') return false;
  return CAPTURE_MIMES.has(m) || CAPTURE_MIME_PREFIXES.some((p) => m.startsWith(p));
}

/**
 * Should this browser download go to the app?
 *
 * @param {object} item     Download info: url, finalUrl, filename, mime, fileSize, incognito, byExtensionId.
 * @param {object} settings Merged with DEFAULT_SETTINGS.
 * @param {object} ctx      hostReady, ownExtensionId, bypassed, letThrough, method.
 * @returns {{capture: boolean, reason: string}}
 */
export function decide(item, settings, ctx = {}) {
  const s = { ...DEFAULT_SETTINGS, ...settings };
  const no = (reason) => ({ capture: false, reason });
  const url = item.finalUrl || item.url;

  if (!s.enabled) return no('turned off');
  if (!ctx.hostReady) return no('download manager not installed');
  // blob:, data: and file: downloads only exist inside the browser.
  if (!isWebUrl(url)) return no('not a web address');
  if (item.byExtensionId && item.byExtensionId !== ctx.ownExtensionId) return no('started by another extension');
  if (item.byExtensionId && item.byExtensionId === ctx.ownExtensionId) return no('handed back to the browser');
  if (ctx.letThrough) return no('handed back to the browser');
  if (ctx.bypassed) return no('bypass key held');
  // A form post can't be repeated by another program.
  if (ctx.method && ctx.method.toUpperCase() !== 'GET') return no('form submission');
  if (item.incognito && !s.captureIncognito) return no('private window');

  const host = hostOf(url);
  const pageHost = hostOf(item.referrer);
  const skip = parseSites(s.skipSites);
  if (skip.some((site) => hostMatches(host, site) || (pageHost && hostMatches(pageHost, site)))) {
    return no('site is on the skip list');
  }

  const size = Number(item.fileSize ?? item.totalBytes ?? -1);
  const minBytes = Number(s.minSizeKB || 0) * 1024;
  if (minBytes > 0 && size >= 0 && size < minBytes) return no('smaller than the minimum size');

  if (s.captureAll) return { capture: true, reason: 'all downloads' };

  const types = parseFileTypes(s.fileTypes);
  const ext = extensionOf(item.filename) || extensionOf(url);
  if (ext && types.has(ext)) return { capture: true, reason: `.${ext} file` };
  if (!ext && mimeWanted(item.mime)) return { capture: true, reason: `${item.mime} content` };
  // Some servers send a generic name like "download.php" but a real type.
  if (ext && !types.has(ext) && ['php', 'asp', 'aspx', 'jsp', 'cgi', 'do', 'action'].includes(ext) && mimeWanted(item.mime)) {
    return { capture: true, reason: `${item.mime} content` };
  }
  return no(ext ? `.${ext} is not in the file type list` : 'unknown file type');
}

/** Picks the links worth downloading from a page, for "Download all links". */
export function pickLinks(links, settings) {
  const types = parseFileTypes({ ...DEFAULT_SETTINGS, ...settings }.fileTypes);
  const seen = new Set();
  const out = [];
  for (const link of links) {
    const url = String(link.url ?? '').split('#')[0];
    if (!isWebUrl(url) || seen.has(url)) continue;
    const ext = extensionOf(url);
    if (!ext || !types.has(ext)) continue;
    seen.add(url);
    out.push({ ...link, url });
  }
  return out;
}

/** Builds a `Cookie` header from cookies, most specific path first (as browsers send them). */
export function cookieHeader(cookies) {
  return [...cookies]
    .sort((a, b) => (b.path?.length ?? 0) - (a.path?.length ?? 0))
    .map((c) => `${c.name}=${c.value}`)
    .join('; ');
}
