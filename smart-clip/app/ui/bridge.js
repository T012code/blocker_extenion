// Talks to the app. Opened in a plain browser (for design work and the interface
// tests) it uses made-up data instead, so every screen can be seen without Windows.
const T = window.__TAURI__;

export const invoke = T ? T.core.invoke : (cmd, args) => mock(cmd, args);
export const listen = T ? T.event.listen : (name, fn) => {
  (listeners[name] ||= []).push(fn);
  return Promise.resolve(() => {});
};

// ---------------------------------------------------------------- made-up data
const listeners = {};
const emit = (name, payload) => (listeners[name] || []).forEach((fn) => fn({ payload }));
const params = new URLSearchParams(location.search);
const minutes = (m) => new Date(Date.now() - m * 60000).toISOString();
const thumb =
  'data:image/svg+xml;base64,' +
  btoa('<svg xmlns="http://www.w3.org/2000/svg" width="224" height="128"><defs><linearGradient id="g" x1="0" x2="1"><stop offset="0" stop-color="#3a7aff"/><stop offset="1" stop-color="#bef050"/></linearGradient></defs><rect width="224" height="128" fill="url(#g)"/><rect x="24" y="24" width="120" height="14" fill="#fff" opacity=".85"/><rect x="24" y="48" width="80" height="10" fill="#fff" opacity=".6"/></svg>');
const db = {
  status: {
    configured: params.get('state') !== 'setup',
    signed_in: !['signin', 'setup'].includes(params.get('state')),
    email: 'layo@example.com',
    online: params.get('state') !== 'offline',
    paused: params.get('state') === 'paused',
    message: params.get('state') === 'offline' ? "Can't reach Smart Clip's server. Check your internet connection." : null,
  },
  settings: {
    theme: 'system', float_icon: true, hidden_apps: ['explorer.exe', 'keepass.exe'], hotkey: 'Alt+Shift+V',
    sync_images: true, paused: params.get('state') === 'paused', device_id: 'x', device_name: 'LAYO-DESKTOP', first_run_done: true,
  },
  clips: params.get('state') === 'empty' ? [] : [
    { id: '1', kind: 'text', excerpt: 'https://layothedeveloper.com/projects/smart-clip', chars: 47, device_name: 'LAYO-LAPTOP', this_device: false, created_at: minutes(0.2) },
    { id: '2', kind: 'image', excerpt: '', chars: 0, device_name: 'LAYO-LAPTOP', this_device: false, created_at: minutes(6), thumb, width: 1920, height: 1080 },
    { id: '3', kind: 'text', excerpt: 'Meeting moved to Thursday at 3pm. Bring the printed budget and the signed contract, and remember the parking code is 4471.', chars: 121, device_name: 'LAYO-DESKTOP', this_device: true, created_at: minutes(42) },
    { id: '4', kind: 'text', excerpt: 'const sum = (a, b) => a + b;', chars: 28, device_name: 'OFFICE-PC', this_device: false, created_at: minutes(60 * 5) },
    { id: '5', kind: 'text', excerpt: '24 Marlborough Road, Manchester M14 6QJ', chars: 39, device_name: 'LAYO-DESKTOP', this_device: true, created_at: minutes(60 * 26) },
    { id: '6', kind: 'text', excerpt: 'Order #A-55102 confirmed', chars: 24, device_name: 'OFFICE-PC', this_device: false, created_at: minutes(60 * 24 * 3) },
  ],
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

async function mock(cmd, args = {}) {
  await wait(120);
  switch (cmd) {
    case 'app_info':
      return { version: '1.0.0', status: db.status, settings: db.settings, autostart: true, hotkeys: ['Alt+Shift+V', 'Ctrl+Shift+V', 'Super+Alt+V', 'Off'] };
    case 'send_code':
      if (!args.email.includes('@')) throw "That email address doesn't look right.";
      return null;
    case 'verify_code':
      if (args.code.replace(/\D/g, '') !== '123456') throw 'That code is wrong or has expired. Check it, or ask for a new one.';
      db.status = { ...db.status, signed_in: true, email: args.email };
      emit('status', db.status);
      return null;
    case 'sign_out':
      db.status = { ...db.status, signed_in: false, email: null };
      emit('status', db.status);
      return null;
    case 'list_clips':
      return db.clips;
    case 'copy_clip':
    case 'paste_clip':
      return null;
    case 'delete_clip':
      db.clips = db.clips.filter((c) => c.id !== args.id);
      emit('clips', db.clips);
      return null;
    case 'clear_clips':
      db.clips = [];
      emit('clips', db.clips);
      return null;
    case 'save_settings':
      db.settings = { ...db.settings, ...args.patch };
      if ('paused' in args.patch) {
        db.status = { ...db.status, paused: db.settings.paused };
        emit('status', db.status);
      }
      emit('settings', db.settings);
      return db.settings;
    case 'set_autostart':
      return args.on;
    default:
      return null;
  }
}
