import { api } from './api.js';
import { DEFAULT_SETTINGS } from '../background/rules.js';

export async function getSettings() {
  const stored = await api.storage.local.get('settings');
  return { ...DEFAULT_SETTINGS, ...(stored.settings ?? {}) };
}

export async function saveSettings(patch) {
  const next = { ...(await getSettings()), ...patch };
  await api.storage.local.set({ settings: next });
  return next;
}
