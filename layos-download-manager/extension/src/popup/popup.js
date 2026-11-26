import { api } from '../shared/api.js';

const $ = (id) => document.getElementById(id);

function showStatus(host, settings) {
  const dot = $('status-dot');
  if (!host.installed) {
    dot.className = 'dot bad';
    $('status-title').textContent = 'Download manager not found';
    $('status-detail').textContent = 'Install Layó\'s Download Manager for Windows. Until then, downloads stay in the browser.';
  } else if (!settings.enabled) {
    dot.className = 'dot warn';
    $('status-title').textContent = 'Paused';
    $('status-detail').textContent = 'The browser is handling downloads itself.';
  } else {
    dot.className = 'dot ok';
    $('status-title').textContent = 'Ready';
    $('status-detail').textContent = host.appRunning
      ? `Connected to version ${host.appVersion ?? '?'}`
      : 'The app starts when a download comes in.';
  }
  $('open-app').disabled = !host.installed;
}

function showHistory(history) {
  if (!history.length) return;
  const list = $('history');
  list.replaceChildren(...history.map((h) => {
    const li = document.createElement('li');
    const name = document.createElement('span');
    name.className = 'name';
    name.textContent = h.name || h.url;
    name.title = h.reason ? `${h.url}\n${h.reason}` : h.url;
    const tag = document.createElement('span');
    tag.className = `tag ${h.status}`;
    tag.textContent = h.status === 'sent' ? 'Sent' : 'In browser';
    li.append(name, tag);
    return li;
  }));
}

async function load() {
  const { host, settings, history } = await api.runtime.sendMessage({ type: 'status' });
  $('enabled').checked = settings.enabled;
  const key = { alt: 'Alt', shift: 'Shift', ctrl: 'Ctrl' }[settings.bypassKey];
  $('bypass-hint').textContent = key ? `Hold ${key} while clicking a link to skip` : 'Only file types in your list';
  showStatus(host, settings);
  showHistory(history);
}

$('enabled').addEventListener('change', async (e) => {
  await api.runtime.sendMessage({ type: 'setEnabled', enabled: e.target.checked });
  load();
});
$('options').addEventListener('click', () => api.runtime.openOptionsPage());
$('open-app').addEventListener('click', async () => {
  const r = await api.runtime.sendMessage({ type: 'showApp' });
  if (r?.ok) window.close();
  else $('status-detail').textContent = r?.error ?? 'Could not open the app.';
});

load();
