import { api } from '../shared/api.js';
import { getSettings, saveSettings } from '../shared/settings.js';
import { DEFAULT_SETTINGS } from '../background/rules.js';

const form = document.getElementById('form');
const $ = (id) => document.getElementById(id);

function fill(settings) {
  for (const el of form.elements) {
    if (!el.name || !(el.name in settings)) continue;
    if (el.type === 'checkbox') el.checked = !!settings[el.name];
    else el.value = settings[el.name];
  }
}

function read() {
  const out = {};
  for (const el of form.elements) {
    if (!el.name) continue;
    out[el.name] = el.type === 'checkbox' ? el.checked : el.type === 'number' ? Math.max(0, Number(el.value) || 0) : el.value;
  }
  return out;
}

let timer;
function flash(text) {
  $('saved').textContent = text;
  clearTimeout(timer);
  timer = setTimeout(() => { $('saved').textContent = ''; }, 2000);
}

form.addEventListener('change', async () => {
  await saveSettings(read());
  flash('Saved');
});

$('reset-types').addEventListener('click', async () => {
  form.elements.fileTypes.value = DEFAULT_SETTINGS.fileTypes;
  await saveSettings({ fileTypes: DEFAULT_SETTINGS.fileTypes });
  flash('File types reset');
});

function showHost(host) {
  const badge = $('status-badge');
  if (host.installed) {
    badge.className = 'badge ok';
    badge.textContent = host.appRunning ? 'Connected' : 'Installed';
    $('status-title').textContent = host.appRunning ? 'Connected to Layó\'s Download Manager' : 'Layó\'s Download Manager is installed';
    $('status-detail').textContent = host.appRunning
      ? `App ${host.appVersion ?? ''}, bridge ${host.hostVersion ?? ''}`
      : 'It starts by itself when a download comes in.';
  } else {
    badge.className = 'badge bad';
    badge.textContent = 'Not installed';
    $('status-title').textContent = 'Layó\'s Download Manager is not installed';
    $('status-detail').textContent = 'Install the Windows app to use this extension. Until then the browser keeps handling downloads. '
      + (host.error ? `(${host.error})` : '');
  }
}

$('recheck').addEventListener('click', async () => {
  $('status-title').textContent = 'Checking…';
  $('status-badge').className = 'badge info';
  $('status-badge').textContent = 'Checking';
  showHost(await api.runtime.sendMessage({ type: 'refresh' }));
});

fill(await getSettings());
showHost((await api.runtime.sendMessage({ type: 'status' })).host);
