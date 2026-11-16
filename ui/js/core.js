'use strict';
// Shared helpers, the API client, the page registry and start-up. Loaded first.
const DEMO = new URLSearchParams(location.search).has('demo');
const $ = (s, el = document) => el.querySelector(s);
const esc = v => String(v ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const icon = (name, cls = 'i') => `<svg class="${cls}"><use href="#i-${name}"/></svg>`;
const now = () => Math.floor(Date.now() / 1000);

// ---------- formatting ----------
function bytes(n) {
  if (!n) return '0 B';
  const u = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const i = Math.min(u.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
  const v = n / 1024 ** i;
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
}
const count = n => Number(n || 0).toLocaleString();
function ago(t) {
  if (!t) return 'never';
  const s = now() - t;
  if (s < 5) return 'just now';
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return new Date(t * 1000).toLocaleDateString();
}
const clock = t => new Date(t * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

// ---------- toast ----------
let toastTimer;
function toast(msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 3200);
}

// ---------- api ----------
async function api(method, path, body) {
  if (DEMO) return demoApi(method, path, body);
  const res = await fetch(path, {
    method,
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (res.status === 204 || res.status === 202) return null;
  const data = await res.json().catch(() => null);
  if (!res.ok) throw new Error(data?.error || `Something went wrong (${res.status}).`);
  return data;
}

/** Runs an async action and shows any error as a toast. */
async function run(fn) {
  try { await fn(); } catch (err) { toast(err.message); }
}

// Two-step confirmation for destructive buttons: the first click arms, the second acts.
const armed = new Map();
function confirmClick(btn, label, act) {
  const key = btn.dataset.confirm;
  if (armed.has(key)) {
    clearTimeout(armed.get(key));
    armed.delete(key);
    act();
    return;
  }
  const old = btn.innerHTML;
  btn.classList.add('danger');
  btn.classList.remove('icon');
  btn.innerHTML = esc(label);
  armed.set(key, setTimeout(() => {
    armed.delete(key);
    btn.classList.remove('danger');
    if (btn.dataset.wasIcon) btn.classList.add('icon');
    btn.innerHTML = old;
  }, 4000));
}

// ---------- app-wide state ----------
let info = null;

function applyTheme() {
  const t = info?.settings.theme || 'system';
  if (t === 'system') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.dataset.theme = t;
}

async function loadInfo() {
  try {
    info = await api('GET', '/api/info');
  } catch {
    return;
  }
  applyTheme();
  $('#side-foot').textContent = `Version ${info.version}`;
  renderNav();
  renderUpdateBanner();
}

function renderUpdateBanner() {
  const el = $('#update-banner');
  const u = info?.update, offer = info?.update_offer;
  if (u?.downloading != null) {
    el.innerHTML = `<div class="banner">${icon('download')}<div class="grow"><b>Downloading the update…</b>
      <div class="meter"><i style="width:${Math.round(u.downloading * 100)}%"></i></div></div></div>`;
    return;
  }
  if (!offer) { el.innerHTML = ''; return; }
  el.innerHTML = `<div class="banner">${icon('download')}
    <div class="grow"><b>Version ${esc(offer.version)} is available.</b> <span class="muted">You have ${esc(info.version)}.</span>
    <a href="${esc(offer.page)}" target="_blank" rel="noopener">What's new</a></div>
    <div class="actions">
      <button class="btn ghost" data-act="update-skip">Skip this version</button>
      <button class="btn" data-act="update-later">Not now</button>
      <button class="btn primary" data-act="update-install">Update</button>
    </div></div>`;
}

$('#update-banner').addEventListener('click', async e => {
  const b = e.target.closest('[data-act]');
  if (!b) return;
  const act = b.dataset.act;
  try {
    if (act === 'update-install') {
      b.disabled = true;
      toast('Downloading the update…');
      const poll = setInterval(loadInfo, 700);
      try { await api('POST', '/api/update/install'); } finally { clearInterval(poll); }
      toast('Installing. Simple PC Tools will restart by itself.');
    } else if (act === 'update-later') {
      await api('POST', '/api/update/later');
      toast("OK. We'll ask again in a few days.");
    } else if (act === 'update-skip') {
      await api('POST', '/api/update/skip');
      toast("We won't ask about this version again.");
    }
  } catch (err) {
    toast(err.message);
    b.disabled = false;
  }
  if (DEMO && act !== 'update-install') { info.update_offer = null; renderUpdateBanner(); return; }
  loadInfo();
});

// ---------- pages ----------
// Each page script (ui/js/<id>.js) calls registerPage() once, in the order
// pages appear in the nav. Tool pages are listed by the server (info.tools,
// from src/tools/mod.rs); Settings is always last.
const PAGES = new Map(); // id -> { el, show }

/** Adds a page and returns its <section>, for the page script's event listeners. */
function registerPage(id, { show }) {
  const el = document.createElement('section');
  el.id = `page-${id}`;
  el.hidden = true;
  $('#pages').append(el);
  PAGES.set(id, { el, show });
  return el;
}

const toolInfo = id => info?.tools.find(t => t.id === id);
const pageOn = id => !info || id === 'settings' || !!toolInfo(id)?.on;
const pageShown = id => PAGES.get(id) && !PAGES.get(id).el.hidden;

let navKey = '';
function renderNav() {
  const tools = (info?.tools || []).filter(t => t.on && PAGES.has(t.id));
  const key = tools.map(t => t.id).join();
  if (key === navKey) return;
  navKey = key;
  document.querySelectorAll('#nav a[data-tool]').forEach(a => a.remove());
  $('#nav').insertAdjacentHTML('afterbegin', tools.map(t =>
    `<a href="#${t.id}" data-tool="${t.id}">${icon(t.id)}<span class="lbl">${esc(t.name)}</span><span class="count" id="nav-count-${t.id}" hidden></span></a>`).join(''));
  navCounts.forEach((n, id) => setNavCount(id, n));
}

/** Shows a red count on a tool's nav item (0 hides it). */
const navCounts = new Map();
function setNavCount(id, n) {
  navCounts.set(id, n);
  const el = $(`#nav-count-${id}`);
  if (!el) return;
  el.hidden = !n;
  el.textContent = n;
}

function route() {
  let page = location.hash.slice(1);
  if (!PAGES.has(page) || !pageOn(page)) page = [...PAGES.keys()].find(pageOn);
  for (const [id, p] of PAGES) p.el.hidden = id !== page;
  document.querySelectorAll('#nav a').forEach(a => {
    if (a.getAttribute('href') === `#${page}`) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });
  PAGES.get(page).show();
}
window.addEventListener('hashchange', route);

// ---------- start, once every page script has registered ----------
window.addEventListener('DOMContentLoaded', async () => {
  await loadInfo();
  route();
  setInterval(loadInfo, 60000);
});
