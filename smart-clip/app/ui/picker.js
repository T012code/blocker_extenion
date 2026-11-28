// The clip picker: opens beside the text box, pastes the clip picked.
// Keys: type to search, ↑/↓ to choose, Enter to paste, 1–9 to paste that one, Esc to close.
import { invoke, listen } from './bridge.js';
import { icons } from './icons.js';
import { ago, from } from './format.js';

const $ = (id) => document.getElementById(id);
const state = { clips: [], shown: [], selected: 0, busy: false };

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

function filtered() {
  const q = $('q').value.trim().toLowerCase();
  return q ? state.clips.filter((c) => c.excerpt.toLowerCase().includes(q) || from(c).toLowerCase().includes(q) || (c.kind === 'image' && 'image picture screenshot'.includes(q))) : state.clips;
}

function render() {
  state.shown = filtered();
  state.selected = Math.min(state.selected, Math.max(0, state.shown.length - 1));
  const list = $('list');
  list.replaceChildren(
    ...state.shown.map((c, i) => {
      const li = el('li', 'pk-item');
      li.id = `clip-${i}`;
      li.setAttribute('role', 'option');
      li.setAttribute('aria-selected', String(i === state.selected));
      li.append(el('span', 'key', i < 9 ? String(i + 1) : ''));
      const body = el('div', 'body');
      if (c.kind === 'image' && c.thumb) {
        const img = el('img', 'thumb');
        img.src = c.thumb;
        img.alt = 'Image';
        body.append(img);
      } else {
        body.append(el('span', 'excerpt', c.excerpt));
      }
      const meta = el('div', 'meta');
      const size = c.kind === 'image' && c.width ? ` · ${c.width}×${c.height}` : '';
      meta.append(el('span', 'from', `${from(c)}${size} · ${ago(c.created_at)}`));
      body.append(meta);
      li.append(body);
      li.addEventListener('mousemove', () => select(i, false));
      li.addEventListener('click', () => paste(i));
      return li;
    }),
  );
  list.setAttribute('aria-activedescendant', state.shown.length ? `clip-${state.selected}` : '');
  const empty = $('empty');
  empty.hidden = state.shown.length > 0;
  list.hidden = !state.shown.length;
  if (!state.shown.length) {
    empty.replaceChildren(
      el('strong', null, state.clips.length ? 'Nothing matches' : 'No clips yet'),
      el('span', null, state.clips.length ? 'Try other words.' : 'Copy something on any of your PCs and it shows up here.'),
    );
  }
}

function select(i, scroll = true) {
  if (i === state.selected || i < 0 || i >= state.shown.length) return;
  $(`clip-${state.selected}`)?.setAttribute('aria-selected', 'false');
  state.selected = i;
  const li = $(`clip-${i}`);
  li?.setAttribute('aria-selected', 'true');
  $('list').setAttribute('aria-activedescendant', `clip-${i}`);
  if (scroll) li?.scrollIntoView({ block: 'nearest' });
}

function message(text) {
  const foot = $('foot');
  if (!text) return foot.querySelector('.msg')?.remove();
  let m = foot.querySelector('.msg');
  if (!m) {
    m = el('span', 'msg');
    foot.prepend(m);
  }
  m.textContent = text;
}

async function paste(i) {
  const c = state.shown[i];
  if (!c || state.busy) return;
  state.busy = true;
  $(`clip-${i}`)?.classList.add('busy');
  message('');
  try {
    await invoke('paste_clip', { id: c.id });
  } catch (err) {
    message(String(err));
  } finally {
    state.busy = false;
    $(`clip-${i}`)?.classList.remove('busy');
  }
}

document.addEventListener('keydown', (e) => {
  if (e.key === 'ArrowDown') {
    e.preventDefault();
    select(Math.min(state.selected + 1, state.shown.length - 1));
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    select(Math.max(state.selected - 1, 0));
  } else if (e.key === 'Enter') {
    e.preventDefault();
    paste(state.selected);
  } else if (e.key === 'Escape') {
    e.preventDefault();
    invoke('close_picker');
  } else if (/^[1-9]$/.test(e.key) && !$('q').value && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    paste(Number(e.key) - 1);
  }
});

$('q').addEventListener('input', () => {
  state.selected = 0;
  render();
});
$('btn-open').addEventListener('click', () => invoke('show_app'));

async function refresh() {
  $('q').value = '';
  state.selected = 0;
  message('');
  state.clips = await invoke('list_clips');
  render();
  $('q').focus();
}

$('search-icon').outerHTML = icons.search(16);
$('btn-open').innerHTML = icons.open(16);
listen('picker-open', refresh);
listen('clips', ({ payload }) => {
  state.clips = payload;
  render();
});
listen('settings', ({ payload }) => window.smartTheme.set(payload.theme));
refresh();
