// The window's layout: the side bar and the details panel can be folded away
// (Ctrl+B, Ctrl+J) or dragged to any size. Sizes are remembered.

const KEY = 'ldm.layout.v1';
const SIDEBAR = { min: 220, max: 420, def: 232, rail: 56 };
const DETAILS = { min: 140, def: 248, bar: 48 };

function load() {
  try {
    return { sidebar: SIDEBAR.def, details: DETAILS.def, sidebarCollapsed: false, detailsCollapsed: false, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') };
  } catch {
    return { sidebar: SIDEBAR.def, details: DETAILS.def, sidebarCollapsed: false, detailsCollapsed: false };
  }
}

export function initLayout() {
  const app = document.getElementById('app');
  const main = document.getElementById('main');
  const s = load();
  // Sizes saved by an older version (or a bad value) are brought within limits.
  s.sidebar = Math.round(Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, Number(s.sidebar) || SIDEBAR.def)));
  s.details = Math.round(Math.max(DETAILS.min, Number(s.details) || DETAILS.def));
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(s)); } catch { /* fine */ } };

  const detailsMax = () => Math.max(DETAILS.min, main.clientHeight - 180);
  const apply = () => {
    app.style.setProperty('--sidebar-w', `${s.sidebar}px`);
    app.style.setProperty('--details-h', `${Math.min(s.details, detailsMax())}px`);
    app.classList.toggle('sidebar-collapsed', s.sidebarCollapsed);
    app.classList.toggle('details-collapsed', s.detailsCollapsed);
    const sb = document.getElementById('btn-sidebar');
    sb.setAttribute('aria-expanded', String(!s.sidebarCollapsed));
    sb.title = s.sidebarCollapsed ? 'Show the side bar (Ctrl+B)' : 'Hide the side bar (Ctrl+B)';
    sb.querySelector('.label').textContent = s.sidebarCollapsed ? 'Show side bar' : 'Hide side bar';
    const split = document.getElementById('split-sidebar');
    split.setAttribute('aria-valuenow', String(s.sidebarCollapsed ? SIDEBAR.rail : s.sidebar));
    split.setAttribute('aria-valuemin', String(SIDEBAR.rail));
    split.setAttribute('aria-valuemax', String(SIDEBAR.max));
    for (const id of ['btn-details', 'details-toggle']) {
      const b = document.getElementById(id);
      b.setAttribute('aria-expanded', String(!s.detailsCollapsed));
      b.title = s.detailsCollapsed ? 'Show the details panel (Ctrl+J)' : 'Hide the details panel (Ctrl+J)';
    }
    document.getElementById('details-body').inert = s.detailsCollapsed;
  };

  const toggleSidebar = (on = s.sidebarCollapsed) => { s.sidebarCollapsed = !on; apply(); save(); };
  const toggleDetails = (on = s.detailsCollapsed) => { s.detailsCollapsed = !on; apply(); save(); };

  // Dragging the side bar's edge. Dragging a folded bar open (or a bar shut) works too.
  drag(document.getElementById('split-sidebar'), 'x', () => (s.sidebarCollapsed ? SIDEBAR.rail : s.sidebar), (w, key) => {
    if (key && s.sidebarCollapsed && w > SIDEBAR.rail) { s.sidebarCollapsed = false; }
    else if (w < (SIDEBAR.rail + SIDEBAR.min) / 2) { s.sidebarCollapsed = true; }
    else { s.sidebarCollapsed = false; s.sidebar = Math.round(Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, w))); }
    apply();
  }, save);
  // Dragging the line above the details panel (upwards makes it taller).
  drag(document.getElementById('split-details'), 'y', () => s.details, (h, key) => {
    if (h < (key ? DETAILS.min : DETAILS.bar + 24)) { s.detailsCollapsed = true; }
    else { s.detailsCollapsed = false; s.details = Math.round(Math.min(detailsMax(), Math.max(DETAILS.min, h))); }
    apply();
  }, save, -1);

  document.getElementById('btn-sidebar').addEventListener('click', () => toggleSidebar());
  document.getElementById('btn-details').addEventListener('click', () => toggleDetails());
  document.getElementById('details-toggle').addEventListener('click', () => toggleDetails());
  // Double-click a splitter to fold that side, or unfold it.
  document.getElementById('split-sidebar').addEventListener('dblclick', () => toggleSidebar());
  addEventListener('keydown', (e) => {
    if (!(e.ctrlKey || e.metaKey) || e.target.closest('input, select, textarea, dialog')) return;
    const k = e.key.toLowerCase();
    if (k === 'b') { e.preventDefault(); toggleSidebar(); }
    else if (k === 'j') { e.preventDefault(); toggleDetails(); }
  });
  addEventListener('resize', apply);
  // Saved sizes apply at once on start, without animating from the defaults.
  document.body.classList.add('layout-init');
  apply();
  requestAnimationFrame(() => requestAnimationFrame(() => document.body.classList.remove('layout-init')));
  return { toggleSidebar, toggleDetails, state: s };
}

/**
 * Pointer and keyboard resizing for a splitter. `get` gives the current size, `set` applies a
 * new one, `dir` is -1 when moving the pointer up/left should grow the panel.
 */
function drag(el, axis, get, set, done, dir = 1) {
  el.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const start = axis === 'x' ? e.clientX : e.clientY;
    const from = get();
    el.setPointerCapture(e.pointerId);
    el.classList.add('dragging');
    document.body.classList.add('resizing');
    if (axis === 'y') document.body.classList.add('rows');
    const move = (ev) => set(from + dir * ((axis === 'x' ? ev.clientX : ev.clientY) - start));
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
      el.classList.remove('dragging');
      document.body.classList.remove('resizing', 'rows');
      done();
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  });
  el.addEventListener('keydown', (e) => {
    const grow = axis === 'x' ? 'ArrowRight' : 'ArrowUp';
    const shrink = axis === 'x' ? 'ArrowLeft' : 'ArrowDown';
    if (e.key !== grow && e.key !== shrink) return;
    e.preventDefault();
    set(get() + (e.key === grow ? 1 : -1) * (e.shiftKey ? 64 : 16), true);
    done();
  });
}
