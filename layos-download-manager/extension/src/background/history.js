// The last few downloads the extension handled, shown in the toolbar pop-up.

import { api } from '../shared/api.js';

const MAX = 15;

async function area() {
  // Session storage is cleared when the browser closes, which suits a short history.
  return api.storage.session ?? api.storage.local;
}

export async function addHistory(entry) {
  const store = await area();
  const { history = [] } = await store.get('history');
  history.unshift({ ...entry, time: Date.now() });
  await store.set({ history: history.slice(0, MAX) });
}

export async function getHistory() {
  const { history = [] } = await (await area()).get('history');
  return history;
}
