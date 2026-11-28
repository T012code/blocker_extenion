// Finds the videos a tab plays, and builds the menu for the on-video download button.
//
// Two sources:
//  1. Network traffic: whole media files, and HLS/DASH stream playlists. HLS master
//     playlists are read to list their qualities.
//  2. The app: for sites like YouTube (where the player streams in pieces), yt-dlp in
//     Layó's Download Manager lists the formats for the page address.

import { api } from '../shared/api.js';
import { baseName, extensionOf } from './rules.js';
import { classify, estimateSize, parseHls, sizeFromHeaders, variantLabel } from './media-detect.js';
import { describe } from './capture.js';
import * as native from './native.js';
import { addHistory } from './history.js';

const MAX_PER_TAB = 40;
/** tabId -> Map(url -> stream) */
const tabs = new Map();

function streamsOf(tabId) {
  if (!tabs.has(tabId)) tabs.set(tabId, new Map());
  return tabs.get(tabId);
}

async function readHls(stream) {
  try {
    const res = await fetch(stream.url, { credentials: 'include' });
    const text = await res.text();
    const parsed = parseHls(text, res.url || stream.url);
    if (parsed) Object.assign(stream, parsed);
  } catch { /* the player will have it; we just can't list qualities */ }
}

function remember(tabId, stream) {
  const list = streamsOf(tabId);
  if (list.has(stream.url)) return;
  // A quality playlist of a master we already know isn't a separate video.
  for (const s of list.values()) if (s.variants?.some((v) => v.url === stream.url)) return;
  list.set(stream.url, stream);
  while (list.size > MAX_PER_TAB) list.delete(list.keys().next().value);
  if (stream.kind === 'hls') readHls(stream);
}

export function start() {
  api.webRequest.onHeadersReceived.addListener(
    (d) => {
      if (d.tabId < 0 || d.statusCode >= 400) return;
      const mime = d.responseHeaders?.find((h) => h.name.toLowerCase() === 'content-type')?.value;
      const size = sizeFromHeaders(d.responseHeaders);
      const c = classify({ url: d.url, mime, size });
      if (c) remember(d.tabId, { ...c, url: d.url, mime, size, pageUrl: d.documentUrl ?? d.initiator, at: Date.now() });
    },
    { urls: ['http://*/*', 'https://*/*'], types: ['media', 'xmlhttprequest', 'other', 'object', 'sub_frame'] },
    ['responseHeaders'],
  );
  // A new page in the tab: forget the old page's videos.
  api.tabs.onUpdated.addListener((tabId, change) => {
    if (change.status === 'loading' && change.url) tabs.delete(tabId);
  });
  api.tabs.onRemoved.addListener((tabId) => tabs.delete(tabId));
}

/**
 * Choices found without asking the app: the video element's own file, and what the tab
 * downloaded. Each choice says how to download it.
 */
export function localChoices(tabId, { src, duration }) {
  const out = [];
  const seen = new Set();
  const add = (c) => {
    const key = c.action.url ?? c.action.pageUrl;
    if (seen.has(key)) return;
    seen.add(key);
    out.push(c);
  };
  const list = [...streamsOf(tabId).values()].sort((a, b) => b.at - a.at);
  if (/^https?:/i.test(src ?? '')) {
    const known = streamsOf(tabId).get(src);
    const c = classify({ url: src, mime: known?.mime });
    if (c?.kind === 'file') {
      add({ label: `Video file ${(c.ext || extensionOf(src) || 'mp4').toUpperCase()}`, size: known?.size ?? null, kind: 'video', action: { type: 'file', url: src } });
    }
  }
  for (const s of list) {
    if (s.protected) {
      add({ label: 'Protected stream (can\'t be downloaded)', disabled: true, kind: 'video', action: { type: 'none', url: s.url } });
    } else if (s.kind === 'hls' && s.variants?.length) {
      for (const v of s.variants) {
        add({
          label: `${variantLabel(v)} stream`,
          size: estimateSize(v.bandwidth, duration),
          estimated: true,
          kind: v.height ? 'video' : 'audio',
          action: { type: 'media', pageUrl: v.url, label: `${variantLabel(v)} stream` },
        });
      }
    } else if (s.kind === 'hls' || s.kind === 'dash') {
      add({ label: `${s.kind === 'hls' ? 'HLS' : 'DASH'} stream, best quality`, size: null, kind: 'video', action: { type: 'media', pageUrl: s.url, label: 'Best quality' } });
    } else if (s.kind === 'file') {
      const name = baseName(s.url) || `media.${s.ext}`;
      add({ label: `${s.ext.toUpperCase()} file · ${name}`, size: s.size ?? null, kind: /^(mp3|m4a|ogg|oga|opus|flac|wav|aac)$/.test(s.ext) ? 'audio' : 'video', action: { type: 'file', url: s.url } });
    }
  }
  return out;
}

/** Asks the app (yt-dlp) for the formats of the page. */
export async function siteChoices({ pageUrl, incognito }) {
  const item = await describe({ url: pageUrl, referrer: pageUrl, incognito });
  const r = await native.send({ type: 'mediaFormats', item }, 120_000);
  return {
    title: r.title,
    live: r.isLive,
    choices: (r.choices ?? []).map((c) => ({
      label: c.label,
      size: c.size ?? null,
      kind: c.kind,
      action: { type: 'media', pageUrl, format: c.format, label: c.label, item: c.item ?? null },
    })),
  };
}

/** Sends the picked choice to the app. */
export async function download({ action, title, pageUrl, incognito }) {
  if (action.type === 'none') throw new Error('This video can\'t be downloaded.');
  let item;
  if (action.type === 'file') {
    const ext = extensionOf(action.url);
    const safeTitle = (title || '').replace(/[\\/:*?"<>|]+/g, ' ').trim().slice(0, 150);
    item = await describe({
      url: action.url, referrer: pageUrl, incognito, pageUrl,
      fileName: safeTitle && ext ? `${safeTitle}.${ext}` : null, description: title,
    });
  } else {
    item = await describe({ url: action.pageUrl, referrer: pageUrl, incognito, pageUrl, description: title });
    item.media = { pageUrl: action.pageUrl, format: action.format ?? null, label: action.label ?? null, title: title || null, item: action.item ?? null };
  }
  const r = await native.send({ type: 'download', source: 'media', ask: false, items: [item] });
  await addHistory({ name: title || baseName(item.url), url: item.url, status: 'sent' });
  return r;
}
