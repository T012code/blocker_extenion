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
  // mousedown comes before the click starts the download; keydown covers Enter on a focused link.
  addEventListener('mousedown', onPress, true);
  addEventListener('keydown', (e) => { if (e.key === 'Enter') onPress(e); }, true);
})();
