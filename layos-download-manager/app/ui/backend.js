// Talks to the Rust side of the app. In tests (?mock in the address) a simulated
// backend from mock.js stands in, so the window can be tried in any browser.

const mock = new URLSearchParams(location.search).has('mock') ? await import('./mock.js') : null;
const tauri = window.__TAURI__;

export function invoke(cmd, args = {}) {
  if (mock) return mock.invoke(cmd, args);
  return tauri.core.invoke(cmd, args);
}

export function listen(event, handler) {
  if (mock) return mock.listen(event, handler);
  return tauri.event.listen(event, (e) => handler(e.payload));
}

/** A native folder or file picker. Returns a path or null. */
export async function pick({ directory = false, title, filters } = {}) {
  if (mock) return mock.pick({ directory });
  const r = await invoke('plugin:dialog|open', { options: { directory, title, filters: filters ?? [], multiple: false } });
  return Array.isArray(r) ? r[0] ?? null : r ?? null;
}

export const isMock = !!mock;

/** This window's label: "main", "mini", or "dl-<id>" for a download's window. */
export function windowLabel() {
  if (mock) return new URLSearchParams(location.search).get('window') ?? 'main';
  return tauri.window.getCurrentWindow().label;
}
