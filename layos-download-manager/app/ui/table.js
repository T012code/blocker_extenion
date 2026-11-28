// The download list: columns you can show, hide, sort and resize, with rows updated in
// place (only cells whose content changed are touched), so hundreds of downloads
// stay smooth while their progress ticks several times a second.

import { STATUS_TEXT, bytes, eta, percent, speed } from './format.js';
import { icon } from './icons.js';

const PREFS_KEY = 'ldm.table.v1';

/** How each status looks: icon and colour group (red failed, green done, yellow held, neutral otherwise). */
export const STATUS_LOOK = {
  downloading: { icon: 'downloading', tone: 'neutral' },
  connecting: { icon: 'connecting', tone: 'neutral', spin: true },
  queued: { icon: 'queued', tone: 'neutral' },
  completed: { icon: 'done', tone: 'good' },
  seeding: { icon: 'seeding', tone: 'good' },
  paused: { icon: 'paused', tone: 'held' },
  stopped: { icon: 'stop-circle', tone: 'neutral' },
  blocked: { icon: 'vpn', tone: 'held' },
  needsRefresh: { icon: 'expired', tone: 'held' },
  failed: { icon: 'failed', tone: 'bad' },
};

export function statusEl(d, { withText = true } = {}) {
  const look = STATUS_LOOK[d.status] ?? STATUS_LOOK.queued;
  const s = document.createElement('span');
  s.className = `status tone-${look.tone}${look.spin ? ' spin' : ''}`;
  s.append(icon(look.icon, { size: 16 }));
  if (withText) s.append(Object.assign(document.createElement('span'), { textContent: STATUS_TEXT[d.status] ?? d.status }));
  return s;
}

const TYPE_ICONS = { video: 'video', music: 'music', documents: 'documents', compressed: 'compressed', programs: 'programs', general: 'general' };

function typeOf(d) {
  if (d.kind?.type === 'torrent') return { label: 'Torrent', icon: 'torrents' };
  if (d.kind?.type === 'media') return { label: 'Video', icon: 'video' };
  const ext = d.fileName.includes('.') ? d.fileName.split('.').pop().slice(0, 5).toUpperCase() : 'File';
  return { label: ext, icon: TYPE_ICONS[d.category] ?? 'general' };
}

const when = (ms) => {
  if (!ms) return '';
  const t = new Date(ms);
  const today = new Date();
  const time = t.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  return t.toDateString() === today.toDateString() ? `Today ${time}` : `${t.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })} ${time}`;
};

const host = (url) => { try { return new URL(url).host; } catch { return ''; } };

/**
 * Columns. `text` gives the cell's text (also used to skip unchanged cells), `sort` a sort key.
 * `ctx` carries progressOf(d), queueName(id) and isActive(status).
 */
export const COLUMNS = [
  { key: 'type', label: 'Type', width: 92, min: 56, text: (d) => typeOf(d).label, sort: (d) => typeOf(d).label,
    render: (td, d) => {
      const t = typeOf(d);
      const s = document.createElement('span');
      s.className = 'type';
      s.append(icon(t.icon), t.label);
      td.replaceChildren(s);
    } },
  { key: 'name', label: 'Name', width: 320, min: 80, text: (d) => d.fileName, sort: (d) => d.fileName.toLowerCase() },
  { key: 'size', label: 'Size', width: 92, align: 'end', text: (d, c) => (c.progressOf(d).size ? bytes(c.progressOf(d).size) : ''), sort: (d, c) => c.progressOf(d).size ?? -1 },
  { key: 'progress', label: 'Progress', width: 200, min: 72, sort: (d, c) => fraction(d, c),
    text: (d, c) => String(Math.round(fraction(d, c) * 1000)) + d.status,
    render: (td, d, c) => {
      if (!td.firstChild) td.innerHTML = '<div class="progress"><div class="bar"><span></span></div><span class="pct"></span></div>';
      const f = fraction(d, c);
      td.querySelector('.bar span').style.setProperty('--p', String(f));
      const pct = d.status === 'completed' ? 100 : percent(c.progressOf(d).done, c.progressOf(d).size);
      td.querySelector('.pct').textContent = pct != null ? `${pct}%` : '';
    } },
  { key: 'downloaded', label: 'Downloaded', width: 110, align: 'end', text: (d, c) => bytes(c.progressOf(d).done), sort: (d, c) => c.progressOf(d).done },
  { key: 'speed', label: 'Speed', width: 100, align: 'end', text: (d, c) => (d.status === 'downloading' ? speed(c.progressOf(d).speed) : ''), sort: (d, c) => (d.status === 'downloading' ? c.progressOf(d).speed : -1) },
  { key: 'eta', label: 'Time left', width: 100, align: 'end', text: (d, c) => (d.status === 'downloading' ? eta((c.progressOf(d).size ?? 0) - c.progressOf(d).done, c.progressOf(d).speed) : ''),
    sort: (d, c) => { const p = c.progressOf(d); return d.status === 'downloading' && p.speed > 0 && p.size ? (p.size - p.done) / p.speed : Infinity; } },
  { key: 'status', label: 'Status', width: 168, text: (d) => d.status + (d.error ?? ''), sort: (d) => STATUS_ORDER.indexOf(d.status),
    render: (td, d, c, changed) => {
      const s = statusEl(d);
      if (changed) s.classList.add('changed');
      td.replaceChildren(s);
      td.title = d.error && d.status !== 'completed' ? d.error : '';
    } },
  { key: 'added', label: 'Added', width: 136, text: (d) => when(d.createdAt), sort: (d) => d.createdAt ?? 0 },
  { key: 'finished', label: 'Finished', width: 136, text: (d) => when(d.completedAt), sort: (d) => d.completedAt ?? 0 },
  { key: 'sections', label: 'Sections', width: 88, align: 'end', text: (d) => (d.kind?.type === 'http' || !d.kind ? String(d.connections) : ''), sort: (d) => d.connections },
  { key: 'queue', label: 'Queue', width: 120, text: (d, c) => c.queueName(d.queueId), sort: (d, c) => c.queueName(d.queueId) },
  { key: 'folder', label: 'Folder', width: 220, text: (d) => d.saveDir, sort: (d) => d.saveDir },
  { key: 'address', label: 'Address', width: 200, text: (d) => host(d.request?.url ?? ''), sort: (d) => host(d.request?.url ?? '') },
];
const STATUS_ORDER = ['downloading', 'connecting', 'seeding', 'blocked', 'queued', 'paused', 'stopped', 'needsRefresh', 'failed', 'completed'];
export const DEFAULT_VISIBLE = ['type', 'name', 'size', 'progress', 'speed', 'eta', 'status'];

function fraction(d, c) {
  if (d.status === 'completed') return 1;
  const p = c.progressOf(d);
  return p.size > 0 ? Math.min(1, p.done / p.size) : 0;
}

const ALL_KEYS = COLUMNS.map((c) => c.key);
const MIN_WIDTH = 44;

function loadPrefs() {
  try {
    const p = JSON.parse(localStorage.getItem(PREFS_KEY) ?? 'null');
    if (p && Array.isArray(p.visible)) {
      // Column order: saved order, then any columns added since.
      const order = (Array.isArray(p.order) ? p.order : []).filter((k) => ALL_KEYS.includes(k));
      p.order = [...order, ...ALL_KEYS.filter((k) => !order.includes(k))];
      return p;
    }
  } catch { /* storage unavailable: defaults */ }
  return { visible: [...DEFAULT_VISIBLE], order: [...ALL_KEYS], widths: {}, sort: { key: null, dir: 1 } };
}

export class DownloadTable {
  constructor({ table, onChange }) {
    this.table = table;
    this.colgroup = table.querySelector('colgroup');
    this.headRow = table.querySelector('thead tr');
    this.tbody = table.querySelector('tbody');
    this.onChange = onChange;
    this.prefs = loadPrefs();
    this.rows = new Map();
    this.firstPaint = true;
    this.tbody.addEventListener('animationend', (e) => {
      if (e.target.tagName === 'TR') e.target.classList.remove('enter');
    });
    this.buildHead();
  }

  save() {
    try { localStorage.setItem(PREFS_KEY, JSON.stringify(this.prefs)); } catch { /* fine */ }
  }

  /** Shown columns, in the user's order. Name is always there. */
  get columns() {
    return this.prefs.order
      .map((k) => COLUMNS.find((c) => c.key === k))
      .filter((c) => c && (this.prefs.visible.includes(c.key) || c.key === 'name'));
  }

  width(col) {
    return this.prefs.widths[col.key] ?? col.width;
  }

  /** Every column keeps exactly its width; an empty last column takes any space left. */
  sizeTable() {
    this.table.style.width = `${this.columns.reduce((n, c) => n + this.width(c), 0)}px`;
  }

  /** Header cells, column widths, sort arrows, resize handles and dragging to reorder. */
  buildHead(resetRows = true) {
    const cols = this.columns;
    this.colgroup.replaceChildren(...cols.map((c) => {
      const col = document.createElement('col');
      col.style.width = `${this.width(c)}px`;
      return col;
    }), document.createElement('col'));
    this.sizeTable();
    const filler = document.createElement('th');
    filler.className = 'filler';
    filler.setAttribute('aria-hidden', 'true');
    this.headRow.replaceChildren(...cols.map((c) => {
      const th = document.createElement('th');
      th.scope = 'col';
      th.dataset.key = c.key;
      if (c.align === 'end') th.classList.add('end');
      const sorted = this.prefs.sort.key === c.key;
      th.setAttribute('aria-sort', sorted ? (this.prefs.sort.dir > 0 ? 'ascending' : 'descending') : 'none');
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'th-button';
      b.append(Object.assign(document.createElement('span'), { className: 'th-label', textContent: c.label }));
      if (sorted) b.append(icon(this.prefs.sort.dir > 0 ? 'arrow-up' : 'arrow-down', { size: 14 }));
      b.title = `Sort by ${c.label.toLowerCase()}, or drag to move`;
      b.addEventListener('click', (e) => {
        if (this.justDragged) { e.preventDefault(); this.justDragged = false; return; }
        this.sortBy(c.key);
      });
      b.addEventListener('pointerdown', (e) => this.startMove(e, c, th));
      b.addEventListener('keydown', (e) => {
        if (e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
          e.preventDefault();
          const at = this.columns.indexOf(c);
          this.moveColumn(c.key, at + (e.key === 'ArrowRight' ? 1 : -1));
          this.headRow.querySelector(`th[data-key="${c.key}"] .th-button`)?.focus();
        }
      });
      const grip = document.createElement('span');
      grip.className = 'col-resize';
      grip.setAttribute('role', 'separator');
      grip.setAttribute('aria-orientation', 'vertical');
      grip.setAttribute('aria-label', `Resize ${c.label} column`);
      grip.setAttribute('aria-description', 'Drag, or use the arrow keys. Double-click to reset.');
      grip.tabIndex = 0;
      grip.addEventListener('pointerdown', (e) => this.startResize(e, c));
      grip.addEventListener('keydown', (e) => {
        if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
          e.preventDefault();
          this.setWidth(c, this.width(c) + (e.key === 'ArrowRight' ? 16 : -16));
        }
      });
      grip.addEventListener('dblclick', () => { delete this.prefs.widths[c.key]; this.save(); this.buildHead(false); this.resizeCols(); });
      th.append(b, grip);
      return th;
    }), filler);
    if (!resetRows) return;
    // Rows must be rebuilt to match the columns (without the entrance animation).
    this.tbody.replaceChildren();
    this.rows.clear();
    this.firstPaint = true;
  }

  resizeCols() {
    this.columns.forEach((c, i) => { this.colgroup.children[i].style.width = `${this.width(c)}px`; });
    this.sizeTable();
  }

  setWidth(col, w) {
    this.prefs.widths[col.key] = Math.max(col.min ?? MIN_WIDTH, Math.min(800, Math.round(w)));
    this.save();
    this.resizeCols();
  }

  startResize(e, col) {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = this.width(col);
    const grip = e.currentTarget;
    grip.setPointerCapture(e.pointerId);
    document.body.classList.add('resizing-x');
    const move = (ev) => this.setWidth(col, startW + ev.clientX - startX);
    const up = () => {
      grip.removeEventListener('pointermove', move);
      grip.removeEventListener('pointerup', up);
      grip.removeEventListener('pointercancel', up);
      document.body.classList.remove('resizing-x');
    };
    grip.addEventListener('pointermove', move);
    grip.addEventListener('pointerup', up);
    grip.addEventListener('pointercancel', up);
  }

  /** Moves a column to position `to` among the shown columns. */
  moveColumn(key, to) {
    const shown = this.columns.map((c) => c.key);
    const from = shown.indexOf(key);
    to = Math.max(0, Math.min(shown.length - 1, to));
    if (from < 0 || from === to) return;
    shown.splice(from, 1);
    shown.splice(to, 0, key);
    // Hidden columns keep their places relative to the others.
    const hidden = this.prefs.order.filter((k) => !shown.includes(k));
    this.prefs.order = [...shown, ...hidden];
    this.save();
    this.buildHead();
    this.onChange();
  }

  /** Dragging a header left or right moves its column; a short press still sorts. */
  startMove(e, col, th) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const button = e.currentTarget;
    let dragging = false;
    let marker = null;
    let to = 0;
    const heads = () => [...this.headRow.querySelectorAll('th[data-key]')];
    // Follow the pointer even once it leaves the header (a plain click still sorts).
    button.setPointerCapture(e.pointerId);
    const move = (ev) => {
      const dx = ev.clientX - startX;
      if (!dragging) {
        if (Math.abs(dx) < 6) return;
        dragging = true;
        th.classList.add('moving');
        document.body.classList.add('moving-col');
        marker = document.createElement('div');
        marker.className = 'col-drop';
        this.table.closest('.list').append(marker);
      }
      th.style.transform = `translateX(${dx}px)`;
      // Where it would land: before the first header whose middle is right of the pointer.
      const all = heads();
      const others = all.filter((h) => h !== th);
      let before = others.findIndex((h) => { const r = h.getBoundingClientRect(); return ev.clientX < r.left + r.width / 2; });
      if (before < 0) before = others.length;
      to = before;
      const list = this.table.closest('.list');
      const lr = list.getBoundingClientRect();
      const edge = before < others.length ? others[before].getBoundingClientRect().left : others[others.length - 1].getBoundingClientRect().right;
      marker.style.left = `${edge - lr.left + list.scrollLeft - 1}px`;
      marker.style.top = `${list.scrollTop}px`;
      marker.style.height = `${list.clientHeight}px`;
    };
    const up = () => {
      button.removeEventListener('pointermove', move);
      button.removeEventListener('pointerup', up);
      button.removeEventListener('pointercancel', up);
      if (!dragging) return;
      this.justDragged = true;
      setTimeout(() => { this.justDragged = false; }, 0);
      th.classList.remove('moving');
      th.style.transform = '';
      marker?.remove();
      document.body.classList.remove('moving-col');
      this.moveColumn(col.key, to);
    };
    button.addEventListener('pointermove', move);
    button.addEventListener('pointerup', up);
    button.addEventListener('pointercancel', up);
  }

  sortBy(key) {
    const s = this.prefs.sort;
    if (s.key !== key) Object.assign(s, { key, dir: 1 });
    else if (s.dir === 1) s.dir = -1;
    else Object.assign(s, { key: null, dir: 1 }); // third click: back to newest first
    this.save();
    this.buildHead(false);
    this.onChange();
  }

  toggleColumn(key) {
    if (key === 'name') return;
    const v = this.prefs.visible;
    this.prefs.visible = v.includes(key) ? v.filter((k) => k !== key) : [...v, key];
    this.save();
    this.buildHead();
    this.onChange();
  }

  resetColumns() {
    this.prefs = { visible: [...DEFAULT_VISIBLE], order: [...ALL_KEYS], widths: {}, sort: { key: null, dir: 1 } };
    this.save();
    this.buildHead();
    this.onChange();
  }

  /** Sorted copy of `list` (newest first when no column is chosen). */
  sort(list, ctx) {
    const { key, dir } = this.prefs.sort;
    const col = COLUMNS.find((c) => c.key === key);
    if (!col) return [...list].sort((a, b) => b.id - a.id);
    return [...list].sort((a, b) => {
      const x = col.sort(a, ctx);
      const y = col.sort(b, ctx);
      const c = typeof x === 'string' ? x.localeCompare(y) : x < y ? -1 : x > y ? 1 : 0;
      return c * dir || b.id - a.id;
    });
  }

  /** Brings the rows in line with `list`: creates, updates changed cells, reorders, removes. */
  update(list, ctx) {
    const cols = this.columns;
    const keep = new Set();
    let prev = null;
    for (const d of list) {
      keep.add(d.id);
      let tr = this.rows.get(d.id);
      if (!tr) {
        tr = document.createElement('tr');
        tr.dataset.id = String(d.id);
        for (const c of cols) {
          const td = document.createElement('td');
          if (c.align === 'end') td.classList.add('end');
          tr.append(td);
        }
        tr.append(Object.assign(document.createElement('td'), { className: 'filler' }));
        if (!this.firstPaint) tr.classList.add('enter');
        this.rows.set(d.id, tr);
      }
      const cls = `${d.status}${ctx.selected.has(d.id) ? ' selected' : ''}${tr.classList.contains('enter') ? ' enter' : ''}`;
      if (d.id === ctx.cursor) tr.dataset.cursor = '1'; else delete tr.dataset.cursor;
      if (tr.className !== cls) tr.className = cls;
      tr.setAttribute('aria-selected', ctx.selected.has(d.id) ? 'true' : 'false');
      cols.forEach((c, i) => {
        const td = tr.children[i];
        const sig = c.text(d, ctx);
        if (td.dataset.sig === sig) return;
        const changed = td.dataset.sig !== undefined;
        td.dataset.sig = sig;
        if (c.render) c.render(td, d, ctx, changed);
        else { td.textContent = sig; td.title = c.key === 'name' || c.key === 'folder' ? sig : ''; }
      });
      // Keep DOM order equal to list order, moving only rows that are out of place.
      const want = prev ? prev.nextSibling : this.tbody.firstChild;
      if (want !== tr) this.tbody.insertBefore(tr, want);
      prev = tr;
    }
    for (const [id, tr] of this.rows) {
      if (!keep.has(id)) { tr.remove(); this.rows.delete(id); }
    }
    this.firstPaint = false;
  }

  /** Ids in the order shown. */
  ids() {
    return [...this.tbody.children].map((r) => Number(r.dataset.id));
  }
}
