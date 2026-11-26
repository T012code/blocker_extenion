// Spotting video and audio in a page's network traffic, and reading HLS playlists.
// Pure functions only, so they can be tested outside the browser.

import { extensionOf } from './rules.js';

const FILE_EXTS = new Set(['mp4', 'm4v', 'webm', 'mov', 'mkv', 'ogv', 'mp3', 'm4a', 'ogg', 'oga', 'opus', 'flac', 'wav', 'aac']);
/** Pieces of a stream, not whole files. */
const SEGMENT_EXTS = new Set(['ts', 'm4s', 'cmfv', 'cmfa', 'm4f', 'aac', 'vtt', 'webvtt']);
const SEGMENT_MIMES = ['video/mp2t', 'video/iso.segment', 'audio/iso.segment', 'text/vtt'];
/** Smaller than this, a "video file" is almost always a piece of a stream or a preview. */
export const MIN_FILE_BYTES = 256 * 1024;

/**
 * What kind of media a response is, or null.
 * @param {{url: string, mime?: string, size?: number}} r
 * @returns {null | {kind: 'hls' | 'dash' | 'file', ext: string}}
 */
export function classify({ url, mime, size }) {
  if (!/^https?:/i.test(url ?? '')) return null;
  const m = String(mime ?? '').toLowerCase().split(';')[0].trim();
  const ext = extensionOf(url);
  if (m.includes('mpegurl') || ext === 'm3u8') return { kind: 'hls', ext: 'm3u8' };
  if (m === 'application/dash+xml' || ext === 'mpd') return { kind: 'dash', ext: 'mpd' };
  if (SEGMENT_EXTS.has(ext) || SEGMENT_MIMES.includes(m)) return null;
  // Sites that stream in byte-range pieces (YouTube and friends) put the range in the address.
  if (/[?&](range|rn|rbuf)=|\/range\/\d/i.test(url)) return null;
  // Facebook and Instagram fetch each stream in byte ranges named in the address.
  if (/[?&](bytestart|byteend)=/i.test(url)) return null;
  const media = m.startsWith('video/') || m.startsWith('audio/') || FILE_EXTS.has(ext);
  if (!media) return null;
  if (Number.isFinite(size) && size >= 0 && size < MIN_FILE_BYTES) return null;
  return { kind: 'file', ext: FILE_EXTS.has(ext) ? ext : (m.split('/')[1] || 'mp4').replace('mpeg', 'mp3').replace('quicktime', 'mov') };
}

/** Total size from Content-Range ("bytes 0-99/12345") or Content-Length. */
export function sizeFromHeaders(headers) {
  const get = (n) => headers?.find((h) => h.name.toLowerCase() === n)?.value;
  const range = get('content-range');
  const total = range && Number(range.split('/')[1]);
  if (Number.isFinite(total) && total > 0) return total;
  const len = Number(get('content-length'));
  return Number.isFinite(len) && len > 0 && !range ? len : undefined;
}

function attrs(line) {
  const out = {};
  // KEY=VALUE,KEY="quoted, value"
  for (const m of line.matchAll(/([A-Z0-9-]+)=("[^"]*"|[^,]*)/g)) out[m[1]] = m[2].replace(/^"|"$/g, '');
  return out;
}

/**
 * Reads an HLS playlist. A "master" playlist lists qualities; a media playlist is one
 * quality made of segments.
 */
export function parseHls(text, baseUrl) {
  const lines = String(text ?? '').split(/\r?\n/).map((l) => l.trim());
  if (lines[0] !== '#EXTM3U') return null;
  const variants = [];
  let encrypted = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line.startsWith('#EXT-X-KEY') || line.startsWith('#EXT-X-SESSION-KEY')) {
      const a = attrs(line);
      // AES-128 is ordinary encryption the downloader can handle; anything else is DRM.
      if (a.METHOD && a.METHOD !== 'NONE' && a.METHOD !== 'AES-128') encrypted = true;
    }
    if (!line.startsWith('#EXT-X-STREAM-INF')) continue;
    const a = attrs(line);
    const uri = lines.slice(i + 1).find((l) => l && !l.startsWith('#'));
    if (!uri) continue;
    let url;
    try { url = new URL(uri, baseUrl).href; } catch { continue; }
    const [, h] = (a.RESOLUTION ?? '').split('x').map(Number);
    variants.push({ url, bandwidth: Number(a.BANDWIDTH) || 0, height: Number.isFinite(h) && h > 0 ? h : null });
  }
  variants.sort((x, y) => (y.height ?? 0) - (x.height ?? 0) || y.bandwidth - x.bandwidth);
  // One entry per height (keep the best bitrate).
  const seen = new Set();
  const unique = variants.filter((v) => {
    const key = v.height ?? `bw${v.bandwidth}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  return { isMaster: unique.length > 0, variants: unique, protected: encrypted };
}

/** The menu label for an HLS quality. */
export function variantLabel(v) {
  if (v.height) return `${v.height}p`;
  if (v.bandwidth) return `${Math.round(v.bandwidth / 1000)} kbit/s`;
  return 'Stream';
}

/** Rough size of a stream quality, from its bitrate and the video length. */
export function estimateSize(bandwidth, seconds) {
  return bandwidth > 0 && seconds > 0 && Number.isFinite(seconds) ? Math.round((bandwidth / 8) * seconds) : null;
}
