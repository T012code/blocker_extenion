// Tells the extension when a link is clicked with the bypass key held (Alt by default),
// so that download stays in the browser. Runs in every page; does nothing else.
(() => {
  const api = globalThis.browser ?? globalThis.chrome;
  let key = 'alt';
  api.storage.local.get('settings').then((s) => { key = s.settings?.bypassKey ?? 'alt'; }).catch(() => {});
  api.storage.onChanged.addListener((changes) => {
    if (changes.settings?.newValue) key = changes.settings.newValue.bypassKey ?? 'alt';
  });

  const held = (e) => (key === 'alt' && e.altKey) || (key === 'shift' && e.shiftKey) || (key === 'ctrl' && (e.ctrlKey || e.metaKey));

  const onPress = (e) => {
    if (key === 'none' || !held(e)) return;
    const link = e.composedPath?.().find((n) => n instanceof Element && n.matches?.('a[href], area[href]'));
    if (!link?.href) return;
    try {
      api.runtime.sendMessage({ type: 'bypass', url: link.href });
    } catch { /* extension reloaded */ }
  };
  // Magnet links open in Layó's Download Manager (unless the bypass key is held).
  let magnets = true;
  let hostReady = false;
  api.storage.local.get(['settings', 'hostReady']).then((s) => {
    magnets = (s.settings?.enabled ?? true) && (s.settings?.captureMagnets ?? true);
    hostReady = !!s.hostReady;
  }).catch(() => {});
  api.storage.onChanged.addListener((c) => {
    if (c.settings?.newValue) magnets = c.settings.newValue.enabled !== false && c.settings.newValue.captureMagnets !== false;
    if (c.hostReady) hostReady = !!c.hostReady.newValue;
  });
  addEventListener('click', (e) => {
    if (!magnets || !hostReady || e.defaultPrevented || e.button !== 0 || held(e)) return;
    const link = e.composedPath?.().find((n) => n instanceof Element && n.matches?.('a[href]'));
    const href = link?.href ?? '';
    if (!/^magnet:\?/i.test(href)) return;
    e.preventDefault();
    api.runtime.sendMessage({ type: 'magnet', url: href, pageUrl: location.href }).then((r) => {
      // If the app couldn't take it, let the system's torrent app have it.
      if (!r?.ok) location.href = href;
    }).catch(() => { location.href = href; });
  }, true);

  // mousedown comes before the click starts the download; keydown covers Enter on a focused link.
  addEventListener('mousedown', onPress, true);
  addEventListener('keydown', (e) => { if (e.key === 'Enter') onPress(e); }, true);
})();
