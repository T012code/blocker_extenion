import {
  DEFAULTS, getSettings, saveSettings, updateSettings, normalizeHost,
} from '../shared/settings.js';

const $ = (id) => document.getElementById(id);

function toast(text, isError = false) {
  const el = $('toast');
  el.textContent = text;
  el.classList.toggle('error', isError);
  el.classList.add('show');
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => el.classList.remove('show'), 2400);
}

function getPath(obj, path) {
  return path.split('.').reduce((o, k) => o?.[k], obj);
}

function setPath(obj, path, value) {
  const keys = path.split('.');
  const last = keys.pop();
  keys.reduce((o, k) => o[k], obj)[last] = value;
}

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  Object.assign(node, props);
  for (const child of [].concat(children)) node.append(child);
  return node;
}

function emptyItem(text) {
  return el('li', { className: 'empty', textContent: text });
}

// ---------------------------------------------------------------- render

async function render() {
  const settings = await getSettings();

  $('enabled').checked = settings.enabled;
  $('master-hint').textContent = settings.enabled
    ? 'Blocking is on.'
    : 'Paused. Nothing is blocked until you turn this back on.';
  document.body.toggleAttribute('data-paused', !settings.enabled);

  for (const input of document.querySelectorAll('[data-setting]')) {
    input.checked = Boolean(getPath(settings, input.dataset.setting));
  }

  renderAllowlist(settings);
  renderHidden(settings);
}

function renderAllowlist(settings) {
  const list = $('allow-list');
  list.replaceChildren();
  if (!settings.allowlist.length) {
    list.append(emptyItem('No trusted sites yet.'));
    return;
  }
  for (const host of settings.allowlist) {
    const remove = el('button', { className: 'btn danger', type: 'button', textContent: 'Remove' });
    remove.setAttribute('aria-label', `Remove ${host}`);
    remove.addEventListener('click', async () => {
      await updateSettings((s) => {
        s.allowlist = s.allowlist.filter((d) => d !== host);
      });
      toast(`${host} removed`);
    });
    list.append(el('li', {}, [el('span', { className: 'item-main', textContent: host }), remove]));
  }
}

function renderHidden(settings) {
  const list = $('hidden-list');
  list.replaceChildren();
  const sites = Object.keys(settings.customHides).sort();
  if (!sites.length) {
    list.append(emptyItem('Nothing hidden yet.'));
    return;
  }
  for (const host of sites) {
    const count = settings.customHides[host].length;
    const main = el('span', { className: 'item-main' }, [
      host,
      el('span', { className: 'item-sub', textContent: `${count} ${count === 1 ? 'element' : 'elements'} hidden` }),
    ]);
    main.title = settings.customHides[host].join('\n');
    const clear = el('button', { className: 'btn danger', type: 'button', textContent: 'Show again' });
    clear.setAttribute('aria-label', `Show hidden elements on ${host} again`);
    clear.addEventListener('click', async () => {
      await updateSettings((s) => {
        delete s.customHides[host];
      });
      toast(`Elements on ${host} will show again`);
    });
    list.append(el('li', {}, [main, clear]));
  }
}

async function renderCounts() {
  try {
    const stats = await (await fetch(chrome.runtime.getURL('rules/stats.json'))).json();
    for (const node of document.querySelectorAll('[data-count]')) {
      const value = stats[node.dataset.count];
      node.textContent = typeof value === 'number' ? value.toLocaleString() : 'many';
    }
  } catch {
    for (const node of document.querySelectorAll('[data-count]')) node.textContent = 'many';
  }
}

async function checkPrivacyControl() {
  const checks = [
    ['WebRTC', chrome.privacy.network.webRTCIPHandlingPolicy],
    ['hyperlink auditing', chrome.privacy.websites.hyperlinkAuditingEnabled],
    ['third-party cookies', chrome.privacy.websites.thirdPartyCookiesAllowed],
    ['network prediction', chrome.privacy.network.networkPredictionEnabled],
  ];
  const blocked = [];
  for (const [name, setting] of checks) {
    try {
      const { levelOfControl } = await setting.get({});
      if (levelOfControl === 'not_controllable' || levelOfControl === 'controlled_by_other_extensions') {
        blocked.push(name);
      }
    } catch {
      blocked.push(name);
    }
  }
  const note = $('privacy-note');
  note.hidden = !blocked.length;
  note.textContent = blocked.length
    ? `Your browser, your organisation or another extension is in charge of: ${blocked.join(', ')}. Those switches may have no effect.`
    : '';
}

// ---------------------------------------------------------------- events

$('enabled').addEventListener('change', async (event) => {
  const enabled = event.target.checked;
  await updateSettings((s) => {
    s.enabled = enabled;
  });
  toast(enabled ? 'Protection is on' : 'Protection paused');
});

for (const input of document.querySelectorAll('[data-setting]')) {
  input.addEventListener('change', async () => {
    await updateSettings((s) => setPath(s, input.dataset.setting, input.checked));
    toast('Saved');
  });
}

$('allow-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const input = $('allow-input');
  const error = $('allow-error');
  const host = normalizeHost(input.value);
  if (!host) {
    input.setAttribute('aria-invalid', 'true');
    error.hidden = false;
    error.textContent = 'Enter a site address such as example.com';
    input.focus();
    return;
  }
  input.removeAttribute('aria-invalid');
  error.hidden = true;
  await updateSettings((s) => {
    if (!s.allowlist.includes(host)) s.allowlist.push(host);
    s.allowlist.sort();
  });
  input.value = '';
  toast(`${host} added`);
});

$('allow-input').addEventListener('input', () => {
  $('allow-input').removeAttribute('aria-invalid');
  $('allow-error').hidden = true;
});

$('export').addEventListener('click', async () => {
  const settings = await getSettings();
  const blob = new Blob([JSON.stringify({ app: 'layos-advanced-blocker', version: 1, settings }, null, 2)], {
    type: 'application/json',
  });
  const url = URL.createObjectURL(blob);
  const a = el('a', { href: url, download: `layos-blocker-settings-${new Date().toISOString().slice(0, 10)}.json` });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});

$('import').addEventListener('click', () => $('import-file').click());

$('import-file').addEventListener('change', async (event) => {
  const file = event.target.files?.[0];
  event.target.value = '';
  if (!file) return;
  try {
    const data = JSON.parse(await file.text());
    const incoming = data?.settings;
    if (data?.app !== 'layos-advanced-blocker' || !incoming || typeof incoming !== 'object') {
      throw new Error('not ours');
    }
    // Clean up anything that came from the file before trusting it.
    incoming.allowlist = (Array.isArray(incoming.allowlist) ? incoming.allowlist : [])
      .map(normalizeHost).filter(Boolean);
    const hides = {};
    for (const [site, list] of Object.entries(incoming.customHides || {})) {
      const host = normalizeHost(site);
      const clean = (Array.isArray(list) ? list : [])
        .filter((s) => typeof s === 'string' && s.length <= 1000 && !/[{}<]/.test(s));
      if (host && clean.length) hides[host] = clean;
    }
    incoming.customHides = hides;
    await saveSettings(incoming);
    // Re-read through the defaults so unknown or missing keys are fixed up.
    await saveSettings(await getSettings());
    toast('Settings imported');
  } catch {
    toast('That file is not a settings export from this extension.', true);
  }
});

$('reset').addEventListener('click', async () => {
  if (!confirm('Reset every setting, trusted site and hidden element?')) return;
  await saveSettings(structuredClone(DEFAULTS));
  toast('Everything is back to the defaults');
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'local' && changes.settings) render();
});

// Highlight the section in view in the side menu.
const links = [...document.querySelectorAll('nav a')];
const observer = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    for (const link of links) {
      link.setAttribute('aria-current', String(link.getAttribute('href') === `#${entry.target.id}`));
    }
  }
}, { rootMargin: '-20% 0px -70% 0px' });
for (const link of links) {
  const section = document.querySelector(link.getAttribute('href'));
  if (section) observer.observe(section);
}

$('version').textContent = `Version ${chrome.runtime.getManifest().version}`;
render();
renderCounts();
checkPrivacyControl();
