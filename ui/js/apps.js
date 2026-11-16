'use strict';
// Apps page: the programs started at sign-in and kept running.
// API: /api/apps (src/tools/apps/api.rs).
const appsPage = registerPage('apps', { show: showApps });
const STATES = {
  healthy:  { label: 'Running', attn: false },
  starting: { label: 'Starting', attn: false, live: true },
  acting:   { label: 'Working…', attn: false, live: true },
  failing:  { label: 'Not responding', attn: true },
  gave_up:  { label: 'Gave up', attn: true },
  held:     { label: 'Left alone', attn: true },
  stopped:  { label: 'Not running', attn: false },
  unknown:  { label: 'Not checked yet', attn: false },
};
const stateOf = app => app.status?.acting ? 'acting' : (app.status?.health || 'unknown');

let apps = null, appsShape = '';
const openEvents = new Set();

function appNote(app, st) {
  const s = app.status || {};
  if (st === 'gave_up') return `Restarted ${s.restarts?.length || 0} times in the last hour, so it stopped trying. Restart it once you've fixed the problem.`;
  if (st === 'held') return `Left alone: ${esc(s.hold_reason)}`;
  if (st === 'failing' && app.keep_running) return 'It will be restarted if the next check fails too.';
  if (st === 'stopped' && !app.keep_running) return 'Turn on "Keep running" to have it restarted automatically.';
  if (st === 'unknown' && app.status?.last_check && !(s.probes || []).length) return 'Started at sign-in. Add a check under Advanced to watch it.';
  return '';
}

function renderApps() {
  const el = appsPage;
  const list = apps?.apps || [];
  const bad = list.filter(a => STATES[stateOf(a)].attn).length;
  setNavCount('apps', bad);
  if (!apps) return;

  const shape = JSON.stringify([apps.paused, apps.check_every_secs, list.map(a => [a.id, a.name, a.program, a.args, a.start_at_login, a.keep_running, stateOf(a), a.status?.hold_reason, (a.status?.probes || []).map(p => [p.ok, p.detail]), a.status?.events?.[0]?.at, openEvents.has(a.id)])]);
  if (shape === appsShape && el.firstChild) { tickApps(); return; }
  appsShape = shape;

  const counts = {};
  list.forEach(a => { const s = stateOf(a); counts[s] = (counts[s] || 0) + 1; });
  const summary = list.length ? `<div class="summary">
      ${['healthy', 'starting', 'acting', 'failing', 'gave_up', 'held', 'stopped', 'unknown'].filter(s => counts[s]).map(s =>
        `<span class="dot st-${s}"><span class="lamp"></span><b>${counts[s]}</b> ${STATES[s].label.toLowerCase()}</span>`).join('')}
      <span class="faint" id="next-check"></span></div>` : '';

  el.innerHTML = `
    <div class="head"><div class="titles"><h1>Apps</h1>
      <p class="sub">Start the programs you choose when you sign in, and restart them if they stop.</p></div>
      <div class="actions">
        ${list.length ? `<button class="btn" id="check-now">${icon('refresh')}Check now</button>` : ''}
        <button class="btn primary" id="add-app">${icon('plus')}Add app</button>
      </div></div>
    ${apps.paused ? `<div class="banner warn">${icon('pause')}<div class="grow"><b>Paused.</b> Apps are still checked, but nothing is started or restarted.</div>
      <button class="btn" id="resume">Resume</button></div>` : ''}
    ${summary}
    ${list.length ? `<div class="card">${list.map(appRow).join('')}</div>` : `
      <div class="card empty">${icon('apps', 'art')}<h2>No apps yet</h2>
        <p>Add a program and Simple PC Tools will start it when you sign in, and bring it back if it closes or stops answering.</p>
        <button class="btn primary" id="add-app-empty">${icon('plus')}Add your first app</button></div>`}
    ${list.length ? `<details class="adv after-list"><summary>${icon('chevron')}Manager settings</summary><div class="inner">
      <label class="switch"><input type="checkbox" id="pause"${apps.paused ? ' checked' : ''}><span class="track"></span>Pause: check apps, but don't start or restart anything</label>
      <label class="field"><span>Check every (seconds)</span><input type="number" min="5" max="86400" id="check-every" value="${apps.check_every_secs}">
      <small>While an app is starting or failing, it's checked every 15 seconds.</small></label></div></details>` : ''}`;
  tickApps();
}

function appRow(app) {
  const st = stateOf(app), S = STATES[st], s = app.status || {};
  const probes = s.probes || [];
  const showProbes = probes.length > 1 || probes.some(p => !p.ok);
  const note = appNote(app, st);
  const id = esc(app.id);
  return `<div class="app st-${st}${S.attn ? ' attn' : ''}" data-id="${id}">
    <span class="lamp${S.live ? ' live' : ''}" title="${S.label}"></span>
    <div class="app-main">
      <div class="app-name"><b>${esc(app.name)}</b><span class="pill">${S.label}</span></div>
      <div class="app-prog data" title="${esc(app.program)} ${esc(app.args)}">${esc(app.program)} ${esc(app.args)}</div>
      ${showProbes ? `<div class="probes">${probes.map(p => `<span class="probe ${p.ok ? 'ok' : 'bad'}" title="${esc(p.detail)}">${icon(p.ok ? 'check' : 'x')}${esc(p.detail)}</span>`).join('')}</div>` : ''}
      ${note ? `<div class="app-note">${note}</div>` : ''}
    </div>
    <div class="app-ctrl">
      <div class="switches">
        <label class="switch" title="Start it when you sign in"><input type="checkbox" data-toggle="start_at_login" id="sl-${id}"${app.start_at_login ? ' checked' : ''}><span class="track"></span>Start at sign-in</label>
        <label class="switch" title="Restart it when it stops or fails a check"><input type="checkbox" data-toggle="keep_running" id="kr-${id}"${app.keep_running ? ' checked' : ''}><span class="track"></span>Keep running</label>
      </div>
      <div class="buttons">
        <button class="btn" data-app-act="restart" data-confirm="restart-${id}" title="Restart now">${icon('restart')}Restart</button>
        <button class="btn icon ghost" data-app-act="stop" data-confirm="stop-${id}" data-was-icon="1" title="Stop it now" aria-label="Stop">${icon('stop')}</button>
        <button class="btn icon ghost" data-app-act="events" title="Activity" aria-label="Activity" aria-expanded="${openEvents.has(app.id)}">${icon('list')}</button>
        <button class="btn icon ghost" data-app-act="edit" title="Edit" aria-label="Edit">${icon('edit')}</button>
      </div>
    </div>
    ${openEvents.has(app.id) ? `<ul class="events">${(s.events || []).map(ev => `<li><time>${clock(ev.at)}</time><span>${esc(ev.message)}</span></li>`).join('') || '<li class="faint">Nothing has happened yet.</li>'}</ul>` : ''}
  </div>`;
}

function tickApps() {
  const n = $('#next-check');
  if (n && apps?.next_check) {
    const s = apps.next_check - now();
    n.textContent = s > 0 ? `Next check in ${s}s` : 'Checking…';
  }
}

async function loadApps() {
  try {
    apps = await api('GET', '/api/apps');
  } catch (e) {
    appsPage.innerHTML = `<div class="banner bad">${icon('alert')}<div class="grow">${esc(e.message)}</div></div>`;
    appsShape = '';
    return;
  }
  if (pageShown('apps')) renderApps();
}

function showApps() { renderApps(); loadApps(); }
setInterval(() => { if (pageShown('apps')) loadApps(); }, 5000);
setInterval(() => { if (pageShown('apps')) tickApps(); }, 1000);


appsPage.addEventListener('click', e => run(async () => {
  const t = e.target;
  if (t.closest('#add-app, #add-app-empty')) return openEditor(null);
  if (t.closest('#check-now')) { await api('POST', '/api/apps/check'); toast('Checking all apps now.'); setTimeout(loadApps, 1500); return; }
  if (t.closest('#resume')) { await api('PATCH', '/api/apps/settings', { paused: false }); toast('Resumed.'); return loadApps(); }
  const b = t.closest('[data-app-act]');
  if (!b) return;
  const app = apps.apps.find(a => a.id === b.closest('.app').dataset.id);
  const act = b.dataset.appAct;
  if (act === 'edit') return openEditor(app);
  if (act === 'events') {
    openEvents.has(app.id) ? openEvents.delete(app.id) : openEvents.add(app.id);
    return renderApps();
  }
  if (act === 'restart') return confirmClick(b, 'Click again to restart', () => run(async () => {
    await api('POST', `/api/apps/${encodeURIComponent(app.id)}/restart`);
    toast(`Restarting ${app.name}…`);
    setTimeout(loadApps, 800);
  }));
  if (act === 'stop') return confirmClick(b, 'Click again to stop', () => run(async () => {
    await api('POST', `/api/apps/${encodeURIComponent(app.id)}/stop`);
    toast(app.keep_running ? `Stopped ${app.name}. "Keep running" will start it again; turn that off to keep it stopped.` : `Stopped ${app.name}.`);
    setTimeout(loadApps, 800);
  }));
}));

appsPage.addEventListener('change', e => run(async () => {
  const t = e.target;
  try {
    if (t.dataset.toggle) {
      const id = t.closest('.app').dataset.id;
      const name = apps.apps.find(a => a.id === id).name;
      await api('PATCH', `/api/apps/${encodeURIComponent(id)}`, { [t.dataset.toggle]: t.checked });
      const what = t.dataset.toggle === 'keep_running' ? 'Keep running' : 'Start at sign-in';
      toast(`${what} is ${t.checked ? 'on' : 'off'} for ${name}.`);
    } else if (t.id === 'pause') {
      await api('PATCH', '/api/apps/settings', { paused: t.checked });
      toast(t.checked ? 'Paused. Nothing will be started or restarted.' : 'Resumed.');
    } else if (t.id === 'check-every') {
      await api('PATCH', '/api/apps/settings', { check_every_secs: Number(t.value) });
      toast(`Apps are checked every ${t.value} seconds.`);
    }
  } finally {
    appsShape = '';
    loadApps();
  }
}));

// ---------- app editor ----------
const CHECK_KINDS = {
  http: { label: 'URL answers', fields: [['url', 'http://localhost:8080/health']] },
  tcp: { label: 'Port is open', fields: [['port', '8080'], ['host', '127.0.0.1']] },
  process: { label: 'Process is running', fields: [['name', 'server.exe']] },
  file_exists: { label: 'File exists', fields: [['path', '%LOCALAPPDATA%\\app\\paused.flag']] },
};

function checkRows(list) {
  return list.map(c => {
    const k = CHECK_KINDS[c.kind] ? c.kind : 'http';
    const fields = CHECK_KINDS[k].fields.map(([f, ph]) =>
      `<input class="input" data-f="${f}" placeholder="${esc(ph)}" value="${esc(c[f] ?? '')}" aria-label="${f}" spellcheck="false">`).join('');
    return `<div class="check-row">
      <select class="input" data-f="kind" aria-label="Check type">${Object.entries(CHECK_KINDS).map(([v, d]) => `<option value="${v}"${v === k ? ' selected' : ''}>${d.label}</option>`).join('')}</select>
      <div class="input-row">${fields}</div>
      <button type="button" class="btn icon ghost" data-remove aria-label="Remove">${icon('x')}</button></div>`;
  }).join('');
}

function readChecks(container) {
  return [...container.querySelectorAll('.check-row')].map(row => {
    const c = { kind: row.querySelector('[data-f=kind]').value };
    row.querySelectorAll('input[data-f]').forEach(i => {
      const v = i.value.trim();
      if (v) c[i.dataset.f] = i.dataset.f === 'port' ? Number(v) : v;
    });
    return c;
  });
}

function openEditor(app) {
  const isNew = !app;
  const a = app ? structuredClone(app) : { name: '', program: '', args: '', folder: '', start_at_login: true, keep_running: true, advanced: {} };
  const adv = Object.assign({ process: null, skip_process_check: false, checks: [], hold_if: [], end_before_restart: true, run_as_admin: false, hide_window: false, failures_before_restart: 2, startup_grace_secs: 60, max_restarts_per_hour: 5, custom_start: [], custom_stop: [] }, a.advanced);
  const p = adv.process || {};
  const custom = (adv.custom_start.length || adv.custom_stop.length) ? JSON.stringify({ start: adv.custom_start, stop: adv.custom_stop }, null, 2) : '';
  const hasAdvanced = !!(adv.process || adv.skip_process_check || adv.checks.length || adv.hold_if.length || custom || adv.run_as_admin);
  const dlg = $('#editor');
  dlg.innerHTML = `<form novalidate>
    <header><h2 id="editor-title">${isNew ? 'Add app' : `Edit ${esc(a.name)}`}</h2>
      <button type="button" class="btn icon ghost" data-close aria-label="Close">${icon('x')}</button></header>
    <div class="body">
      <div class="form-error" id="ed-error" role="alert" hidden></div>
      <label class="field"><span>Program</span>
        <span class="input-row"><input id="ed-program" value="${esc(a.program)}" placeholder="C:\\Program Files\\Example\\example.exe" spellcheck="false">
        <button type="button" class="btn" id="ed-browse">Browse…</button></span>
        <small>An .exe, a script (.bat, .cmd, .ps1) or a shortcut.</small></label>
      <label class="field"><span>Name</span><input id="ed-name" value="${esc(a.name)}" placeholder="What you call it"></label>
      <div class="cols">
        <label class="field"><span>Arguments <span class="faint">(optional)</span></span><input id="ed-args" value="${esc(a.args)}" placeholder="--minimized" spellcheck="false"></label>
        <label class="field"><span>Start in <span class="faint">(optional)</span></span><input id="ed-folder" value="${esc(a.folder)}" placeholder="The program's folder" spellcheck="false"></label>
      </div>
      <label class="switch"><input type="checkbox" id="ed-sl"${a.start_at_login ? ' checked' : ''}><span class="track"></span>Start at sign-in</label>
      <label class="switch"><input type="checkbox" id="ed-kr"${a.keep_running ? ' checked' : ''}><span class="track"></span>Keep running: restart it when it stops or fails a check</label>

      <details class="adv"${hasAdvanced ? ' open' : ''}><summary>${icon('chevron')}Advanced</summary><div class="inner">
        <div class="field"><span>How to tell it's running</span>
          <small>By default Simple PC Tools looks for the program's own process. Set this when the program is a launcher or script that starts something else.</small>
          <div class="cols">
            <input class="input" id="ed-pname" value="${esc(p.name || '')}" placeholder="${esc(app?.watched_process || 'Process name, e.g. server.exe')}" aria-label="Process name" spellcheck="false">
            <input class="input" id="ed-pcmd" value="${esc(p.cmdline_contains || '')}" placeholder="Command line contains (optional)" aria-label="Command line contains" spellcheck="false">
          </div>
          <label class="switch"><input type="checkbox" id="ed-skip"${adv.skip_process_check ? ' checked' : ''}><span class="track"></span>Don't look for a process; use only the checks below</label>
        </div>
        <div class="field"><span>Extra checks</span>
          <small>All of them must pass, or the app counts as not responding. For example, a URL that must answer.</small>
          <div class="checks" id="ed-checks">${checkRows(adv.checks)}</div>
          <div><button type="button" class="btn" data-add="ed-checks">${icon('plus')}Add check</button></div></div>
        <div class="field"><span>Leave it alone when</span>
          <small>If any of these pass, it isn't restarted even when it looks down (it's busy, or you stopped it on purpose).</small>
          <div class="checks" id="ed-holds">${checkRows(adv.hold_if)}</div>
          <div><button type="button" class="btn" data-add="ed-holds">${icon('plus')}Add condition</button></div></div>
        <div class="cols">
          <label class="field"><span>Failed checks before restart</span><input type="number" min="1" id="ed-fails" value="${adv.failures_before_restart}"></label>
          <label class="field"><span>Grace after starting (s)</span><input type="number" min="0" id="ed-grace" value="${adv.startup_grace_secs}"></label>
          <label class="field"><span>Max restarts per hour</span><input type="number" min="1" id="ed-max" value="${adv.max_restarts_per_hour}"></label>
        </div>
        <label class="switch"><input type="checkbox" id="ed-end"${adv.end_before_restart ? ' checked' : ''}><span class="track"></span>End the old process before restarting it</label>
        <label class="switch"><input type="checkbox" id="ed-hide"${adv.hide_window ? ' checked' : ''}><span class="track"></span>Hide the console window</label>
        <label class="switch"><input type="checkbox" id="ed-admin"${adv.run_as_admin ? ' checked' : ''}><span class="track"></span>Run as administrator</label>
        <label class="field"><span>Custom start and stop steps (JSON)</span>
          <textarea id="ed-custom" rows="5" spellcheck="false" placeholder='{ "start": [{ "kind": "run", "program": "...", "args": "..." }], "stop": [{ "kind": "end_process", "name": "x.exe" }, { "kind": "wait", "secs": 2 }] }'>${esc(custom)}</textarea>
          <small>Replaces the default steps (run the program; end its process). Leave it empty to use the defaults.</small></label>
      </div></details>
    </div>
    <footer>
      ${isNew ? '' : `<button type="button" class="btn" id="ed-delete" data-confirm="delete-${esc(a.id)}">${icon('trash')}Remove app</button>`}
      <span class="grow"></span>
      <button type="button" class="btn" data-close>Cancel</button>
      <button type="submit" class="btn primary">${isNew ? 'Add app' : 'Save'}</button>
    </footer></form>`;

  const form = $('form', dlg);
  const fail = msg => { const el = $('#ed-error'); el.textContent = msg; el.hidden = false; el.scrollIntoView({ block: 'nearest' }); };
  form.addEventListener('click', e => run(async () => {
    const t = e.target;
    if (t.closest('[data-close]')) return dlg.close();
    if (t.closest('[data-remove]')) return t.closest('.check-row').remove();
    const add = t.closest('[data-add]');
    if (add) return $(`#${add.dataset.add}`).insertAdjacentHTML('beforeend', checkRows([{ kind: 'http' }]));
    if (t.closest('#ed-browse')) {
      const r = await api('POST', '/api/apps/pick');
      if (r?.program) {
        $('#ed-program').value = r.program;
        if (!$('#ed-name').value) $('#ed-name').value = r.name;
      }
      return;
    }
    const del = t.closest('#ed-delete');
    if (del) return confirmClick(del, 'Click again to remove', () => run(async () => {
      await api('DELETE', `/api/apps/${encodeURIComponent(a.id)}`);
      dlg.close();
      toast(`Removed ${a.name}. If it's running, it keeps running until you close it.`);
      loadApps();
    }));
  }));
  form.addEventListener('change', e => {
    const sel = e.target.closest('select[data-f=kind]');
    if (sel) sel.closest('.check-row').outerHTML = checkRows([{ kind: sel.value }]);
  });
  form.addEventListener('submit', async e => {
    e.preventDefault();
    let steps = { start: [], stop: [] };
    const raw = $('#ed-custom').value.trim();
    if (raw) {
      try { steps = Object.assign(steps, JSON.parse(raw)); }
      catch { return fail("The custom steps aren't valid JSON."); }
    }
    const pname = $('#ed-pname').value.trim(), pcmd = $('#ed-pcmd').value.trim();
    const body = {
      id: a.id || '',
      name: $('#ed-name').value.trim(),
      program: $('#ed-program').value.trim().replace(/^"|"$/g, ''),
      args: $('#ed-args').value.trim(),
      folder: $('#ed-folder').value.trim(),
      start_at_login: $('#ed-sl').checked,
      keep_running: $('#ed-kr').checked,
      advanced: {
        process: pname ? { name: pname, cmdline_contains: pcmd || undefined } : null,
        skip_process_check: $('#ed-skip').checked,
        checks: readChecks($('#ed-checks')),
        hold_if: readChecks($('#ed-holds')),
        end_before_restart: $('#ed-end').checked,
        run_as_admin: $('#ed-admin').checked,
        hide_window: $('#ed-hide').checked,
        failures_before_restart: Number($('#ed-fails').value) || 1,
        startup_grace_secs: Number($('#ed-grace').value) || 0,
        max_restarts_per_hour: Number($('#ed-max').value) || 1,
        custom_start: steps.start || [],
        custom_stop: steps.stop || [],
      },
    };
    if (!body.name && body.program) body.name = body.program.split(/[\\/]/).pop().replace(/\.[^.]+$/, '');
    try {
      if (isNew) await api('POST', '/api/apps', body);
      else await api('PUT', `/api/apps/${encodeURIComponent(a.id)}`, body);
    } catch (err) {
      return fail(err.message);
    }
    dlg.close();
    toast(isNew ? `Added ${body.name}.` : `Saved ${body.name}.`);
    appsShape = '';
    loadApps();
  });
  dlg.showModal();
  (isNew ? $('#ed-program') : $('#ed-name')).focus();
}
