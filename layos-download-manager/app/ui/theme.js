// Light, dark or the system's choice, for every window of the app. Loaded as a plain
// script in <head> so the page never flashes in the wrong colours. The choice is kept in
// the settings; this copy in localStorage (shared by all the app's windows) lets each
// window apply it before anything draws, and follow a change made in Settings at once.
(() => {
  const KEY = 'ldm-theme';
  const apply = (t) => {
    if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t;
    else delete document.documentElement.dataset.theme;
  };
  let saved = null;
  try { saved = localStorage.getItem(KEY); } catch { /* storage off */ }
  apply(saved);
  addEventListener('storage', (e) => { if (e.key === KEY) apply(e.newValue); });
  window.ldmTheme = {
    apply,
    /** Applies the theme here and in the app's other windows. */
    set(t) {
      apply(t);
      try { localStorage.setItem(KEY, t ?? 'system'); } catch { /* storage off */ }
    },
  };
})();
