'use strict';
// Settings page: app-wide settings, the tool switches (from info.tools) and updates.
const settingsPage = registerPage('settings', { show: showSettings });
function settingRow(id, label, desc, on) {
  return `<div class="row"><div class="grow"><div class="label" id="${id}-l">${label}</div><div class="desc">${desc}</div></div>
    <label class="switch"><input type="checkbox" id="${id}" aria-labelledby="${id}-l"${on ? ' checked' : ''}><span class="track"></span></label></div>`;
}

function renderSettings() {
  const el = settingsPage;
  if (!info) { el.innerHTML = ''; return; }
  const s = info.settings, u = info.update;
  const updateLine = u.checking ? 'Checking…'
    : u.error ? esc(u.error)
    : u.latest && u.latest.version !== info.version ? `Version ${esc(u.latest.version)} is available.`
    : u.last_checked ? `You have the latest version. Checked ${ago(u.last_checked)}.` : 'Not checked yet.';
  el.innerHTML = `
    <div class="head"><div class="titles"><h1>Settings</h1></div></div>
    <div class="section"><h2>General</h2><div class="card">
      ${settingRow('set-autostart', 'Start with Windows', 'Starts Simple PC Tools in the tray when you sign in, with administrator rights and no prompt.', info.start_with_windows)}
      <div class="row"><div class="grow"><div class="label">Theme</div><div class="desc">System follows your Windows setting.</div></div>
        <div class="seg" role="group" aria-label="Theme">${['system', 'light', 'dark'].map(t => `<button data-theme-pick="${t}" aria-pressed="${t === s.theme}">${t[0].toUpperCase() + t.slice(1)}</button>`).join('')}</div></div>
    </div></div>
    <div class="section"><h2>Tools</h2><div class="card">
      ${info.tools.map(t => settingRow(`tool-${t.id}`, esc(t.name), esc(t.about), t.on)).join('')}
    </div></div>
    <div class="section"><h2>Updates</h2><div class="card">
      ${settingRow('set-updates', 'Check for updates', 'Once a day, asks GitHub whether there is a newer version. Nothing about you or your PC is sent.', s.updates.check)}
      <div class="row"><div class="grow"><div class="label">Version ${esc(info.version)}</div><div class="desc">${updateLine}</div></div>
        <button class="btn" id="check-updates">${icon('refresh')}Check now</button></div>
    </div></div>
    <div class="section"><h2>About</h2><div class="card">
      <div class="row"><div class="grow"><div class="label">Simple PC Tools</div>
        <div class="desc">Small tools that do what you want with no frills. Free and open source under the GPL-3.0 licence. No ads, no accounts, no tracking.</div></div></div>
      <div class="row"><div class="grow"><div class="label">Ideas and problems</div><div class="desc">Tell us what would make it better, or which simple tool you'd like next.</div></div>
        <div class="actions"><a class="btn" href="https://github.com/${esc(info.repo)}/issues/new/choose" target="_blank" rel="noopener">${icon('open')}Send feedback</a>
        <a class="btn ghost" href="https://github.com/${esc(info.repo)}" target="_blank" rel="noopener">Source code</a></div></div>
      <div class="row"><div class="grow"><div class="label">Data folder</div><div class="desc data">${esc(info.data_dir)}</div></div></div>
      ${info.elevated ? '' : `<div class="row"><div class="grow"><div class="label">Administrator rights</div><div class="desc">Not running as administrator, so disk scans are slower and apps start with this window's rights.</div></div></div>`}
    </div></div>`;
}

function showSettings() { renderSettings(); loadInfo().then(renderSettings); }

settingsPage.addEventListener('change', async e => {
  const t = e.target;
  const map = {
    'set-autostart': ['start_with_windows', on => on ? 'Simple PC Tools will start with Windows.' : "Simple PC Tools won't start with Windows."],
    'set-updates': ['check_updates', on => on ? "We'll let you know when there's a new version." : "Update checks are off. We won't ask again."],
  };
  if (t.id.startsWith('tool-')) return run(async () => {
    const tool = toolInfo(t.id.slice(5));
    try {
      await api('PATCH', '/api/settings', { tools: { [tool.id]: t.checked } });
      toast(`${tool.name} is ${t.checked ? 'on' : 'off'}.`);
    } catch (err) {
      t.checked = !t.checked;
      throw err;
    } finally {
      if (DEMO) tool.on = t.checked; else await loadInfo();
      renderNav();
    }
  });
  const m = map[t.id];
  if (!m) return;
  try {
    await api('PATCH', '/api/settings', { [m[0]]: t.checked });
    toast(m[1](t.checked));
    if (DEMO) {
      if (m[0] === 'check_updates') info.settings.updates.check = t.checked;
      if (m[0] === 'start_with_windows') info.start_with_windows = t.checked;
      return;
    }
  } catch (err) {
    toast(err.message);
    t.checked = !t.checked;
  }
  await loadInfo();
  renderSettings();
});

settingsPage.addEventListener('click', e => run(async () => {
  const pick = e.target.closest('[data-theme-pick]');
  if (pick) {
    await api('PATCH', '/api/settings', { theme: pick.dataset.themePick });
    if (DEMO) { info.settings.theme = pick.dataset.themePick; applyTheme(); return renderSettings(); }
    await loadInfo();
    return renderSettings();
  }
  if (e.target.closest('#check-updates')) {
    await api('POST', '/api/update/check').catch(err => toast(err.message));
    if (!DEMO) await loadInfo();
    renderSettings();
  }
}));
