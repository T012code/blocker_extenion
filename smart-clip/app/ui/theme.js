// Light, dark or the system's choice, for every window. Loaded as a plain script in
// <head> so a window never flashes in the wrong colours. The choice lives in the app's
// settings; this copy in localStorage (shared by the app's windows) applies it before
// anything draws, and follows a change made in Settings at once.
(() => {
  const KEY = 'smartclip-theme';
  const apply = (t) => {
    if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t;
    else delete document.documentElement.dataset.theme;
  };
  let saved = null;
  try { saved = localStorage.getItem(KEY); } catch { /* storage off */ }
  apply(saved);
  addEventListener('storage', (e) => { if (e.key === KEY) apply(e.newValue); });
  window.smartTheme = {
    apply,
    set(t) {
      apply(t);
      try { localStorage.setItem(KEY, t || 'system'); } catch { /* storage off */ }
    },
  };
})();
