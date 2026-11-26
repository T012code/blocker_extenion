// Background script for Layó's Download Manager.
// Listeners are registered straight away, as browsers require for service workers.

import { api } from '../shared/api.js';
import { getSettings, saveSettings } from '../shared/settings.js';
import * as capture from './capture.js';
import * as menus from './menus.js';
import * as native from './native.js';
import * as requests from './requests.js';
import { noteBypass } from './bypass.js';
import { getHistory } from './history.js';

requests.start();
capture.start();
menus.start();

api.runtime.onInstalled.addListener(({ reason }) => {
  menus.create();
  native.refresh();
  if (reason === 'install') api.runtime.openOptionsPage();
});

api.runtime.onStartup.addListener(() => {
  menus.create();
  native.refresh();
});

api.runtime.onMessage.addListener((msg, sender, reply) => {
  // Only our own pages and content scripts.
  if (sender.id !== api.runtime.id) return false;
  switch (msg?.type) {
    case 'bypass':
      noteBypass(msg.url);
      return false;
    case 'status':
      Promise.all([native.getState({ maxAgeMs: 0 }), getSettings(), getHistory()])
        .then(([host, settings, history]) => reply({ host, settings, history }));
      return true;
    case 'refresh':
      native.refresh().then(reply);
      return true;
    case 'setEnabled':
      saveSettings({ enabled: !!msg.enabled }).then(reply);
      return true;
    case 'showApp':
      native.send({ type: 'showApp' }).then(() => reply({ ok: true }), (e) => reply({ ok: false, error: e.message }));
      return true;
    default:
      return false;
  }
});
