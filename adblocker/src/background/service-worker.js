import {
  RULESETS, COUNTED_RULESETS, getSettings, updateSettings, normalizeHost, isAllowlisted,
} from '../shared/settings.js';

const BLOCKED_PAGE = chrome.runtime.getURL('src/blocked/blocked.html');

// Dynamic rule IDs. Everything else (proceed-anyway exceptions) uses session rules.
const RULE_SECURITY_WARNING = 1;
const RULE_ALLOWLIST = 2;

// Priorities: allowlist beats the security warning, which beats everything in the static lists.
const PRIORITY_ALLOWLIST = 100;
const PRIORITY_PROCEED = 90;
const PRIORITY_WARNING = 50;

let popupDomains = null;
let securityDomains = null;

async function loadJson(path) {
  const res = await fetch(chrome.runtime.getURL(path));
  return res.json();
}

async function getPopupDomains() {
  popupDomains ??= new Set(await loadJson('rules/popup-domains.json'));
  return popupDomains;
}

async function getSecurityDomains() {
  securityDomains ??= await loadJson('rules/security-domains.json');
  return securityDomains;
}

// ------------------------------------------------------------------ applying settings

let applying = Promise.resolve();

/** Brings the browser in line with the saved settings. Calls are queued, never run in parallel. */
function apply() {
  applying = applying.then(applyNow, applyNow);
  return applying;
}

async function applyNow() {
  const settings = await getSettings();
  const steps = [
    ['rulesets', () => applyRulesets(settings)],
    ['dynamic rules', () => applyDynamicRules(settings)],
    ['content scripts', () => applyContentScripts(settings)],
    ['privacy', () => applyPrivacy(settings)],
    ['badge', () => applyBadge(settings)],
  ];
  for (const [name, step] of steps) {
    try {
      await step();
    } catch (err) {
      console.error(`Could not apply ${name}:`, err);
    }
  }
}

async function applyRulesets(settings) {
  const enable = [];
  const disable = [];
  for (const id of RULESETS) {
    (settings.enabled && settings.rulesets[id] ? enable : disable).push(id);
  }
  await chrome.declarativeNetRequest.updateEnabledRulesets({
    enableRulesetIds: enable,
    disableRulesetIds: disable,
  });
}

async function applyDynamicRules(settings) {
  const addRules = [];

  if (settings.enabled && settings.allowlist.length) {
    addRules.push({
      id: RULE_ALLOWLIST,
      priority: PRIORITY_ALLOWLIST,
      action: { type: 'allowAllRequests' },
      condition: { requestDomains: settings.allowlist, resourceTypes: ['main_frame'] },
    });
  }

  if (settings.enabled && settings.rulesets.security) {
    addRules.push({
      id: RULE_SECURITY_WARNING,
      priority: PRIORITY_WARNING,
      action: { type: 'redirect', redirect: { regexSubstitution: `${BLOCKED_PAGE}#\\0` } },
      condition: {
        regexFilter: '^.+$',
        requestDomains: await getSecurityDomains(),
        resourceTypes: ['main_frame'],
      },
    });
  }

  const existing = await chrome.declarativeNetRequest.getDynamicRules();
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: existing.map((r) => r.id),
    addRules,
  });
}

function excludePatterns(allowlist) {
  return allowlist.flatMap((d) => [`*://${d}/*`, `*://*.${d}/*`]);
}

async function applyContentScripts(settings) {
  const registered = await chrome.scripting.getRegisteredContentScripts();
  if (registered.length) {
    await chrome.scripting.unregisterContentScripts({ ids: registered.map((s) => s.id) });
  }
  if (!settings.enabled) return;

  const excludeMatches = excludePatterns(settings.allowlist);
  const everywhere = {
    matches: ['<all_urls>'],
    excludeMatches,
    allFrames: true,
    runAt: 'document_start',
  };
  const youtubeSites = ['*://*.youtube.com/*', '*://*.youtube-nocookie.com/*'];
  const scripts = [];

  if (settings.cosmetic) {
    scripts.push({ id: 'cosmetic', css: ['src/content/cosmetic.css'], ...everywhere });
  }
  if (settings.annoyances) {
    scripts.push({ id: 'annoyances', css: ['src/content/annoyances.css'], ...everywhere });
  }
  if (Object.keys(settings.customHides).length) {
    scripts.push({ id: 'custom-hides', js: ['src/content/custom-hides.js'], ...everywhere });
  }
  if (settings.youtube) {
    scripts.push({
      id: 'youtube-main',
      js: ['src/content/youtube-main.js'],
      world: 'MAIN',
      matches: youtubeSites,
      excludeMatches,
      allFrames: true,
      runAt: 'document_start',
    });
    scripts.push({
      id: 'youtube',
      js: ['src/content/youtube.js'],
      css: ['src/content/youtube.css'],
      matches: youtubeSites,
      excludeMatches,
      allFrames: true,
      runAt: 'document_start',
    });
  }
  if (scripts.length) await chrome.scripting.registerContentScripts(scripts);
}

async function setPrivacy(setting, protect, protectedValue) {
  try {
    if (protect) await setting.set({ value: protectedValue });
    else await setting.clear({});
  } catch (err) {
    // Some browsers or managed (work/school) setups don't allow changing these.
    console.warn('Privacy setting not changed:', err?.message || err);
  }
}

async function applyPrivacy(settings) {
  const on = settings.enabled;
  const p = settings.privacy;
  await Promise.all([
    setPrivacy(chrome.privacy.network.webRTCIPHandlingPolicy, on && p.webrtc, 'disable_non_proxied_udp'),
    setPrivacy(chrome.privacy.websites.hyperlinkAuditingEnabled, on && p.hyperlinkAuditing, false),
    setPrivacy(chrome.privacy.websites.thirdPartyCookiesAllowed, on && p.thirdPartyCookies, false),
    setPrivacy(chrome.privacy.network.networkPredictionEnabled, on && p.networkPrediction, false),
  ]);
}

async function applyBadge(settings) {
  await chrome.action.setBadgeBackgroundColor({ color: settings.enabled ? '#1f6f5c' : '#6b6b6b' });
  if (chrome.action.setBadgeTextColor) await chrome.action.setBadgeTextColor({ color: '#ffffff' });
  await chrome.declarativeNetRequest.setExtensionActionOptions({
    displayActionCountAsBadgeText: settings.enabled,
  });
  await chrome.action.setBadgeText({ text: settings.enabled ? '' : 'off' });
}

// ------------------------------------------------------------------ popup windows

// Tabs opened by a page (window.open, target=_blank) in the last few seconds.
const openedTabs = new Map();

chrome.webNavigation.onCreatedNavigationTarget.addListener((details) => {
  openedTabs.set(details.tabId, { sourceTabId: details.sourceTabId, at: Date.now() });
  checkPopup(details.tabId, details.url);
});

chrome.webNavigation.onBeforeNavigate.addListener((details) => {
  if (details.frameId === 0 && openedTabs.has(details.tabId)) checkPopup(details.tabId, details.url);
});

chrome.tabs.onRemoved.addListener((tabId) => openedTabs.delete(tabId));

async function checkPopup(tabId, url) {
  const opened = openedTabs.get(tabId);
  if (!opened) return;
  if (Date.now() - opened.at > 10_000) {
    openedTabs.delete(tabId);
    return;
  }
  const host = normalizeHost(url);
  if (!host) return;

  const settings = await getSettings();
  if (!settings.enabled || !settings.popups) return;

  const domains = await getPopupDomains();
  let match = false;
  for (let h = host; h.includes('.'); h = h.slice(h.indexOf('.') + 1)) {
    if (domains.has(h)) {
      match = true;
      break;
    }
  }
  if (!match) return;

  try {
    const source = await chrome.tabs.get(opened.sourceTabId);
    if (isAllowlisted(settings, source.url)) return;
  } catch {
    // The page that opened it is already gone. Still close the ad.
  }
  openedTabs.delete(tabId);
  chrome.tabs.remove(tabId).catch(() => {});
}

// ------------------------------------------------------------------ messages

async function blockedCount(tabId) {
  try {
    const { rulesMatchedInfo } = await chrome.declarativeNetRequest.getMatchedRules({ tabId });
    return rulesMatchedInfo.filter((m) => COUNTED_RULESETS.has(m.rule.rulesetId)).length;
  } catch {
    return null;
  }
}

async function allowProceed(url) {
  const host = normalizeHost(url);
  if (!host) throw new Error('Bad address');
  const existing = await chrome.declarativeNetRequest.getSessionRules();
  const id = existing.reduce((max, r) => Math.max(max, r.id), 0) + 1;
  await chrome.declarativeNetRequest.updateSessionRules({
    addRules: [{
      id,
      priority: PRIORITY_PROCEED,
      action: { type: 'allow' },
      condition: { requestDomains: [host], resourceTypes: ['main_frame'] },
    }],
  });
}

async function startPicker(tabId) {
  await chrome.scripting.executeScript({ target: { tabId }, files: ['src/content/picker.js'] });
}

const handlers = {
  async tabInfo({ tabId }) {
    const tab = await chrome.tabs.get(tabId);
    const settings = await getSettings();
    const host = normalizeHost(tab.url);
    const web = /^https?:/.test(tab.url || '');
    return {
      host,
      web,
      allowlisted: isAllowlisted(settings, host),
      blocked: web ? await blockedCount(tabId) : null,
      hiddenCount: settings.customHides[host]?.length || 0,
    };
  },

  async toggleSite({ host, allow }) {
    host = normalizeHost(host);
    if (!host) throw new Error('Bad address');
    await updateSettings((s) => {
      s.allowlist = s.allowlist.filter((d) => d !== host && !host.endsWith(`.${d}`));
      if (allow) s.allowlist.push(host);
      s.allowlist.sort();
    });
    await apply();
  },

  async proceed({ url }, sender) {
    if (!sender.url?.startsWith(BLOCKED_PAGE)) throw new Error('Not allowed');
    await allowProceed(url);
  },

  async startPicker({ tabId }) {
    await startPicker(tabId);
  },

  async saveHide({ selector }, sender) {
    const host = normalizeHost(sender.tab?.url);
    if (!host || !sender.tab) throw new Error('No page');
    if (typeof selector !== 'string' || !selector || selector.length > 1000 || /[{}<]/.test(selector)) {
      throw new Error('Bad selector');
    }
    await updateSettings((s) => {
      const list = s.customHides[host] || [];
      if (!list.includes(selector)) list.push(selector);
      s.customHides[host] = list;
    });
    await apply();
  },

  async clearHides({ host }) {
    host = normalizeHost(host);
    await updateSettings((s) => {
      delete s.customHides[host];
    });
    await apply();
  },
};

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (sender.id !== chrome.runtime.id) return false;
  const handler = handlers[message?.type];
  if (!handler) return false;
  handler(message, sender)
    .then((result) => sendResponse({ ok: true, result }))
    .catch((err) => sendResponse({ ok: false, error: String(err?.message || err) }));
  return true;
});

// ------------------------------------------------------------------ context menu

function buildMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: 'hide-element',
      title: 'Hide an element on this page',
      contexts: ['page', 'image', 'link', 'frame'],
      documentUrlPatterns: ['http://*/*', 'https://*/*'],
    });
  });
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === 'hide-element' && tab?.id != null) startPicker(tab.id).catch(() => {});
});

// ------------------------------------------------------------------ lifecycle

chrome.runtime.onInstalled.addListener(async ({ reason }) => {
  buildMenus();
  await apply();
  if (reason === 'install') chrome.runtime.openOptionsPage();
});

chrome.runtime.onStartup.addListener(() => {
  apply();
});

let pending = null;
chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== 'local' || !changes.settings) return;
  // Settings pages can save several times in a row. Apply once things settle.
  clearTimeout(pending);
  pending = setTimeout(apply, 150);
});
