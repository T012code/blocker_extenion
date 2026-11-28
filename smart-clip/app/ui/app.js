// The main window: sign in, the clips from every PC, and settings.
import { invoke, listen } from './bridge.js';
import { icons } from './icons.js';
import { ago, from, hotkeyLabel } from './format.js';

const $ = (id) => document.getElementById(id);
const state = { info: null, clips: [], filter: 'all', query: '', view: null, email: '' };

// ---------------------------------------------------------------- small helpers
let toastTimer;
function toast(text) {
  const t = $('toast');
  t.textContent = text;
  t.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 2200);
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

/** Shows a button as working ("Sending…"), then puts its words back. */
function busy(button, on, label) {
  if (on) {
    button.dataset.label = button.textContent;
    if (label) button.textContent = label;
  } else if (button.dataset.label) {
    button.textContent = button.dataset.label;
  }
  button.disabled = on;
}

// ---------------------------------------------------------------- views
function show(view) {
  state.view = view;
  for (const v of ['setup', 'signin', 'clips', 'settings']) $(`view-${v}`).hidden = v !== view;
  const signedIn = view === 'clips' || view === 'settings';
  $('btn-settings').hidden = view !== 'clips';
  $('btn-back').hidden = view !== 'settings';
  $('title').textContent = view === 'settings' ? 'Settings' : 'Smart Clip';
  $('status').hidden = !signedIn || view === 'settings';
  $('main').scrollTop = 0;
}

function pickView() {
  const s = state.info.status;
  if (!s.configured) return show('setup');
  if (!s.signed_in) {
    show('signin');
    return;
  }
  if (state.view !== 'settings') show('clips');
}

function renderStatus() {
  const s = state.info.status;
  const tag = $('status');
  if (s.paused) {
    tag.className = 'tag off';
    tag.textContent = 'Paused';
  } else if (s.online) {
    tag.className = 'tag ok';
    tag.textContent = 'Syncing';
  } else {
    tag.className = 'tag warn';
    tag.textContent = 'Offline';
  }
  const banner = $('banner');
  banner.replaceChildren();
  if (s.paused) {
    banner.insertAdjacentHTML('beforeend', icons.pause(16));
    banner.append(el('span', null, 'Syncing is paused. New copies stay on this PC.'));
    const b = el('button', 'btn', 'Resume');
    b.type = 'button';
    b.onclick = () => saveSettings({ paused: false });
    banner.append(b);
    banner.hidden = false;
  } else if (s.message && s.signed_in) {
    banner.insertAdjacentHTML('beforeend', icons.cloudOff(16));
    banner.append(el('span', null, s.message));
    banner.hidden = false;
  } else {
    banner.hidden = true;
  }
}

// ---------------------------------------------------------------- sign in
let resendTimer;
function startResendCountdown(seconds = 60) {
  const b = $('btn-resend');
  clearInterval(resendTimer);
  let left = seconds;
  const tick = () => {
    b.disabled = left > 0;
    b.textContent = left > 0 ? `Send a new code (${left}s)` : 'Send a new code';
    left -= 1;
    if (left < -1) clearInterval(resendTimer);
  };
  tick();
  resendTimer = setInterval(tick, 1000);
}

async function sendCode(email) {
  await invoke('send_code', { email });
  state.email = email.trim().toLowerCase();
  $('sent-to').textContent = state.email;
  $('form-email').hidden = true;
  $('form-code').hidden = false;
  $('code').value = '';
  $('code-error').textContent = '';
  $('code').focus();
  startResendCountdown();
}

$('form-email').addEventListener('submit', async (e) => {
  e.preventDefault();
  const button = $('btn-send');
  $('email-error').textContent = '';
  busy(button, true, 'Sending…');
  try {
    await sendCode($('email').value);
  } catch (err) {
    $('email-error').textContent = String(err);
  } finally {
    busy(button, false);
  }
});

$('form-code').addEventListener('submit', async (e) => {
  e.preventDefault();
  const button = $('btn-verify');
  $('code-error').textContent = '';
  busy(button, true, 'Signing in…');
  try {
    await invoke('verify_code', { email: state.email, code: $('code').value });
    toast('Signed in. Your clips will show up here.');
  } catch (err) {
    $('code-error').textContent = String(err);
    $('code').select();
  } finally {
    busy(button, false);
  }
});

// Sign in as soon as the last digit is typed (or a code is pasted).
$('code').addEventListener('input', () => {
  const digits = $('code').value.replace(/\D/g, '');
  if (digits.length === 6 && !$('btn-verify').disabled) $('form-code').requestSubmit();
});

$('btn-resend').addEventListener('click', async () => {
  $('code-error').textContent = '';
  try {
    await sendCode(state.email);
    toast('A new code is on its way.');
  } catch (err) {
    $('code-error').textContent = String(err);
  }
});

$('btn-other').addEventListener('click', () => {
  clearInterval(resendTimer);
  $('form-code').hidden = true;
  $('form-email').hidden = false;
  $('email').focus();
  $('email').select();
});

// ---------------------------------------------------------------- clips
function visibleClips() {
  const q = state.query.trim().toLowerCase();
  return state.clips.filter((c) => (state.filter === 'all' || c.kind === state.filter) && (!q || c.excerpt.toLowerCase().includes(q) || from(c).toLowerCase().includes(q)));
}

function meta(c) {
  const m = el('div', 'meta');
  m.insertAdjacentHTML('beforeend', c.kind === 'image' ? icons.image(13) : icons.pc(13));
  const parts = [from(c), ago(c.created_at)];
  if (c.kind === 'image' && c.width) parts.splice(1, 0, `${c.width}×${c.height}`);
  parts.forEach((p, i) => {
    if (i) m.append(el('span', 'sep', '·'));
    m.append(el('span', i === 0 ? 'from' : null, p));
  });
  return m;
}

async function copy(c) {
  try {
    await invoke('copy_clip', { id: c.id });
    toast(c.kind === 'image' ? 'Image copied. Paste it with Ctrl+V.' : 'Copied. Paste it with Ctrl+V.');
  } catch (err) {
    toast(String(err));
  }
}

function renderClips() {
  const list = $('clips');
  const items = visibleClips();
  list.replaceChildren(
    ...items.map((c) => {
      const li = el('li', 'clip');
      const main = el('button', 'clip-main');
      main.type = 'button';
      main.title = 'Copy';
      if (c.kind === 'image' && c.thumb) {
        const img = el('img', 'thumb');
        img.src = c.thumb;
        img.alt = 'Image';
        main.append(img);
      } else {
        main.append(el('span', 'excerpt', c.excerpt));
      }
      main.append(meta(c));
      main.onclick = () => copy(c);
      const actions = el('div', 'clip-actions');
      const cp = el('button', 'btn ghost icon');
      cp.type = 'button';
      cp.title = 'Copy';
      cp.setAttribute('aria-label', 'Copy');
      cp.innerHTML = icons.copy(16);
      cp.onclick = () => copy(c);
      const del = el('button', 'btn ghost icon del');
      del.type = 'button';
      del.title = 'Delete from all PCs';
      del.setAttribute('aria-label', 'Delete from all PCs');
      del.innerHTML = icons.trash(16);
      del.onclick = async () => {
        try {
          await invoke('delete_clip', { id: c.id });
        } catch (err) {
          toast(String(err));
        }
      };
      actions.append(cp, del);
      li.append(main, actions);
      return li;
    }),
  );
  const empty = $('empty');
  empty.hidden = items.length > 0;
  if (!items.length) {
    empty.replaceChildren();
    if (state.clips.length) {
      empty.append(el('h2', null, 'Nothing matches'), el('p', null, 'Try other words, or show all clips.'));
    } else {
      const hotkey = state.info.settings.hotkey;
      empty.append(el('h2', null, 'No clips yet'), el('p', null, 'Copy something on any PC signed in with your email. It shows up here in a moment.'));
      const ways = el('div', 'ways');
      const a = el('div');
      const icon = el('img', 'float-sample');
      icon.src = 'brand/app-icon.png';
      icon.alt = '';
      a.append(icon, el('span', null, 'Click the Smart Clip icon in any text box to paste'));
      ways.append(a);
      if (hotkey !== 'Off') {
        const b = el('div');
        hotkeyLabel(hotkey).split('+').forEach((k, i) => {
          if (i) b.append('+');
          b.append(el('kbd', null, k));
        });
        b.append(el('span', null, 'opens your clips anywhere'));
        ways.append(b);
      }
      empty.append(ways);
    }
  }
}

$('search').addEventListener('input', (e) => {
  state.query = e.target.value;
  renderClips();
});

for (const b of document.querySelectorAll('[data-filter]')) {
  b.addEventListener('click', () => {
    state.filter = b.dataset.filter;
    for (const o of document.querySelectorAll('[data-filter]')) o.setAttribute('aria-pressed', String(o === b));
    renderClips();
  });
}

// ---------------------------------------------------------------- settings
async function saveSettings(patch) {
  try {
    state.info.settings = await invoke('save_settings', { patch });
    renderSettings();
  } catch (err) {
    toast(String(err));
  }
}

function renderSettings() {
  const s = state.info.settings;
  $('set-email').textContent = state.info.status.email || '';
  if (document.activeElement !== $('set-device')) $('set-device').value = s.device_name;
  $('set-float').checked = s.float_icon;
  $('set-paused').checked = s.paused;
  $('set-images').checked = s.sync_images;
  $('set-autostart').checked = state.info.autostart;
  const sel = $('set-hotkey');
  if (!sel.options.length) {
    for (const h of state.info.hotkeys) sel.append(new Option(hotkeyLabel(h), h));
  }
  sel.value = s.hotkey;
  for (const b of $('set-theme').querySelectorAll('button')) b.setAttribute('aria-pressed', String(b.dataset.theme === s.theme));
  window.smartTheme.set(s.theme);
  const chips = $('hidden-apps');
  chips.replaceChildren();
  if (s.float_icon) {
    for (const app of s.hidden_apps) {
      const chip = el('span', 'chip', app);
      const x = el('button');
      x.type = 'button';
      x.title = `Show the icon in ${app} again`;
      x.setAttribute('aria-label', `Show the icon in ${app} again`);
      x.innerHTML = icons.close(12);
      x.onclick = () => saveSettings({ hidden_apps: s.hidden_apps.filter((a) => a !== app) });
      chip.append(x);
      chips.append(chip);
    }
    if (s.hidden_apps.length) chips.prepend(el('span', 'note', 'Hidden in:'));
  }
  $('version').textContent = `Version ${state.info.version}.`;
}

$('set-device').addEventListener('change', (e) => saveSettings({ device_name: e.target.value }));
$('set-float').addEventListener('change', (e) => saveSettings({ float_icon: e.target.checked }));
$('set-paused').addEventListener('change', (e) => saveSettings({ paused: e.target.checked }));
$('set-images').addEventListener('change', (e) => saveSettings({ sync_images: e.target.checked }));
$('set-hotkey').addEventListener('change', (e) => saveSettings({ hotkey: e.target.value }));
$('set-theme').addEventListener('click', (e) => {
  const t = e.target.closest('button')?.dataset.theme;
  if (t) saveSettings({ theme: t });
});
$('set-autostart').addEventListener('change', async (e) => {
  try {
    state.info.autostart = await invoke('set_autostart', { on: e.target.checked });
  } catch (err) {
    toast(String(err));
  }
  renderSettings();
});
$('btn-signout').addEventListener('click', async () => {
  await invoke('sign_out');
  state.view = null;
});

function resetClear() {
  const box = $('clear-box');
  box.replaceChildren();
  const b = el('button', 'btn danger', 'Delete all');
  b.type = 'button';
  b.id = 'btn-clear';
  b.onclick = askClear;
  box.append(b);
}
function askClear() {
  const box = $('clear-box');
  box.replaceChildren();
  const yes = el('button', 'btn danger solid', 'Delete everywhere');
  yes.type = 'button';
  const no = el('button', 'btn', 'Cancel');
  no.type = 'button';
  yes.onclick = async () => {
    try {
      await invoke('clear_clips');
      toast('All clips deleted.');
    } catch (err) {
      toast(String(err));
    }
    resetClear();
  };
  no.onclick = resetClear;
  box.append(no, yes);
  no.focus();
}
$('btn-clear').addEventListener('click', askClear);

$('btn-settings').addEventListener('click', () => {
  renderSettings();
  show('settings');
});
$('btn-back').addEventListener('click', () => show('clips'));
$('credit').addEventListener('click', () => invoke('open_site'));
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && state.view === 'settings') show('clips');
});

// ---------------------------------------------------------------- start
$('btn-settings').innerHTML = icons.settings(18);
$('btn-back').innerHTML = icons.back(18);
$('search-icon').outerHTML = icons.search(16);

async function start() {
  state.info = await invoke('app_info');
  window.smartTheme.set(state.info.settings.theme);
  state.clips = await invoke('list_clips');
  renderStatus();
  pickView();
  renderClips();
  if (state.view === 'signin') $('email').focus();

  listen('status', async ({ payload }) => {
    const was = state.info.status.signed_in;
    state.info.status = payload;
    renderStatus();
    if (was !== payload.signed_in) {
      state.view = null;
      if (!payload.signed_in) {
        $('form-code').hidden = true;
        $('form-email').hidden = false;
      }
      pickView();
      state.clips = await invoke('list_clips');
      renderClips();
    }
  });
  listen('clips', ({ payload }) => {
    state.clips = payload;
    renderClips();
  });
  listen('settings', ({ payload }) => {
    state.info.settings = payload;
    renderSettings();
    renderClips();
  });
  // Keep "5 min ago" honest.
  setInterval(() => state.view === 'clips' && renderClips(), 60000);
}
start();
