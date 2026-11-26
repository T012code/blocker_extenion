// Talks to the app through the bridge program (Native Messaging host).

import { api } from '../shared/api.js';

export const HOST_NAME = 'com.layo.downloadmanager';

/** Last known bridge state, also kept in session storage for the popup and for service worker restarts. */
let state = { checked: 0, installed: false, appRunning: false, appVersion: null, hostVersion: null, error: null };

function withTimeout(promise, ms, what) {
  let timer;
  return Promise.race([
    promise.finally(() => clearTimeout(timer)),
    new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${what} timed out`)), ms); }),
  ]);
}

/** Sends one message; the browser starts the bridge, which starts the app if needed. */
export async function send(message, timeoutMs = 45_000) {
  const reply = await withTimeout(api.runtime.sendNativeMessage(HOST_NAME, message), timeoutMs, 'Layó\'s Download Manager');
  if (!reply) throw new Error('no reply from Layó\'s Download Manager');
  if (reply.type === 'error') throw new Error(reply.message);
  return reply;
}

async function store() {
  try {
    await api.storage.session.set({ hostState: state });
  } catch { /* session storage missing in very old browsers */ }
  // Content scripts can't read session storage everywhere; they only need this flag.
  api.storage.local.set({ hostReady: !!state.installed }).catch(() => {});
  updateBadge();
}

function updateBadge() {
  const ok = state.installed;
  api.action.setBadgeText({ text: ok ? '' : '!' }).catch(() => {});
  api.action.setBadgeBackgroundColor?.({ color: '#c42b1c' })?.catch?.(() => {});
  api.action.setTitle({
    title: ok
      ? 'Layó\'s Download Manager'
      : 'Layó\'s Download Manager is not installed or the browser bridge is missing',
  }).catch(() => {});
}

export async function refresh() {
  try {
    const r = await send({ type: 'ping' }, 8_000);
    state = {
      checked: Date.now(), installed: true, appRunning: !!r.appRunning,
      appVersion: r.appVersion ?? null, hostVersion: r.hostVersion ?? null, error: null,
    };
  } catch (e) {
    state = { ...state, checked: Date.now(), installed: false, appRunning: false, error: String(e?.message ?? e) };
  }
  await store();
  return state;
}

/** The bridge state, checking again when unknown or older than five minutes. */
export async function getState({ maxAgeMs = 5 * 60_000 } = {}) {
  if (!state.checked) {
    try {
      const saved = (await api.storage.session.get('hostState')).hostState;
      if (saved) state = saved;
    } catch { /* ignore */ }
  }
  if (!state.checked) return refresh();
  if (Date.now() - state.checked > maxAgeMs) refresh(); // in the background
  return state;
}

/** A download just failed to reach the app: check again soon. */
export function markStale() {
  state = { ...state, checked: 1 };
}
