'use strict';
// Resources page: what's using memory, processor and network, by program, with
// a stacked memory chart and a port lookup. API: /api/resources
// (src/tools/resources/mod.rs).
const resourcesPage = registerPage('resources', { show: showResources });
const LAYERS = 7; // programs drawn as their own layer; colours --c1 … --c7
const WINDOW_MS = 15 * 60 * 1000;
const LIST_SHOWN = 40;
const res = {
  now: null, history: [], last: 0, names: new Map(), slots: new Map(),
  ports: [], portsAt: 0, sort: 'mem', query: '', open: null, procsOpen: null,
  showAll: false, listenOnly: true, paused: false, hoverList: false, order: [], timer: null,
};

// ---------- data ----------
async function tickResources() {
  clearTimeout(res.timer);
  if (!pageShown('resources')) return;
  try {
    const d = await api('GET', `/api/resources?since=${res.last}`);
    if (d.t) mergeResources(d);
    if (Date.now() - res.portsAt > 5000) {
      res.portsAt = Date.now();
      res.ports = (await api('GET', '/api/resources/ports')).ports;
    }
  } catch (e) {
    toast(e.message);
  }
  if (!pageShown('resources')) return;
  if (!res.paused) renderResourcesLive();
  res.timer = setTimeout(tickResources, res.now ? 1000 : 300);
}

function mergeResources(d) {
  res.now = d;
  for (const g of d.groups) res.names.set(g.id, g.name);
  for (const [id, name] of Object.entries(d.names || {})) res.names.set(Number(id), name);
  res.history.push(...d.history);
  res.last = d.t;
  const cut = d.t - WINDOW_MS;
  while (res.history.length && res.history[0].t < cut) res.history.shift();
  // Keep each big program's colour while it stays in the top few.
  const top = [...d.groups].sort((a, b) => b.mem - a.mem).slice(0, LAYERS).map(g => g.id);
  for (const id of res.slots.keys()) if (!top.includes(id)) res.slots.delete(id);
  for (const id of top) {
    if (res.slots.has(id)) continue;
    const used = new Set(res.slots.values());
    res.slots.set(id, [1, 2, 3, 4, 5, 6, 7].find(s => !used.has(s)));
  }
}

const group = id => res.now?.groups.find(g => g.id === id);
const ioText = n => n >= 1024 ? rate(n) : '–';
const rate = n => `${bytes(n)}/s`;
const pct = n => `${n < 10 && n > 0 ? n.toFixed(1) : Math.round(n)}%`;
const swatch = id => res.slots.has(id) ? `<i class="sw k${res.slots.get(id)}"></i>` : '<i class="sw"></i>';
const groupPorts = id => res.ports.filter(p => p.group === id);

// ---------- page ----------
function renderResources() {
  resourcesPage.innerHTML = `
    <div class="head"><div class="titles"><h1>Resources</h1>
      <p class="sub">What's using your PC right now. Point at the chart to see which program is which.</p></div>
      <div class="actions"><button class="btn" id="res-pause">${icon(res.paused ? 'play' : 'pause')}${res.paused ? 'Resume' : 'Pause'}</button></div></div>
    <div id="res-banner"></div>
    <div class="stats" id="res-stats"></div>
    <div class="card chart-card">
      <div class="card-head"><h2>Memory</h2><span class="muted" id="mem-sum"></span></div>
      <div class="chart" id="mem-chart"><div class="chart-y faint" id="mem-max"></div>
        <svg viewBox="0 0 1000 200" preserveAspectRatio="none" aria-hidden="true"></svg>
        <div class="guide" hidden></div><div class="tip" hidden></div></div>
      <div class="chart-x faint"><span id="mem-span"></span><span>now</span></div>
      <div class="legend" id="mem-legend"></div>
    </div>
    <div class="card progs">
      <div class="toolbar">
        <input class="input" id="res-q" type="search" placeholder="Find a program, or type a port number" aria-label="Find a program or port" title="Shortcut: / or Ctrl+F" autocomplete="off" spellcheck="false" value="${esc(res.query)}">
        <span class="faint count" id="res-count" aria-live="polite"></span>
        <div class="seg" role="group" aria-label="Sort by">${[['mem', 'Memory'], ['cpu', 'Processor'], ['io', 'Activity']].map(([k, l]) =>
          `<button data-sort="${k}" aria-pressed="${res.sort === k}">${l}</button>`).join('')}</div>
      </div>
      <div id="res-hit"></div>
      <div id="res-list"></div>
    </div>
    <details class="adv res-adv" id="res-adv"><summary>${icon('chevron')}Advanced: system details and network ports</summary><div class="inner" id="res-adv-body"></div></details>`;
  renderResourcesLive();
}

function renderResourcesLive() {
  const d = res.now;
  if (!d) {
    $('#res-list').innerHTML = '<div class="scan-state"><span class="spinner"></span><span class="muted">Reading what\'s running…</span></div>';
    return;
  }
  const s = d.system;
  $('#res-banner').innerHTML = s.elevated ? '' : `<div class="banner warn">${icon('alert')}<div class="grow">Running without administrator rights, so Windows hides some programs' details. Their memory is counted under "Windows and system".</div></div>`;
  const h = res.history;
  const busiest = [...d.groups].sort((a, b) => b.cpu - a.cpu)[0];
  $('#res-stats').innerHTML = [
    stat('Processor', pct(s.cpu), busiest && busiest.cpu >= 1 ? `Busiest: ${esc(busiest.name)}` : 'Mostly idle', h.map(x => x.cpu), 100),
    stat('Memory', bytes(s.mem_used), `of ${bytes(s.mem_total)} (${Math.round(s.mem_used / s.mem_total * 100)}%)`, h.map(x => x.used), s.mem_total),
    stat('Network', `↓ ${rate(s.net_rx)}`, `↑ ${rate(s.net_tx)}`, h.map(x => x.rx + x.tx), Math.max(1, ...h.map(x => x.rx + x.tx))),
  ].join('');
  $('#mem-sum').textContent = `${bytes(s.mem_used)} of ${bytes(s.mem_total)} in use`;
  drawChart();
  renderList();
  if ($('#res-adv').open) renderAdvanced();
}

function stat(label, value, sub, series, max) {
  return `<div class="card stat"><div class="k">${label}</div><div class="v">${value}</div><div class="faint">${sub}</div>${spark(series, max)}</div>`;
}

function spark(series, max) {
  if (series.length < 2) return '<svg class="spark" viewBox="0 0 100 30" preserveAspectRatio="none"></svg>';
  const pts = series.map((v, i) => `${(i / (series.length - 1) * 100).toFixed(2)},${(30 - Math.min(1, v / max) * 28).toFixed(2)}`);
  return `<svg class="spark" viewBox="0 0 100 30" preserveAspectRatio="none" aria-hidden="true">
    <path class="spark-fill" d="M0,30L${pts.join('L')}L100,30Z"/><path class="spark-line" d="M${pts.join('L')}"/></svg>`;
}

// ---------- memory chart ----------
// Layers, bottom up: Windows and system, the biggest programs, other programs.
function chartLayers() {
  const ids = [...res.slots.entries()].sort((a, b) => a[1] - b[1]).map(e => e[0]);
  const layers = [
    { key: 'sys', cls: 'ksys', name: 'Windows and system', at: h => Math.max(0, h.used - h.procs) },
    ...ids.map(id => ({ key: id, cls: `k${res.slots.get(id)}`, name: res.names.get(id) || 'Closed program', at: h => (h.top.find(t => t[0] === id) || [0, 0])[1] })),
  ];
  layers.push({ key: 'other', cls: 'kother', name: 'Other programs', at: h => Math.max(0, h.procs - ids.reduce((n, id) => n + layers.find(l => l.key === id).at(h), 0)) });
  return layers;
}

function drawChart() {
  const h = res.history, svg = $('#mem-chart svg');
  if (!svg || !h.length) return;
  const total = res.now.system.mem_total;
  // Scale to the peak (rounded up to whole GB), so the layers aren't squashed; the label says so.
  const gb = 1024 ** 3, peak = Math.max(...h.map(s => s.used)) * 1.12;
  const max = Math.min(total, Math.ceil(peak / gb / (peak > 8 * gb ? 2 : 1)) * (peak > 8 * gb ? 2 : 1) * gb);
  const t1 = h[h.length - 1].t, t0 = Math.min(h[0].t, t1 - 60000);
  const x = t => ((t - t0) / (t1 - t0) * 1000).toFixed(1);
  const y = v => (200 - Math.min(1, v / max) * 200).toFixed(1);
  const layers = chartLayers();
  const base = h.map(() => 0);
  res.chart = { t0, t1, total, max, layers, stacks: [] };
  let paths = '';
  for (const l of layers) {
    const lo = base.slice();
    h.forEach((s, i) => { base[i] += l.at(s); });
    res.chart.stacks.push({ lo, hi: base.slice() });
    const top = h.map((s, i) => `${x(s.t)},${y(base[i])}`);
    const bottom = h.map((s, i) => `${x(s.t)},${y(lo[i])}`).reverse();
    paths += `<path class="layer ${l.cls}" data-layer="${l.key}" d="M${top.join('L')}L${bottom.join('L')}Z"/>`;
  }
  const grid = [0.25, 0.5, 0.75].map(f => `<path class="grid" d="M0,${200 - f * 200}H1000"/>`).join('');
  svg.innerHTML = grid + paths;
  $('#mem-max').textContent = max < total ? `${bytes(max)} (of ${bytes(total)})` : bytes(total);
  const span = Math.round((t1 - t0) / 60000);
  $('#mem-span').textContent = span >= 1 ? `${span} min ago` : `${Math.round((t1 - t0) / 1000)} s ago`;
  const latest = h[h.length - 1];
  const free = Math.max(0, total - latest.used);
  $('#mem-legend').innerHTML = [...layers].reverse().map(l =>
    `<button class="leg" data-layer="${l.key}"><i class="sw ${l.cls}"></i><span>${esc(l.name)}</span><b>${bytes(l.at(latest))}</b></button>`).join('')
    + `<span class="leg"><i class="sw kfree"></i><span>Free</span><b>${bytes(free)}</b></span>`;
  if (res.hover) hoverChart(res.hover.x, res.hover.y);
}

const LAYER_NOTES = {
  sys: 'Windows itself, drivers, and memory that programs share',
  other: 'Every smaller program, added together',
};

function hoverChart(cx, cy) {
  const box = $('#mem-chart'), c = res.chart;
  if (!box || !c) return;
  res.hover = { x: cx, y: cy };
  const r = box.getBoundingClientRect();
  const fx = Math.min(1, Math.max(0, (cx - r.left) / r.width));
  const t = c.t0 + fx * (c.t1 - c.t0);
  const h = res.history;
  let i = 0;
  for (let j = 1; j < h.length; j++) if (Math.abs(h[j].t - t) < Math.abs(h[i].t - t)) i = j;
  const v = (1 - (cy - r.top) / r.height) * c.max;
  const k = c.stacks.findIndex(s => v >= s.lo[i] && v < s.hi[i]);
  const tip = $('.tip', box), guide = $('.guide', box);
  const px = (h[i].t - c.t0) / (c.t1 - c.t0) * r.width;
  guide.hidden = false;
  guide.style.left = `${px}px`;
  const when = i === h.length - 1 ? 'Now' : new Date(h[i].t).toLocaleTimeString();
  let body;
  if (k < 0) {
    body = `<b>Free</b><div>${bytes(Math.max(0, c.total - h[i].used))}</div>`;
  } else {
    const l = c.layers[k], amount = c.stacks[k].hi[i] - c.stacks[k].lo[i];
    body = `<div class="tip-name"><i class="sw ${l.cls}"></i><b>${esc(l.name)}</b></div>
      <div>${bytes(amount)} · ${Math.round(amount / c.total * 100)}% of memory</div>
      ${LAYER_NOTES[l.key] ? `<div class="faint">${LAYER_NOTES[l.key]}</div>` : ''}`;
  }
  highlightLayer(k < 0 ? null : c.layers[k].key);
  tip.innerHTML = `${body}<div class="faint">${when}</div>`;
  tip.hidden = false;
  const left = px + 14 + tip.offsetWidth > r.width ? px - 14 - tip.offsetWidth : px + 14;
  tip.style.left = `${Math.max(0, left)}px`;
  tip.style.top = `${Math.min(r.height - tip.offsetHeight, Math.max(0, cy - r.top - tip.offsetHeight / 2))}px`;
}

function highlightLayer(key) {
  const k = key == null ? null : String(key);
  document.querySelectorAll('#mem-chart .layer').forEach(p => p.classList.toggle('dim', k !== null && p.dataset.layer !== k));
  document.querySelectorAll('#mem-legend .leg').forEach(b => b.classList.toggle('on', k !== null && b.dataset.layer === k));
}

function leaveChart() {
  res.hover = null;
  const box = $('#mem-chart');
  if (!box) return;
  $('.tip', box).hidden = true;
  $('.guide', box).hidden = true;
  highlightLayer(null);
}

// ---------- program list ----------
// Every word typed must appear in the name, the exe or the path; a number is a
// process ID or a port. Returns the name with the matches marked, and, when
// something matched that the name doesn't show, why the row is there.
function match(g, q) {
  if (!q) return { name: esc(g.name), why: '' };
  if (/^\d+$/.test(q)) {
    const n = Number(q);
    if (g.procs.some(p => p.pid === n)) return { name: esc(g.name), why: `process <mark>${n}</mark>` };
    const port = res.ports.some(p => p.group === g.id && (p.port === n || p.remote_port === n));
    return port ? { name: esc(g.name), why: `port <mark>${n}</mark>` } : null;
  }
  const ts = q.toLowerCase().split(/\s+/).filter(Boolean);
  const exe = [...new Set(g.procs.map(p => p.name))].join(', ');
  const hay = [g.name, exe, g.path || ''].map(h => h.toLowerCase());
  if (!ts.every(t => hay.some(h => h.includes(t)))) return null;
  const missing = ts.filter(t => !hay[0].includes(t));
  const why = !missing.length ? '' : missing.every(t => hay[1].includes(t)) ? exe : g.path;
  return { name: mark(g.name, ts), why: why ? mark(why, ts) : '' };
}

/** Escapes text and wraps each place a search word appears in <mark>. */
function mark(text, ts) {
  const s = String(text), l = s.toLowerCase();
  if (l.length !== s.length) return esc(s);
  const on = new Array(s.length).fill(false);
  for (const t of ts) for (let i = l.indexOf(t); i >= 0; i = l.indexOf(t, i + 1)) on.fill(true, i, i + t.length);
  let out = '';
  for (let i = 0, j; i < s.length; i = j) {
    for (j = i; j < s.length && on[j] === on[i]; j++);
    out += on[i] ? `<mark>${esc(s.slice(i, j))}</mark>` : esc(s.slice(i, j));
  }
  return out;
}

function portHit(q) {
  if (!/^\d+$/.test(q) || Number(q) > 65535) return '';
  const n = Number(q);
  const own = res.ports.filter(p => p.port === n && (p.state === 'listening' || p.proto === 'UDP'));
  if (!own.length) {
    const used = res.ports.filter(p => p.port === n || p.remote_port === n);
    return `<div class="hit muted">${used.length ? `No program is listening on port ${n}, but ${used.length} connection${used.length > 1 ? 's use' : ' uses'} it (below).` : `Nothing is using port ${n}.`}</div>`;
  }
  const seen = new Set();
  return `<div class="hit">${own.filter(p => !seen.has(`${p.proto}${p.pid}`) && seen.add(`${p.proto}${p.pid}`)).map(p =>
    `<div><b>Port ${n}</b> (${p.proto}) is open by <b>${esc(p.name || `process ${p.pid}`)}</b> <span class="faint">· process ${p.pid} · ${esc(p.local)}</span></div>`).join('')}</div>`;
}

function renderList() {
  const el = $('#res-list');
  // A two-step End button is armed: rebuilding would reset it under the pointer.
  if (!el || armed.size) return;
  const q = res.query.trim();
  $('#res-hit').innerHTML = portHit(q);
  const hits = new Map();
  for (const g of res.now.groups) {
    const m = match(g, q);
    if (m) hits.set(g.id, m);
  }
  const all = res.now.groups.filter(g => hits.has(g.id));
  $('#res-count').textContent = q ? `${all.length} of ${res.now.groups.length}` : '';
  const key = res.sort;
  // Hold the rows still under the pointer, and while a row has keyboard focus.
  const hold = res.hoverList || el.contains(document.activeElement);
  let order;
  if (hold && res.order.length) {
    // Don't reshuffle rows under the pointer; new programs go to the end.
    const ids = new Set(all.map(g => g.id));
    order = res.order.filter(id => ids.has(id));
    for (const g of all) if (!order.includes(g.id)) order.push(g.id);
  } else {
    order = all.sort((a, b) => b[key] - a[key] || b.mem - a.mem).map(g => g.id);
  }
  res.order = order;
  const shown = res.showAll || q ? order : order.slice(0, LIST_SHOWN);
  // Under the pointer, update the numbers in place: replacing the rows could swallow a click.
  const rowsKey = [shown.join(), res.open, res.procsOpen, q, res.sort].join('|');
  if (hold && rowsKey === res.listKey) {
    for (const id of shown) {
      const g = group(id), cells = el.querySelectorAll(`tr[data-row="${id}"] td.num`);
      if (g && cells.length === 3) [pct(g.cpu), bytes(g.mem), ioText(g.io)].forEach((v, i) => { cells[i].textContent = v; });
    }
    return;
  }
  res.listKey = rowsKey;
  el.innerHTML = `<div class="table-wrap"><table class="items">
    <colgroup><col><col class="c-num"><col class="c-num"><col class="c-num c-io"></colgroup>
    <thead><tr><th>Program</th><th class="num">Processor</th><th class="num">Memory</th><th class="num c-io">Activity</th></tr></thead>
    <tbody>${shown.map(id => programRow(group(id), hits.get(id))).join('')}
    ${!shown.length ? `<tr><td colspan="4" class="faint">No program matches "${esc(q)}".</td></tr>` : ''}</tbody></table></div>
    ${order.length > shown.length ? `<div class="more"><button class="btn ghost" id="res-all">Show all ${order.length} programs</button></div>` : ''}`;
}

function programRow(g, m) {
  const open = res.open === g.id;
  const n = g.procs.length;
  return `<tr class="prog${open ? ' open' : ''}" data-row="${g.id}">
    <td><button class="item-name" data-group="${g.id}" aria-expanded="${open}" title="${esc(g.path || g.name)}">${swatch(g.id)}<span class="nm"><span>${m.name}</span>${m.why ? `<small class="why faint">${m.why}</small>` : ''}</span>${n > 1 ? `<em class="faint">${n}</em>` : ''}</button></td>
    <td class="num">${pct(g.cpu)}</td><td class="num">${bytes(g.mem)}</td><td class="num c-io">${ioText(g.io)}</td></tr>
    ${open ? `<tr class="detail"><td colspan="4">${programDetail(g)}</td></tr>` : ''}`;
}

function programDetail(g) {
  const ports = groupPorts(g.id);
  const listen = [...new Set(ports.filter(p => p.state === 'listening').map(p => p.port))].sort((a, b) => a - b);
  const udp = [...new Set(ports.filter(p => p.proto === 'UDP').map(p => p.port))].sort((a, b) => a - b);
  const conns = ports.filter(p => p.state === 'connected').length;
  const chips = (list, proto) => list.slice(0, 12).map(p => `<span class="probe">${proto} ${p}</span>`).join('') + (list.length > 12 ? `<span class="faint">and ${list.length - 12} more</span>` : '');
  return `<div class="prog-detail">
    ${g.path ? `<div class="data faint path">${esc(g.path)}</div>` : ''}
    <div class="facts">${bytes(g.mem)} memory · ${pct(g.cpu)} processor · ${n(g.procs.length, 'process', 'processes')} · ${conns ? n(conns, 'network connection', 'network connections') : 'no network connections'}</div>
    ${listen.length || udp.length ? `<div class="probes">${chips(listen, 'TCP')}${chips(udp, 'UDP')}</div>` : ''}
    <div class="actions">
      ${g.path ? `<button class="btn" data-reveal-group="${g.id}">${icon('open')}Show in File Explorer</button>` : ''}
      ${g.protected ? '<span class="muted">Windows needs this to keep running, so it can\'t be ended here.</span>'
        : `<button class="btn" data-confirm="end-${g.id}" data-end-group="${g.id}">${icon('stop')}End program</button>`}
    </div>
    <details class="adv"${res.procsOpen === g.id ? ' open' : ''} data-procs="${g.id}"><summary>${icon('chevron')}Processes</summary>
      <div class="table-wrap"><table class="items procs">
        <colgroup><col class="c-pid"><col><col class="c-num"><col class="c-num"><col class="c-num c-io"><col class="c-act"></colgroup>
        <thead><tr><th>ID</th><th>Name</th><th class="num">Processor</th><th class="num">Memory</th><th class="num c-io">Threads</th><th><span hidden>End</span></th></tr></thead>
        <tbody>${[...g.procs].sort((a, b) => b.mem - a.mem).map(p => `<tr><td class="data">${p.pid}</td><td>${esc(p.name)}</td>
          <td class="num">${pct(p.cpu)}</td><td class="num">${bytes(p.mem)}</td><td class="num c-io">${p.threads}</td>
          <td>${g.protected ? '' : `<button class="btn icon ghost" data-confirm="pid-${p.pid}" data-was-icon="1" data-end-pid="${p.pid}" title="End this process" aria-label="End process ${p.pid}">${icon('x')}</button>`}</td></tr>`).join('')}</tbody>
      </table></div></details>
  </div>`;
}

const n = (k, one, many) => `${count(k)} ${k === 1 ? one : many}`;

// ---------- advanced ----------
function renderAdvanced() {
  const s = res.now?.system;
  const body = $('#res-adv-body');
  if (!s || !body) return;
  const up = s.uptime, days = Math.floor(up / 86400), hours = Math.floor(up % 86400 / 3600);
  const rows = res.ports
    .filter(p => !res.listenOnly || p.state === 'listening' || p.proto === 'UDP')
    .filter(p => !res.query.trim() || matchesPort(p, res.query.trim()))
    .sort((a, b) => a.port - b.port || a.proto.localeCompare(b.proto));
  body.innerHTML = `
    <div class="facts">Committed memory ${bytes(s.commit_used)} of ${bytes(s.commit_limit)} · ${count(s.processes)} processes · ${count(s.threads)} threads ·
      up for ${days ? `${n(days, 'day', 'days')}, ` : ''}${n(hours, 'hour', 'hours')} · updated every ${res.now.every} s</div>
    <div class="row-line"><label class="switch"><input type="checkbox" id="res-listen"${res.listenOnly ? ' checked' : ''}><span class="track"></span><span>Only ports that programs are listening on</span></label>
      <span class="faint">${n(rows.length, 'port', 'ports')}${res.query.trim() ? ` matching "${esc(res.query.trim())}"` : ''}</span></div>
    <div class="table-wrap"><table class="items ports">
      <colgroup><col class="c-port"><col class="c-proto"><col><col class="c-addr"><col class="c-state"></colgroup>
      <thead><tr><th class="num">Port</th><th>Type</th><th>Program</th><th>Address</th><th>State</th></tr></thead>
      <tbody>${rows.slice(0, 500).map(p => `<tr><td class="num">${p.port}</td><td>${p.proto}</td>
        <td>${p.group != null ? `<button class="link" data-group="${p.group}">${esc(p.name)}</button>` : esc(p.name || `process ${p.pid}`)} <span class="faint">${p.pid}</span></td>
        <td class="data">${esc(p.local)}${p.remote ? ` → ${esc(p.remote)}:${p.remote_port}` : ''}</td><td>${esc(p.state)}</td></tr>`).join('')}
      ${!rows.length ? '<tr><td colspan="5" class="faint">No ports to show.</td></tr>' : ''}</tbody></table></div>`;
}

function matchesPort(p, q) {
  if (/^\d+$/.test(q)) return p.port === Number(q) || p.remote_port === Number(q) || p.pid === Number(q);
  return (p.name || '').toLowerCase().includes(q.toLowerCase());
}

// ---------- events ----------
function showResources() {
  renderResources();
  tickResources();
}

function openProgram(id) {
  res.open = res.open === id ? null : id;
  if (res.open != null && !$(`#res-list [data-group="${id}"]`)) { res.query = ''; $('#res-q').value = ''; }
  renderList();
  $(`#res-list [data-group="${id}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
}

resourcesPage.addEventListener('click', e => run(async () => {
  const t = e.target;
  if (t.closest('#res-pause')) {
    res.paused = !res.paused;
    t.closest('#res-pause').innerHTML = `${icon(res.paused ? 'play' : 'pause')}${res.paused ? 'Resume' : 'Pause'}`;
    if (!res.paused) renderResourcesLive();
    return toast(res.paused ? 'Paused. The numbers stay put until you resume.' : 'Live again.');
  }
  const sort = t.closest('[data-sort]');
  if (sort) {
    res.sort = sort.dataset.sort;
    resourcesPage.querySelectorAll('[data-sort]').forEach(b => b.setAttribute('aria-pressed', b === sort));
    res.hoverList = false;
    return renderList();
  }
  if (t.closest('#res-all')) { res.showAll = true; return renderList(); }
  const leg = t.closest('.leg[data-layer]');
  if (leg && !['sys', 'other'].includes(leg.dataset.layer)) return openProgram(Number(leg.dataset.layer));
  const rv = t.closest('[data-reveal-group]');
  if (rv) return api('POST', '/api/resources/reveal', { group: Number(rv.dataset.revealGroup) });
  const eg = t.closest('[data-end-group]');
  if (eg) {
    const g = group(Number(eg.dataset.endGroup));
    return confirmClick(eg, `Click again to end ${g?.name || 'it'}`, () => run(async () => {
      const r = await api('POST', '/api/resources/end', { group: Number(eg.dataset.endGroup) });
      toast(r && r.ended < r.of ? `Ended ${r.ended} of ${r.of} processes of ${g?.name}.` : `Ended ${g?.name}.`);
      res.open = null;
      res.portsAt = 0;
      tickResources();
    }));
  }
  const ep = t.closest('[data-end-pid]');
  if (ep) {
    return confirmClick(ep, 'End', () => run(async () => {
      await api('POST', '/api/resources/end', { pid: Number(ep.dataset.endPid) });
      toast(`Ended process ${ep.dataset.endPid}.`);
      tickResources();
    }));
  }
  const g = t.closest('[data-group]');
  if (g) return openProgram(Number(g.dataset.group));
}));

function setQuery(v) {
  res.query = v;
  res.hoverList = false;
  renderList();
  if ($('#res-adv').open) renderAdvanced();
}

resourcesPage.addEventListener('input', e => {
  if (e.target.id === 'res-q') setQuery(e.target.value);
});

// / or Ctrl+F jumps to the search. In it: Enter opens the top program, Down
// moves into the list, Esc clears it (or leaves it when it's empty). In the
// list, Up and Down move between programs.
document.addEventListener('keydown', e => {
  const box = $('#res-q');
  if (!box || !pageShown('resources')) return;
  const typing = e.target.closest?.('input, textarea, select, [contenteditable]');
  if ((e.key === '/' && !typing) || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f')) {
    e.preventDefault();
    box.focus();
    box.select();
    return;
  }
  const rows = [...resourcesPage.querySelectorAll('#res-list .item-name[data-group]')];
  if (e.target === box) {
    if (e.key === 'Escape') {
      e.preventDefault();
      if (box.value) { box.value = ''; setQuery(''); } else box.blur();
    } else if (e.key === 'Enter' && box.value.trim() && rows[0]) {
      e.preventDefault();
      if (res.open !== Number(rows[0].dataset.group)) openProgram(Number(rows[0].dataset.group));
    } else if (e.key === 'ArrowDown' && rows[0]) {
      e.preventDefault();
      rows[0].focus();
    }
    return;
  }
  const i = rows.indexOf(e.target);
  if (i < 0 || (e.key !== 'ArrowDown' && e.key !== 'ArrowUp')) return;
  e.preventDefault();
  const next = e.key === 'ArrowDown' ? rows[i + 1] : i ? rows[i - 1] : box;
  next?.focus();
});

resourcesPage.addEventListener('change', e => {
  if (e.target.id === 'res-listen') { res.listenOnly = e.target.checked; renderAdvanced(); }
});

resourcesPage.addEventListener('toggle', e => {
  const d = e.target;
  if (d.id === 'res-adv' && d.open) renderAdvanced();
  if (d.dataset?.procs) res.procsOpen = d.open ? Number(d.dataset.procs) : (res.procsOpen === Number(d.dataset.procs) ? null : res.procsOpen);
}, true);

resourcesPage.addEventListener('pointermove', e => {
  if (e.target.closest('#mem-chart')) hoverChart(e.clientX, e.clientY);
  else if (res.hover) leaveChart();
  const leg = e.target.closest('.leg[data-layer]');
  if (leg) highlightLayer(leg.dataset.layer);
  else if (!res.hover) highlightLayer(null);
  res.hoverList = !!e.target.closest('#res-list');
});
resourcesPage.addEventListener('pointerleave', () => { leaveChart(); res.hoverList = false; });
