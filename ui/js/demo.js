'use strict';
// Demo data for /?demo: every state of every page, and no server writes.
// Keep it in step with the API; tests/ui_design.rs checks the basics.
const t0 = now();
const demo = {
  info: {
    version: '0.1.0', repo: 'Dri-water/simple-pc-tools', elevated: true, data_dir: 'C:\\Users\\you\\AppData\\Local\\Simple PC Tools', start_with_windows: true,
    tools: [
      { id: 'apps', name: 'Apps', about: 'Start programs when you sign in and keep them running.', on: true },
      { id: 'disk', name: 'Disk usage', about: 'See which folders take up the most space.', on: true },
    ],
    settings: { tools: {}, theme: 'system', updates: { check: true, skipped: null, snoozed_until: 0 } },
    update: { current: '0.1.0', latest: { version: '0.2.0', page: 'https://github.com/Dri-water/simple-pc-tools/releases', notes: '', installer_url: 'x', installer_size: 1, digest: null }, last_checked: t0 - 3600, checking: false, error: null, downloading: null },
    update_offer: { version: '0.2.0', page: 'https://github.com/Dri-water/simple-pc-tools/releases' },
  },
  apps: {
    paused: false, check_every_secs: 60, next_check: t0 + 42,
    apps: [
      { id: 'notes-sync', name: 'Notes sync', program: 'C:\\Program Files\\NotesSync\\notessync.exe', args: '--tray', folder: '', start_at_login: true, keep_running: true, advanced: {}, watched_process: 'notessync.exe',
        status: { health: 'healthy', acting: false, last_check: t0, probes: [{ ok: true, detail: 'notessync.exe running (pid 4412)' }], restarts: [], events: [{ at: t0 - 7200, message: 'Notes sync: running' }] } },
      { id: 'home-server', name: 'Home media server', program: 'C:\\Servers\\media\\mediaserver.exe', args: '', folder: '', start_at_login: true, keep_running: true, watched_process: 'mediaserver.exe',
        advanced: { checks: [{ kind: 'http', url: 'http://localhost:8096/health', timeout_secs: 10 }] },
        status: { health: 'failing', acting: false, last_check: t0, probes: [{ ok: true, detail: 'mediaserver.exe running (pid 9120)' }, { ok: false, detail: 'http://localhost:8096/health unreachable: timed out' }], restarts: [], events: [{ at: t0 - 50, message: 'Home media server: failing (http://localhost:8096/health unreachable: timed out)' }] } },
      { id: 'game-overlay', name: 'Game overlay', program: 'C:\\Games\\Overlay\\overlay.exe', args: '', folder: '', start_at_login: true, keep_running: true, advanced: {}, watched_process: 'overlay.exe',
        status: { health: 'starting', acting: false, last_check: t0, probes: [{ ok: false, detail: 'overlay.exe not running' }], restarts: [t0 - 20], events: [{ at: t0 - 20, message: 'Game overlay: started C:\\Games\\Overlay\\overlay.exe (pid 1180)' }] } },
      { id: 'backup', name: 'Nightly backup agent', program: 'C:\\Tools\\backup\\agent.exe', args: '', folder: '', start_at_login: true, keep_running: true, watched_process: 'agent.exe',
        advanced: { hold_if: [{ kind: 'file_exists', path: 'C:\\Tools\\backup\\paused.flag' }] },
        status: { health: 'held', acting: false, last_check: t0, hold_reason: 'C:\\Tools\\backup\\paused.flag exists', probes: [{ ok: false, detail: 'agent.exe not running' }], restarts: [], events: [] } },
      { id: 'dev-db', name: 'Dev database', program: 'C:\\Dev\\db\\start.bat', args: '', folder: '', start_at_login: false, keep_running: true,
        advanced: { skip_process_check: true, checks: [{ kind: 'tcp', host: '127.0.0.1', port: 5432, timeout_secs: 3 }] },
        status: { health: 'gave_up', acting: false, last_check: t0, probes: [{ ok: false, detail: '127.0.0.1:5432 refused' }], restarts: [t0 - 3000, t0 - 2000, t0 - 1000, t0 - 600, t0 - 300], events: [] } },
      { id: 'chat', name: 'Chat', program: 'C:\\Users\\you\\AppData\\Local\\Chat\\chat.exe', args: '--minimized', folder: '', start_at_login: true, keep_running: false, advanced: {}, watched_process: 'chat.exe',
        status: { health: 'stopped', acting: false, last_check: t0, probes: [{ ok: false, detail: 'chat.exe not running' }], restarts: [], events: [] } },
    ],
  },
  drives: { fast: true, drives: [
    { root: 'C:\\', label: 'Windows', file_system: 'NTFS', total: 1024e9, free: 211e9 },
    { root: 'D:\\', label: 'Games', file_system: 'NTFS', total: 2000e9, free: 120e9 },
  ] },
};
// Demo states, for the design tests: health: 'healthy' 'failing' 'starting' 'held' 'gave_up' 'stopped'.
const demoTree = (() => {
  const nodes = [];
  const add = (name, parent, size, files, dir = true) => { nodes.push({ id: nodes.length, name, parent, size, files, is_dir: dir }); return nodes.length - 1; };
  const root = add('C:\\', 0, 813e9, 1840211);
  const users = add('Users', root, 402e9, 712004);
  add('Windows', root, 38e9, 310221);
  add('Program Files', root, 61e9, 220114);
  add('Program Files (x86)', root, 19e9, 90112);
  add('ProgramData', root, 14e9, 120443);
  add('pagefile.sys', root, 17e9, 1, false);
  add('hiberfil.sys', root, 6.4e9, 1, false);
  add('Games', root, 255e9, 377315);
  const you = add('you', users, 398e9, 700001);
  add('Public', users, 4e9, 12003);
  add('Videos', you, 190e9, 1203);
  add('AppData', you, 96e9, 510221);
  add('Downloads', you, 61e9, 3420);
  add('Documents', you, 33e9, 150021);
  add('Pictures', you, 18e9, 36100);
  add('big-installer.iso', you, 4.7e9, 1, false);
  return nodes;
})();
const demoKids = id => demoTree.filter(n => n.parent === id && n.id !== id).sort((a, b) => b.size - a.size);
let demoScan = { state: 'idle' };

async function demoApi(method, path, body) {
  await new Promise(r => setTimeout(r, 120));
  const p = path.replace(/\?.*/, '');
  if (p === '/api/info') return demo.info;
  if (p === '/api/apps' && method === 'GET') return structuredClone({ ...demo.apps, next_check: now() + 60 - ((now() - t0 + 18) % 60) });
  if (p === '/api/apps/pick') return { program: 'C:\\Program Files\\Example\\example.exe', name: 'Example' };
  if (p === '/api/disk/drives') return demo.drives;
  if (p === '/api/disk/scan') { demoScan = { state: 'scanning', path: body.path, files: 0, bytes: 0, phase: 'Reading file table', elapsed: 0, started: Date.now() }; return null; }
  if (p === '/api/disk/status') {
    if (demoScan.state === 'scanning') {
      const t = (Date.now() - demoScan.started) / 1000;
      Object.assign(demoScan, { files: Math.round(t * 420000), bytes: t * 190e9, elapsed: t });
      if (t > 2.2) Object.assign(demoScan, { state: 'done', files: 1840211, bytes: 813e9, elapsed: 4.4, method: 'file_table', skipped: 0 });
    }
    return { ...demoScan };
  }
  if (p === '/api/disk/list') {
    const id = Number(new URLSearchParams(path.split('?')[1] || '').get('id') || 0);
    const node = demoTree[id];
    const crumbs = [];
    for (let n = node; ; n = demoTree[n.parent]) { crumbs.unshift({ id: n.id, name: n.name }); if (n.id === 0) break; }
    const children = demoKids(id).map(c => ({ ...c, share: c.size / node.size, has_children: demoKids(c.id).length > 0, allocated: c.size, dirs: 0, modified: t0 - 86400 }));
    return { node: { ...node, share: 1 }, path: '', crumbs, children, other: id === 0 ? { count: 12, size: 2.1e9 } : null };
  }
  if (p === '/api/disk/pick') return { path: 'C:\\Users\\you\\Downloads' };
  return null; // writes change nothing in the demo
}

// Screenshot set-ups for the README (scripts/screenshots.ps1):
// /?demo&theme=dark&shot=editor#apps, /?demo&shot=disk#disk, ...
if (DEMO) window.addEventListener('DOMContentLoaded', () => setTimeout(async () => {
  const q = new URLSearchParams(location.search);
  if (q.get('theme')) {
    demo.info.settings.theme = q.get('theme');
    applyTheme();
    if (pageShown('settings')) renderSettings();
  }
  if (q.get('noupdate') !== null) { demo.info.update_offer = null; renderUpdateBanner(); }
  const shot = q.get('shot');
  if (shot === 'editor') document.querySelector('[data-id=home-server] [data-app-act=edit]')?.click();
  if (shot === 'activity') document.querySelector('[data-id=home-server] [data-app-act=events]')?.click();
  if (shot === 'disk') {
    await startScan('C:\\');
    await new Promise(r => setTimeout(r, 2600));
    await pollScan();
  }
}, 300));
