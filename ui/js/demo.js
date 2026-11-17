'use strict';
// Demo data for /?demo: every state of every page, and no server writes.
// Keep it in step with the API; tests/ui_design.rs checks the basics.
const t0 = now();
const demo = {
  info: {
    version: '0.2.0', repo: 'Dri-water/simple-pc-tools', elevated: true, data_dir: 'C:\\Users\\you\\AppData\\Local\\Simple PC Tools', start_with_windows: true,
    tools: [
      { id: 'apps', name: 'Apps', about: 'Start programs when you sign in and keep them running.', on: true },
      { id: 'disk', name: 'Disk usage', about: 'See which folders take up the most space.', on: true },
      { id: 'resources', name: 'Resources', about: "See what's using your memory, processor and network, and which program has a port open.", on: true },
    ],
    settings: { tools: {}, theme: 'system', updates: { check: true, skipped: null, snoozed_until: 0 } },
    update: { current: '0.2.0', latest: { version: '0.3.0', page: 'https://github.com/Dri-water/simple-pc-tools/releases', notes: '', installer_url: 'x', installer_size: 1, digest: null }, last_checked: t0 - 3600, checking: false, error: null, downloading: null },
    update_offer: { version: '0.3.0', page: 'https://github.com/Dri-water/simple-pc-tools/releases' },
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

// Resources: made-up programs whose use drifts smoothly with time, so the
// history and the live view agree. [id, name, exe, folder, memory GB, cpu %, processes, part of Windows]
const demoProgs = [
  [1, 'Web browser', 'browser.exe', 'C:\\Program Files\\Browser', 3.0, 5, 23],
  [2, 'Code editor', 'editor.exe', 'C:\\Users\\you\\AppData\\Local\\Programs\\Editor', 1.5, 3, 9],
  [3, 'Home media server', 'mediaserver.exe', 'C:\\Servers\\media', 1.1, 1.5, 2],
  [4, 'Photo editor', 'photos.exe', 'C:\\Program Files\\Photo Editor', 1.3, 2, 3],
  [5, 'Game launcher', 'launcher.exe', 'C:\\Games\\Launcher', 0.7, 0.5, 4],
  [6, 'Chat', 'chat.exe', 'C:\\Users\\you\\AppData\\Local\\Chat', 0.55, 0.8, 5],
  [7, 'Dev database', 'postgres.exe', 'C:\\Dev\\db\\bin', 0.42, 0.3, 8],
  [8, 'Service Host', 'svchost.exe', 'C:\\Windows\\System32', 0.38, 0.9, 61, true],
  [9, 'Antivirus', 'av-service.exe', 'C:\\Program Files\\Antivirus', 0.25, 0.7, 3],
  [10, 'Desktop Window Manager', 'dwm.exe', 'C:\\Windows\\System32', 0.15, 1.2, 1, true],
  [11, 'Windows Explorer', 'explorer.exe', 'C:\\Windows', 0.2, 0.4, 1],
  [12, 'Music player', 'music.exe', 'C:\\Program Files\\Music', 0.18, 0.6, 3],
  [13, 'Notes sync', 'notessync.exe', 'C:\\Program Files\\NotesSync', 0.09, 0.1, 1],
  [14, 'Simple PC Tools', 'simple-pc-tools.exe', 'C:\\Program Files\\Simple PC Tools', 0.04, 0.2, 1, true],
  [20, 'System', 'System', null, 0.6, 0.4, 1, true],
];
const GB = 1024 ** 3;
const demoT0 = Date.now();
const wave = (t, id, k = 1) => Math.sin(t / 47000 * k + id * 1.7) * 0.5 + Math.sin(t / 13000 * k + id) * 0.2;
function demoMem(p, t) {
  const [id, , , , gb] = p;
  if (id === 4 && t < demoT0 - 6 * 60000) return 0; // opened six minutes ago
  const grow = id === 1 ? 0.75 + 0.25 * Math.min(1, (t - demoT0 + 15 * 60000) / (15 * 60000)) : 1;
  return gb * GB * grow * (1 + 0.06 * wave(t, id));
}
const demoCpu = (p, t) => Math.max(0, p[5] * (1 + wave(t, p[0], 7)));
function demoSample(t) {
  const top = demoProgs.map(p => [p[0], demoMem(p, t)]).filter(x => x[1] > 0).sort((a, b) => b[1] - a[1]);
  const procs = top.reduce((n, x) => n + x[1], 0) + 0.5 * GB;
  return {
    t, cpu: demoProgs.reduce((n, p) => n + demoCpu(p, t), 2), used: procs + 3.4 * GB, procs,
    rx: Math.max(140e3, 900e3 * (1 + 1.6 * wave(t, 3, 1.5))), tx: 60e3 * (1 + wave(t, 5, 1.5)), top,
  };
}
function demoResources(since) {
  const t = Date.now();
  const history = [];
  if (since) for (let s = since + 1000; s <= t; s += 1000) history.push(demoSample(s));
  else for (let s = t - 15 * 60000; s <= t; s += 5000) history.push(demoSample(s));
  const cur = demoSample(t);
  const groups = demoProgs.filter(p => demoMem(p, t) > 0).map(p => {
    const [id, name, exe, dir, , , n, prot] = p;
    const mem = demoMem(p, t), cpu = demoCpu(p, t);
    const procs = Array.from({ length: n }, (_, i) => ({
      pid: id === 20 ? 4 : id * 1000 + 300 + 4 * i, parent: 1, name: exe, mem: mem * (i ? 0.6 / Math.max(1, n - 1) : n > 1 ? 0.4 : 1),
      cpu: cpu / n, io: 0, threads: 8 + i,
    }));
    return {
      id, name, path: dir && `${dir}\\${exe}`, protected: !!prot, mem, cpu, io: id === 1 ? 2.4e6 : id === 3 ? 640e3 : 0,
      threads: procs.reduce((k, x) => k + x.threads, 0), procs,
    };
  });
  return {
    t, every: 1, groups, history, names: {},
    system: {
      cpu: cur.cpu, mem_total: 32 * GB, mem_used: cur.used, commit_used: 19 * GB, commit_limit: 37 * GB, net_rx: cur.rx, net_tx: cur.tx,
      uptime: 3 * 86400 + 5 * 3600, processes: 214, threads: 3120, elevated: true,
    },
  };
}
const demoPort = (proto, local, port, state, group, pid, name, remote = null, remote_port = null) => ({ proto, local, port, state, group, pid, name, remote, remote_port });
const demoPorts = [
  demoPort('TCP', '0.0.0.0', 8096, 'listening', 3, 3300, 'Home media server'),
  demoPort('TCP', '127.0.0.1', 5432, 'listening', 7, 7012, 'Dev database'),
  demoPort('TCP', '127.0.0.1', 9229, 'listening', 2, 2012, 'Code editor'),
  demoPort('TCP', '0.0.0.0', 135, 'listening', 8, 8016, 'Service Host'),
  demoPort('TCP', '0.0.0.0', 445, 'listening', 20, 4, 'System'),
  demoPort('TCP', '192.168.1.20', 52344, 'connected', 1, 1012, 'Web browser', '140.82.112.4', 443),
  demoPort('TCP', '192.168.1.20', 52345, 'connected', 1, 1016, 'Web browser', '142.250.180.14', 443),
  demoPort('TCP', '192.168.1.20', 52388, 'connected', 6, 6012, 'Chat', '162.159.135.234', 443),
  demoPort('TCP', '192.168.1.20', 8096, 'connected', 3, 3300, 'Home media server', '192.168.1.31', 50122),
  demoPort('UDP', '0.0.0.0', 5353, 'open', 1, 1012, 'Web browser'),
  demoPort('UDP', '0.0.0.0', 3702, 'open', 8, 8020, 'Service Host'),
  demoPort('UDP', '0.0.0.0', 1900, 'open', 3, 3300, 'Home media server'),
];

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
  if (p === '/api/resources') return demoResources(Number(new URLSearchParams(path.split('?')[1] || '').get('since') || 0));
  if (p === '/api/resources/ports') return { ports: demoPorts };
  if (p === '/api/resources/end') return { ended: 1, of: 1 };
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
    if (pageShown('resources')) renderResources();
  }
  if (q.get('noupdate') !== null) { demo.info.update_offer = null; renderUpdateBanner(); }
  const shot = q.get('shot');
  if (shot === 'editor') document.querySelector('[data-id=home-server] [data-app-act=edit]')?.click();
  if (shot === 'activity') document.querySelector('[data-id=home-server] [data-app-act=events]')?.click();
  if (shot === 'chart' || shot === 'port' || shot === 'search') {
    await new Promise(r => setTimeout(r, 1500));
    if (shot === 'search') {
      const q = $('#res-q');
      q.value = 'serv';
      q.dispatchEvent(new Event('input', { bubbles: true }));
    } else if (shot === 'port') {
      const q = $('#res-q');
      q.value = '8096';
      q.dispatchEvent(new Event('input', { bubbles: true }));
      document.querySelector('#res-list [data-group="3"]')?.click();
    } else {
      // Point at the web browser's layer, two thirds of the way along, and hold it there.
      const r = $('#mem-chart').getBoundingClientRect(), c = res.chart;
      const k = c.layers.findIndex(l => l.key === 1), i = Math.floor(res.history.length * 0.68);
      const mid = (c.stacks[k].lo[i] + c.stacks[k].hi[i]) / 2;
      const x = r.left + r.width * (res.history[i].t - c.t0) / (c.t1 - c.t0), y = r.top + r.height * (1 - mid / c.max);
      $('#mem-chart').dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: x, clientY: y }));
      res.paused = true;
    }
  }
  if (shot === 'disk') {
    await startScan('C:\\');
    await new Promise(r => setTimeout(r, 2600));
    await pollScan();
  }
}, 300));
