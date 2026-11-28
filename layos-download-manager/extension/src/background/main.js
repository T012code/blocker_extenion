// Background script for Layó's Download Manager.
// Listeners are registered straight away, as browsers require for service workers.

import { api } from '../shared/api.js';
import { getSettings, saveSettings } from '../shared/settings.js';
import * as capture from './capture.js';
import * as menus from './menus.js';
import * as native from './native.js';
import * as requests from './requests.js';
import * as media from './media.js';
import { describe } from './capture.js';
import { addHistory } from './history.js';
import { noteBypass } from './bypass.js';
import { clearHistory, getHistory } from './history.js';

requests.start();
capture.start();
menus.start();
media.start();

/** Replies to a content script with `{ ok, ...result }` or `{ ok: false, error }`. */
function answer(promise, reply) {
  promise.then((r) => reply({ ok: true, ...r }), (e) => reply({ ok: false, error: String(e?.message ?? e) }));
  return true;
}

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
    // ---- The on-video download button
    case 'videoChoices':
      reply({ choices: sender.tab ? media.localChoices(sender.tab.id, msg) : [] });
      return false;
    case 'siteFormats':
      return answer(
        native.getState().then((host) => {
          if (!host.installed) throw new Error('Install Layó\'s Download Manager to download videos from this site.');
          return media.siteChoices({ pageUrl: msg.pageUrl, playlistUrl: msg.playlistUrl, incognito: !!sender.tab?.incognito });
        }),
        reply,
      );
    case 'videoDownload':
      return answer(
        media.download({ action: msg.choice.action, title: msg.title, pageUrl: msg.pageUrl, incognito: !!sender.tab?.incognito }),
        reply,
      );
    // ---- Magnet links clicked on a page
    case 'magnet':
      return answer(
        describe({ url: msg.url, referrer: msg.pageUrl }).then(async (item) => {
          item.cookies = null;
          await native.send({ type: 'download', source: 'link', ask: null, items: [item] });
          const name = new URLSearchParams(msg.url.slice(msg.url.indexOf('?'))).get('dn') || 'Torrent';
          await addHistory({ name, url: msg.url, status: 'sent' });
          return {};
        }),
        reply,
      );
    case 'status':
      Promise.all([native.getState({ maxAgeMs: 0 }), getSettings(), getHistory()])
        .then(([host, settings, history]) => reply({ host, settings, history }));
      return true;
    case 'clearHistory':
      clearHistory().then(() => reply({ ok: true }));
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
