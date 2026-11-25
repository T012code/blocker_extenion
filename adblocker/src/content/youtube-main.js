// Runs inside YouTube's own page context, before YouTube's scripts.
// YouTube sends its ad schedule inside the same data as the video itself
// (the "player response"). We remove the ad parts from that data so the player
// never learns there were ads to show. Nothing else in the data is touched.
(() => {
  'use strict';

  if (window.__layoYoutubeGuard) return;
  window.__layoYoutubeGuard = true;

  const AD_KEYS = ['adPlacements', 'adSlots', 'playerAds', 'adBreakHeartbeatParams'];

  function strip(obj) {
    if (!obj || typeof obj !== 'object') return false;
    let changed = false;
    for (const key of AD_KEYS) {
      if (Object.prototype.hasOwnProperty.call(obj, key)) {
        delete obj[key];
        changed = true;
      }
    }
    return changed;
  }

  function prune(data) {
    try {
      if (!data || typeof data !== 'object') return data;
      strip(data);
      if (data.playerResponse) strip(data.playerResponse);
      // Responses that bundle several parts come back as arrays.
      if (Array.isArray(data)) {
        for (const part of data) {
          if (part && typeof part === 'object') {
            strip(part);
            if (part.playerResponse) strip(part.playerResponse);
          }
        }
      }
    } catch {
      // Never break the page over this.
    }
    return data;
  }

  // 1. JSON.parse: YouTube parses most player data through it.
  const nativeParse = JSON.parse;
  JSON.parse = new Proxy(nativeParse, {
    apply(target, thisArg, args) {
      return prune(Reflect.apply(target, thisArg, args));
    },
  });

  // 2. fetch().json(): the player API responses (/youtubei/v1/player).
  const nativeJson = Response.prototype.json;
  Response.prototype.json = new Proxy(nativeJson, {
    apply(target, thisArg, args) {
      return Reflect.apply(target, thisArg, args).then(prune);
    },
  });

  // 3. Data embedded straight into the page HTML on first load.
  for (const name of ['ytInitialPlayerResponse', 'ytPlayerResponse']) {
    let value;
    try {
      Object.defineProperty(window, name, {
        configurable: true,
        get: () => value,
        set: (v) => {
          value = prune(v);
        },
      });
    } catch {
      // Already defined by the page; the other hooks still apply.
    }
  }
})();
