import { getSettings, updateSettings } from '../shared/settings.js';

const $ = (id) => document.getElementById(id);
let tab = null;
let info = null;

function send(type, data = {}) {
  return chrome.runtime.sendMessage({ type, ...data }).then((res) => {
    if (!res?.ok) throw new Error(res?.error || 'Something went wrong');
    return res.result;
  });
}

function toast(text, isError = false) {
  const el = $('toast');
  el.textContent = text;
  el.classList.toggle('error', isError);
  el.classList.add('show');
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => el.classList.remove('show'), 2200);
}

function getPath(obj, path) {
  return path.split('.').reduce((o, k) => o?.[k], obj);
}

function setPath(obj, path, value) {
  const keys = path.split('.');
  const last = keys.pop();
  keys.reduce((o, k) => o[k], obj)[last] = value;
}

function renderSite(settings) {
  const site = $('site');
  const button = $('toggle-site');
  const count = info?.blocked;
  $('count').textContent = count == null ? '–' : String(count);

  if (!info?.web || !info.host) {
    site.dataset.state = 'unsupported';
    $('host').textContent = 'Browser page';
    $('status').textContent = 'The browser does not let extensions run on this page.';
    button.hidden = true;
    return;
  }

  $('host').textContent = info.host;
  button.hidden = false;
  button.disabled = !settings.enabled;

  if (!settings.enabled) {
    site.dataset.state = 'paused';
    $('status').textContent = 'Protection is paused everywhere.';
    button.textContent = 'Paused';
  } else if (info.allowlisted) {
    site.dataset.state = 'allowlisted';
    $('status').textContent = 'Turned off on this site.';
    button.textContent = 'Turn protection back on';
    button.classList.add('primary');
  } else {
    site.dataset.state = 'on';
    $('status').textContent = 'Protection is on.';
    button.textContent = 'Turn off on this site';
    button.classList.remove('primary');
  }
}

async function render() {
  const settings = await getSettings();
  $('enabled').checked = settings.enabled;
  for (const input of document.querySelectorAll('[data-setting]')) {
    input.checked = Boolean(getPath(settings, input.dataset.setting));
    input.disabled = !settings.enabled;
  }
  document.querySelector('.quick').setAttribute('aria-disabled', String(!settings.enabled));
  $('picker').disabled = !info?.web || !settings.enabled;
  renderSite(settings);
}

async function init() {
  [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  try {
    info = await send('tabInfo', { tabId: tab.id });
  } catch {
    info = { web: false };
  }
  await render();
}

$('enabled').addEventListener('change', async (event) => {
  const enabled = event.target.checked;
  await updateSettings((s) => {
    s.enabled = enabled;
  });
  await render();
  toast(enabled ? 'Protection is on' : 'Protection paused');
});

for (const input of document.querySelectorAll('[data-setting]')) {
  input.addEventListener('change', async () => {
    await updateSettings((s) => setPath(s, input.dataset.setting, input.checked));
    toast('Saved. Reload the page to see the change.');
  });
}

$('toggle-site').addEventListener('click', async () => {
  const button = $('toggle-site');
  button.setAttribute('aria-busy', 'true');
  try {
    await send('toggleSite', { host: info.host, allow: !info.allowlisted });
    info.allowlisted = !info.allowlisted;
    await chrome.tabs.reload(tab.id);
    await render();
  } catch (err) {
    toast(err.message, true);
  } finally {
    button.removeAttribute('aria-busy');
  }
});

$('picker').addEventListener('click', async () => {
  try {
    await send('startPicker', { tabId: tab.id });
    window.close();
  } catch {
    toast('This page does not allow hiding elements.', true);
  }
});

$('settings').addEventListener('click', () => {
  chrome.runtime.openOptionsPage();
  window.close();
});

init();
