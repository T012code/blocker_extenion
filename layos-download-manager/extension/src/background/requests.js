// Remembers recent requests so a download can be repeated exactly as the browser made it:
// the method (a POST can't be repeated), cookies, referrer and user agent.

import { api, isFirefox } from '../shared/api.js';

const KEEP_MS = 2 * 60_000;
const MAX = 400;
const recent = new Map();

function remember(url, patch) {
  const now = Date.now();
  const entry = { ...(recent.get(url) ?? {}), ...patch, at: now };
  recent.delete(url);
  recent.set(url, entry);
  while (recent.size > MAX) recent.delete(recent.keys().next().value);
}

function header(headers, name) {
  return headers?.find((h) => h.name.toLowerCase() === name)?.value;
}

export function lookup(...urls) {
  const now = Date.now();
  for (const url of urls) {
    const e = url && recent.get(url);
    if (e && now - e.at < KEEP_MS) return e;
  }
  return null;
}

export function start() {
  const filter = { urls: ['http://*/*', 'https://*/*'], types: ['main_frame', 'sub_frame', 'other', 'xmlhttprequest', 'media'] };
  api.webRequest.onBeforeRequest.addListener((d) => {
    remember(d.url, { method: d.method });
  }, filter);
  // Chrome hides Cookie and Referer unless 'extraHeaders' is asked for; Firefox shows them and rejects the option.
  const extra = !isFirefox && api.webRequest.OnBeforeSendHeadersOptions?.EXTRA_HEADERS ? ['extraHeaders'] : [];
  api.webRequest.onBeforeSendHeaders.addListener((d) => {
    remember(d.url, {
      method: d.method,
      cookie: header(d.requestHeaders, 'cookie'),
      referer: header(d.requestHeaders, 'referer'),
      userAgent: header(d.requestHeaders, 'user-agent'),
    });
  }, filter, ['requestHeaders', ...extra]);
  // Follow redirects, so the final address finds the original request.
  api.webRequest.onBeforeRedirect.addListener((d) => {
    const from = recent.get(d.url);
    if (from) remember(d.redirectUrl, { method: from.method === 'POST' && d.statusCode === 307 ? 'POST' : 'GET', referer: from.referer });
  }, filter);
}
