// Takes browser downloads over and sends them to the app.
//
// Chrome, Edge, Opera, Brave and Vivaldi: downloads.onDeterminingFilename fires as soon as
// the browser knows it's a download, before it asks where to save. Firefox doesn't have
// that event, so there downloads.onCreated is used. Either way the browser's own copy is
// cancelled and erased, and the link goes to the app with the cookies, referrer and user
// agent the browser used. If the app can't be reached the download is handed back.

import { api } from '../shared/api.js';
import { getSettings } from '../shared/settings.js';
import { baseName, cookieHeader, decide } from './rules.js';
import * as native from './native.js';
import * as requests from './requests.js';
import { isBypassed, isHandedBack, noteHandedBack } from './bypass.js';
import { addHistory } from './history.js';

/** Downloads already looked at (events can fire twice). */
const seen = new Set();

export function askFlag(settings) {
  return settings.ask === 'always' ? true : settings.ask === 'never' ? false : null;
}

async function cookiesFor(url, incognito) {
  try {
    let storeId;
    if (incognito) {
      const stores = await api.cookies.getAllCookieStores();
      storeId = (stores.find((s) => s.incognito) ?? stores.find((s) => s.id !== '0' && s.id !== 'firefox-default'))?.id;
    }
    const cookies = await api.cookies.getAll(storeId ? { url, storeId } : { url });
    return cookieHeader(cookies) || null;
  } catch {
    return null;
  }
}

/** What the app needs to fetch `url` the way the browser would. */
export async function describe({ url, referrer, incognito, fileName, fileSize, mime, pageUrl, description }) {
  const req = requests.lookup(url);
  const cookies = req?.cookie ?? (await cookiesFor(url, incognito));
  return {
    url,
    referer: referrer || req?.referer || pageUrl || null,
    cookies,
    userAgent: req?.userAgent || navigator.userAgent,
    fileName: fileName || null,
    fileSize: Number(fileSize) > 0 ? Number(fileSize) : null,
    mime: mime || null,
    pageUrl: pageUrl || null,
    description: description || null,
  };
}

async function evaluate(item) {
  const [settings, host] = await Promise.all([getSettings(), native.getState()]);
  const url = item.finalUrl || item.url;
  const req = requests.lookup(url, item.url);
  const ctx = {
    hostReady: host.installed,
    ownExtensionId: api.runtime.id,
    bypassed: isBypassed(url, item.url),
    letThrough: isHandedBack(url, item.url),
    method: req?.method,
  };
  return { decision: decide(item, settings, ctx), settings };
}

async function stopBrowserCopy(id) {
  try {
    await api.downloads.cancel(id);
  } catch { /* already finished or gone */ }
  try {
    await api.downloads.erase({ id });
  } catch { /* ignore */ }
}

/**
 * @param trustName The browser's file name is the real one (Chrome's file-name step, or
 *   Firefox). In Chrome's backup path it can be a temporary name, so the app asks the server.
 */
async function sendToApp(item, settings, trustName) {
  const url = item.finalUrl || item.url;
  const browserName = trustName ? baseName(item.filename) : '';
  const name = browserName || baseName(url);
  const payload = await describe({
    url,
    referrer: item.referrer,
    incognito: item.incognito,
    fileName: browserName || null,
    fileSize: item.fileSize > 0 ? item.fileSize : item.totalBytes,
    mime: item.mime,
  });
  try {
    await native.send({ type: 'download', source: 'capture', ask: askFlag(settings), items: [payload] });
    await addHistory({ name, url, status: 'sent' });
  } catch (e) {
    native.markStale();
    await giveBack(url, name, e, settings);
  }
}

/** The app couldn't take it: let the browser download it after all. */
async function giveBack(url, name, error, settings) {
  noteHandedBack(url);
  await addHistory({ name, url, status: 'returned', reason: String(error?.message ?? error) });
  try {
    await api.downloads.download({ url });
  } catch { /* the link may only have worked once */ }
  if (settings.notifyFallback) {
    api.notifications?.create({
      type: 'basic',
      iconUrl: api.runtime.getURL('icons/icon-128.png'),
      title: 'Downloading in the browser instead',
      message: `Layó's Download Manager couldn't take "${name}": ${error?.message ?? error}`,
    }).catch?.(() => {});
  }
}

function startChromium() {
  api.downloads.onDeterminingFilename.addListener((item, suggest) => {
    if (seen.has(item.id)) return false;
    seen.add(item.id);
    evaluate(item)
      .then(async ({ decision, settings }) => {
        if (!decision.capture) {
          suggest();
          return;
        }
        // Cancel before answering, so the browser never shows its "Save as" window.
        await stopBrowserCopy(item.id);
        suggest();
        await sendToApp(item, settings, true);
      })
      .catch(() => suggest());
    return true; // we answer asynchronously
  });
}

async function onCreated(item, trustName) {
  if (seen.has(item.id) || item.state !== 'in_progress') return;
  seen.add(item.id);
  // Decide on the address and type only when the name may be a temporary one.
  const { decision, settings } = await evaluate(trustName ? item : { ...item, filename: '' });
  if (!decision.capture) return;
  await stopBrowserCopy(item.id);
  await sendToApp(item, settings, trustName);
}

export function start() {
  if (api.downloads.onDeterminingFilename) {
    startChromium();
    // Backup: when something else controls downloads (enterprise policy, automation),
    // Chrome skips the file name step. Catch those a moment after they start.
    api.downloads.onCreated.addListener((item) => {
      setTimeout(async () => {
        if (seen.has(item.id)) return;
        const [now] = await api.downloads.search({ id: item.id });
        if (now?.state === 'in_progress') onCreated({ ...item, ...now }, false);
      }, 1000);
    });
  } else {
    api.downloads.onCreated.addListener((item) => onCreated(item, true));
  }
}
