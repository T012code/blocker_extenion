// Settings live in chrome.storage.local under one key so every part of the
// extension reads and writes the same shape.

export const RULESETS = [
  'ads', 'trackers', 'security', 'strip_params', 'privacy_signals',
  'block_pings', 'social', 'hide_referrer', 'https_upgrade',
];

// Rulesets whose matches count as "blocked" in the popup.
export const COUNTED_RULESETS = new Set(['ads', 'trackers', 'security', 'social', 'block_pings']);

export const DEFAULTS = Object.freeze({
  enabled: true,
  rulesets: {
    ads: true,
    trackers: true,
    security: true,
    strip_params: true,
    privacy_signals: true,
    block_pings: true,
    social: false,
    hide_referrer: false,
    https_upgrade: false,
  },
  cosmetic: true,
  youtube: true,
  annoyances: false,
  popups: true,
  privacy: {
    webrtc: true,
    hyperlinkAuditing: true,
    thirdPartyCookies: false,
    networkPrediction: false,
  },
  allowlist: [],
  customHides: {},
});

function merge(base, value) {
  if (Array.isArray(base)) return Array.isArray(value) ? value : base;
  if (base && typeof base === 'object') {
    const out = {};
    for (const key of Object.keys(base)) out[key] = merge(base[key], value?.[key]);
    if (value && typeof value === 'object') {
      for (const key of Object.keys(value)) if (!(key in base)) out[key] = value[key];
    }
    return out;
  }
  return typeof value === typeof base ? value : base;
}

export async function getSettings() {
  const { settings } = await chrome.storage.local.get('settings');
  return merge(DEFAULTS, settings);
}

export async function saveSettings(settings) {
  await chrome.storage.local.set({ settings });
  return settings;
}

export async function updateSettings(mutator) {
  const settings = await getSettings();
  await mutator(settings);
  return saveSettings(settings);
}

/** "www.Example.com." -> "example.com". Returns '' for anything that isn't a host name. */
export function normalizeHost(input) {
  let host = String(input || '').trim().toLowerCase();
  if (!host) return '';
  try {
    if (host.includes('/') || host.includes(':')) host = new URL(host.includes('://') ? host : `http://${host}`).hostname;
  } catch {
    return '';
  }
  host = host.replace(/^www\./, '').replace(/\.$/, '');
  return /^[a-z0-9.-]+\.[a-z0-9-]{2,}$|^localhost$/.test(host) ? host : '';
}

/** True when host is on the allowlist directly or through a parent domain. */
export function isAllowlisted(settings, host) {
  host = normalizeHost(host);
  if (!host) return false;
  return settings.allowlist.some((d) => host === d || host.endsWith(`.${d}`));
}
