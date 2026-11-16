'use strict';
// Disk usage page: folders sorted by size. API: /api/disk (src/tools/disk.rs).
const diskPage = registerPage('disk', { show: showDisk });
let drives = null, scan = null, listing = null, diskTimer = null;

function renderDisk() {
  const el = diskPage;
  const scanning = scan?.state === 'scanning';
  el.innerHTML = `
    <div class="head"><div class="titles"><h1>Disk usage</h1>
      <p class="sub">See which folders take up the most space. Nothing is changed or deleted here.</p></div>
      <div class="actions"><button class="btn" id="scan-folder"${scanning ? ' disabled' : ''}>${icon('folder')}Scan a folder…</button></div></div>
    ${drives && !drives.fast ? `<div class="banner warn">${icon('alert')}<div class="grow">Running without administrator rights, so scans go through folders one by one instead of reading the drive's file table. They work, just more slowly.</div></div>` : ''}
    <div class="drives">${(drives?.drives || []).map(d => {
      const used = d.total - d.free, pct = d.total ? used / d.total : 0;
      const name = d.root.replace(/\\$/, '');
      return `<div class="card drive"><div class="top"><b>${esc(d.label ? `${d.label} (${name})` : name)}</b><span class="faint">${esc(d.file_system)}</span></div>
        <div class="meter${pct > .9 ? ' full' : ''}"><i style="width:${(pct * 100).toFixed(1)}%"></i></div>
        <div class="muted">${bytes(d.free)} free of ${bytes(d.total)}</div>
        <div><button class="btn${scan?.state === 'done' && scan.path === d.root ? '' : ' primary'}" data-scan="${esc(d.root)}"${scanning ? ' disabled' : ''}>Scan ${esc(name)}</button></div></div>`;
    }).join('')}</div>
    <div id="scan-area" data-state="${esc(scan?.state || 'idle')}">${scanArea()}</div>`;
}

function scanArea() {
  if (!scan || scan.state === 'idle') return `<div class="card empty">${icon('disk', 'art')}<h2>Pick a drive to scan</h2><p>You'll see its folders sorted by size, biggest first. Click a folder to look inside.</p></div>`;
  if (scan.state === 'scanning') return `<div class="card scan-state"><span class="spinner"></span>
    <div class="grow"><b>Scanning ${esc(scan.path)}</b><div class="muted">${esc(scan.phase || 'Starting')} · ${count(scan.files)} files · ${bytes(scan.bytes)} · ${Number(scan.elapsed).toFixed(0)}s</div></div>
    <button class="btn" id="scan-cancel">Cancel</button></div>`;
  if (scan.state === 'error') return `<div class="banner bad">${icon('alert')}<div class="grow"><b>The scan didn't finish.</b> ${esc(scan.error)}</div></div>`;
  if (!listing) return '';
  const how = scan.method === 'file_table' ? "read the drive's file table" : 'went through every folder';
  const last = listing.crumbs.length - 1;
  return `<div class="hint scan-note">Scanned ${esc(scan.path)}: ${count(scan.files)} files in ${Number(scan.elapsed).toFixed(1)} seconds (${how}).
      ${scan.skipped ? `${count(scan.skipped)} folders couldn't be read (no permission).` : ''}</div>
    <div class="card">
      <div class="crumbs">${listing.crumbs.map((c, i) => `${i ? icon('chevron') : ''}<button data-node="${c.id}"${i === last ? ' aria-current="page"' : ''}>${esc(c.name)}</button>`).join('')}
        <span class="total">${bytes(listing.node.size)} · ${count(listing.node.files)} files</span></div>
      <div class="table-wrap"><table class="items">
        <colgroup><col><col class="c-share"><col class="c-size"><col class="c-files"><col class="c-act"></colgroup>
        <thead><tr><th>Name</th><th>Share</th><th class="num">Size</th><th class="num">Files</th><th><span hidden>Actions</span></th></tr></thead>
        <tbody>${listing.children.map(c => `<tr>
          <td><button class="item-name${c.is_dir ? ' dir' : ''}" ${c.is_dir && c.has_children ? `data-node="${c.id}"` : 'tabindex="-1"'} title="${esc(c.name)}">${icon(c.is_dir ? 'folder' : 'file')}<span>${esc(c.name)}</span></button></td>
          <td><div class="share"><div class="meter"><i style="width:${(c.share * 100).toFixed(1)}%"></i></div><span>${(c.share * 100).toFixed(c.share < .1 ? 1 : 0)}%</span></div></td>
          <td class="num">${bytes(c.size)}</td>
          <td class="num">${c.is_dir ? count(c.files) : ''}</td>
          <td><button class="btn icon ghost" data-reveal="${c.id}" title="Show in File Explorer" aria-label="Show in File Explorer">${icon('open')}</button></td></tr>`).join('')}
          ${listing.other ? `<tr><td class="faint" colspan="2">${count(listing.other.count)} smaller items</td><td class="num">${bytes(listing.other.size)}</td><td></td><td></td></tr>` : ''}
          ${!listing.children.length ? '<tr><td colspan="5" class="faint">This folder is empty.</td></tr>' : ''}
        </tbody></table></div></div>`;
}

async function pollScan() {
  clearTimeout(diskTimer);
  try {
    scan = await api('GET', '/api/disk/status');
    if (scan.state === 'done' && (!listing || listing.scanPath !== scan.path)) await openNode(null, false);
  } catch (e) {
    return toast(e.message);
  }
  if (!pageShown('disk')) return;
  const area = $('#scan-area');
  if (!area || area.dataset.state !== scan.state) renderDisk();
  else area.innerHTML = scanArea();
  if (scan.state === 'scanning') diskTimer = setTimeout(pollScan, 400);
}

async function openNode(id, render = true) {
  try {
    listing = await api('GET', `/api/disk/list${id == null ? '' : `?id=${id}`}`);
    listing.scanPath = scan.path;
  } catch (e) {
    return toast(e.message);
  }
  if (render) $('#scan-area').innerHTML = scanArea();
}

async function startScan(path) {
  await api('POST', '/api/disk/scan', { path });
  listing = null;
  pollScan();
}

async function showDisk() {
  renderDisk();
  if (!drives) {
    try { drives = await api('GET', '/api/disk/drives'); } catch (e) { toast(e.message); }
    renderDisk();
  }
  pollScan();
}

diskPage.addEventListener('click', e => run(async () => {
  const t = e.target;
  const d = t.closest('[data-scan]');
  if (d) return startScan(d.dataset.scan);
  if (t.closest('#scan-folder')) {
    const r = await api('POST', '/api/disk/pick');
    if (r?.path) await startScan(r.path);
    return;
  }
  if (t.closest('#scan-cancel')) { await api('POST', '/api/disk/cancel'); return toast('Scan cancelled.'); }
  const r = t.closest('[data-reveal]');
  if (r) return api('POST', '/api/disk/reveal', { id: Number(r.dataset.reveal) });
  const n = t.closest('[data-node]');
  if (n && !n.hasAttribute('aria-current')) return openNode(Number(n.dataset.node));
}));
