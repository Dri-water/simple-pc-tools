//! Resources: what's using the PC's memory, processor and network, grouped by
//! program, with a short history for the memory chart and a port lookup.
//!
//! Sampling is cheap but not free, so it runs every second only while the
//! page is open, and every few seconds otherwise (to keep the chart's history).

pub mod ports;
pub mod sample;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Notify;

use crate::api::bad_request;
use sample::{Process, Sampler, System};

/// How much history the memory chart keeps.
const HISTORY_MS: u64 = 15 * 60 * 1000;
/// Programs kept per history sample, biggest first; the rest count as "other".
const HISTORY_TOP: usize = 16;
const WATCHED_EVERY: Duration = Duration::from_secs(1);
const BACKGROUND_EVERY: Duration = Duration::from_secs(5);
/// The page counts as open for this long after its last request.
const WATCH_MS: u64 = 15_000;

/// Processes Windows needs to keep running. The UI doesn't offer to end them,
/// and the API refuses to.
const PROTECTED: &[&str] = &[
    "system",
    "registry",
    "memory compression",
    "secure system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "dwm.exe",
    "fontdrvhost.exe",
    "simple-pc-tools.exe",
];

pub struct Resources {
    pub enabled: AtomicBool,
    pub wake: Notify,
    watched_until: AtomicU64,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    sampler: Sampler,
    groups: Groups,
    history: VecDeque<Sample>,
    latest: Option<Latest>,
}

/// Programs: processes grouped by their exe, so 30 browser processes are one row.
#[derive(Default)]
struct Groups {
    list: Vec<Group>,
    by_key: HashMap<String, u32>,
    descriptions: HashMap<String, Option<String>>,
}

struct Group {
    name: String,
    path: Option<String>,
    protected: bool,
}

struct Sample {
    t: u64,
    cpu: f32,
    used: u64,
    /// Memory used by all processes we could read; the rest of `used` is Windows itself.
    procs: u64,
    rx: f64,
    tx: f64,
    top: Vec<(u32, u64)>,
}

struct Latest {
    t: u64,
    sys: System,
    procs: Vec<(u32, Process)>,
}

pub fn start() -> Arc<Resources> {
    let r = Arc::new(Resources {
        enabled: AtomicBool::new(true),
        wake: Notify::new(),
        watched_until: AtomicU64::new(0),
        inner: Mutex::default(),
    });
    tokio::spawn(r.clone().run());
    r
}

impl Resources {
    async fn run(self: Arc<Self>) {
        loop {
            let wait = if !self.enabled.load(Ordering::Relaxed) {
                // Let go of the process handles and history while switched off.
                *self.inner.lock().unwrap() = Inner::default();
                Duration::from_secs(3600)
            } else {
                let me = self.clone();
                let _ = tokio::task::spawn_blocking(move || me.sample_now()).await;
                if self.watched() {
                    WATCHED_EVERY
                } else {
                    BACKGROUND_EVERY
                }
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.wake.notified() => {}
            }
        }
    }

    fn watched(&self) -> bool {
        now_ms() < self.watched_until.load(Ordering::Relaxed)
    }

    /// Marks the page as open; samples straight away if it wasn't.
    fn watch(&self) {
        let was = self.watched();
        self.watched_until
            .store(now_ms() + WATCH_MS, Ordering::Relaxed);
        if !was {
            self.wake.notify_one();
        }
    }

    fn sample_now(&self) {
        let mut inner = self.inner.lock().unwrap();
        let Inner {
            sampler,
            groups,
            history,
            latest,
        } = &mut *inner;
        let (sys, procs) = sampler.sample();
        let t = now_ms();
        let procs: Vec<(u32, Process)> = procs.into_iter().map(|p| (groups.id(&p), p)).collect();

        let mut by_group: HashMap<u32, u64> = HashMap::new();
        for (g, p) in &procs {
            *by_group.entry(*g).or_default() += p.mem;
        }
        let mut top: Vec<(u32, u64)> = by_group.into_iter().filter(|g| g.1 > 0).collect();
        top.sort_by_key(|g| std::cmp::Reverse(g.1));
        top.truncate(HISTORY_TOP);
        history.push_back(Sample {
            t,
            cpu: sys.cpu,
            used: sys.mem_used,
            procs: procs.iter().map(|(_, p)| p.mem).sum(),
            rx: sys.net_rx,
            tx: sys.net_tx,
            top,
        });
        while history.front().is_some_and(|s| s.t + HISTORY_MS < t) {
            history.pop_front();
        }
        *latest = Some(Latest { t, sys, procs });
    }
}

impl Groups {
    fn id(&mut self, p: &Process) -> u32 {
        let key = match &p.path {
            Some(path) => path.to_lowercase(),
            None => format!("name:{}", p.name.to_lowercase()),
        };
        if let Some(&id) = self.by_key.get(&key) {
            return id;
        }
        let description = match &p.path {
            Some(path) => self
                .descriptions
                .entry(path.to_lowercase())
                .or_insert_with(|| sample::file_description(path))
                .clone(),
            None => None,
        };
        let id = self.list.len() as u32;
        self.list.push(Group {
            name: display_name(&p.name, description.as_deref()),
            path: p.path.clone(),
            protected: PROTECTED.contains(&p.name.to_lowercase().as_str()),
        });
        self.by_key.insert(key, id);
        id
    }
}

/// A friendly name: the exe's description when it has a useful one, else its
/// file name without ".exe".
fn display_name(exe: &str, description: Option<&str>) -> String {
    let stem = exe
        .strip_suffix(".exe")
        .or_else(|| exe.strip_suffix(".EXE"))
        .unwrap_or(exe);
    match description.map(str::trim) {
        // Some exes describe themselves with just their file name, or nothing useful.
        Some(d) if !d.is_empty() && !d.eq_ignore_ascii_case(exe) && d.len() <= 60 => d.into(),
        _ if stem.is_empty() => exe.into(),
        _ => stem.into(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

// ---------- API ----------

type Shared = State<Arc<Resources>>;

pub fn router(r: Arc<Resources>) -> Router {
    Router::new()
        .route("/", get(now))
        .route("/ports", get(port_list))
        .route("/end", post(end))
        .route("/reveal", post(reveal))
        .with_state(r)
}

#[derive(Deserialize)]
struct Since {
    /// History samples after this time (ms); 0 for all of it.
    #[serde(default)]
    since: u64,
}

/// The latest reading, grouped by program, plus the history since `since`.
async fn now(State(r): Shared, Query(q): Query<Since>) -> Json<Value> {
    r.watch();
    let inner = r.inner.lock().unwrap();
    let Some(latest) = &inner.latest else {
        return Json(json!({ "t": 0, "groups": [], "history": [] }));
    };
    let s = &latest.sys;

    struct Row<'a> {
        mem: u64,
        cpu: f32,
        io: f64,
        threads: u32,
        procs: Vec<&'a Process>,
    }
    let mut rows: HashMap<u32, Row> = HashMap::new();
    for (g, p) in &latest.procs {
        let row = rows.entry(*g).or_insert(Row {
            mem: 0,
            cpu: 0.0,
            io: 0.0,
            threads: 0,
            procs: Vec::new(),
        });
        row.mem += p.mem;
        row.cpu += p.cpu;
        row.io += p.io;
        row.threads += p.threads;
        row.procs.push(p);
    }
    let groups: Vec<Value> = rows
        .iter()
        .map(|(&id, row)| {
            let g = &inner.groups.list[id as usize];
            json!({
                "id": id,
                "name": g.name,
                "path": g.path,
                "protected": g.protected,
                "mem": row.mem,
                "cpu": row.cpu.min(100.0),
                "io": row.io,
                "threads": row.threads,
                "procs": row.procs.iter().map(|p| json!({
                    "pid": p.pid, "parent": p.parent, "name": p.name,
                    "mem": p.mem, "cpu": p.cpu, "io": p.io, "threads": p.threads,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    let history: Vec<&Sample> = inner.history.iter().filter(|h| h.t > q.since).collect();
    // Names of programs in the history that have since closed.
    let closed: HashSet<u32> = history
        .iter()
        .flat_map(|h| h.top.iter().map(|t| t.0))
        .filter(|id| !rows.contains_key(id))
        .collect();
    let names: HashMap<String, &str> = closed
        .into_iter()
        .map(|id| (id.to_string(), inner.groups.list[id as usize].name.as_str()))
        .collect();

    Json(json!({
        "t": latest.t,
        "every": if r.watched() { WATCHED_EVERY } else { BACKGROUND_EVERY }.as_secs(),
        "system": {
            "cpu": s.cpu,
            "mem_total": s.mem_total,
            "mem_used": s.mem_used,
            "commit_used": s.commit_used,
            "commit_limit": s.commit_limit,
            "net_rx": s.net_rx,
            "net_tx": s.net_tx,
            "uptime": s.uptime_secs,
            "processes": latest.procs.len(),
            "threads": latest.procs.iter().map(|(_, p)| u64::from(p.threads)).sum::<u64>(),
            "elevated": disk_scan::is_elevated(),
        },
        "groups": groups,
        "history": history.iter().map(|h| json!({
            "t": h.t, "cpu": h.cpu, "used": h.used, "procs": h.procs, "rx": h.rx, "tx": h.tx,
            "top": h.top,
        })).collect::<Vec<_>>(),
        "names": names,
    }))
}

async fn port_list(State(r): Shared) -> Json<Value> {
    r.watch();
    let list = tokio::task::spawn_blocking(ports::list)
        .await
        .unwrap_or_default();
    let inner = r.inner.lock().unwrap();
    let owner: HashMap<u32, u32> = inner
        .latest
        .iter()
        .flat_map(|l| l.procs.iter().map(|(g, p)| (p.pid, *g)))
        .collect();
    let ports: Vec<Value> = list
        .iter()
        .map(|p| {
            let g = owner.get(&p.pid);
            json!({
                "proto": p.proto,
                "local": p.local.to_string(),
                "port": p.port,
                "remote": p.remote.map(|r| r.0.to_string()),
                "remote_port": p.remote.map(|r| r.1),
                "state": p.state,
                "pid": p.pid,
                "group": g,
                "name": g.map(|&g| inner.groups.list[g as usize].name.as_str()).unwrap_or(match p.pid {
                    0 => "System Idle Process",
                    4 => "System",
                    _ => "",
                }),
            })
        })
        .collect();
    Json(json!({ "ports": ports }))
}

#[derive(Deserialize)]
struct EndRequest {
    /// A program (every process in it) ...
    group: Option<u32>,
    /// ... or one process.
    pid: Option<u32>,
}

/// Ends a program or one process.
async fn end(State(r): Shared, Json(req): Json<EndRequest>) -> Response {
    let (name, pids) = {
        let inner = r.inner.lock().unwrap();
        let Some(latest) = &inner.latest else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let procs: Vec<&(u32, Process)> = latest
            .procs
            .iter()
            .filter(|(g, p)| match (req.pid, req.group) {
                (Some(pid), _) => p.pid == pid,
                (None, Some(group)) => *g == group,
                _ => false,
            })
            .collect();
        let Some((g, first)) = procs.first() else {
            return bad_request(anyhow::anyhow!("That program has already closed."));
        };
        let group = &inner.groups.list[*g as usize];
        if group.protected || PROTECTED.contains(&first.name.to_lowercase().as_str()) {
            return bad_request(anyhow::anyhow!(
                "{} is part of Windows (or this app), so it can't be ended here.",
                group.name
            ));
        }
        (
            group.name.clone(),
            procs.iter().map(|(_, p)| p.pid).collect::<Vec<_>>(),
        )
    };
    let mut ended = 0;
    let mut last_err = None;
    for pid in &pids {
        match sample::end_process(*pid) {
            Ok(()) => ended += 1,
            Err(e) => last_err = Some(e),
        }
    }
    r.wake.notify_one();
    match (ended, last_err) {
        (0, Some(e)) => bad_request(anyhow::anyhow!("Couldn't end {name}: {e}")),
        _ => Json(json!({ "ended": ended, "of": pids.len() })).into_response(),
    }
}

#[derive(Deserialize)]
struct RevealRequest {
    group: u32,
}

async fn reveal(State(r): Shared, Json(req): Json<RevealRequest>) -> Response {
    let path = {
        let inner = r.inner.lock().unwrap();
        inner
            .groups
            .list
            .get(req.group as usize)
            .and_then(|g| g.path.clone())
    };
    let Some(path) = path else {
        return bad_request(anyhow::anyhow!(
            "Windows doesn't say where that program is."
        ));
    };
    match super::show_in_explorer(Path::new(&path)) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bad_request(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_names() {
        assert_eq!(
            display_name("chrome.exe", Some("Google Chrome")),
            "Google Chrome"
        );
        assert_eq!(display_name("tool.exe", Some("tool.exe")), "tool");
        assert_eq!(display_name("tool.exe", Some("  ")), "tool");
        assert_eq!(display_name("Registry", None), "Registry");
    }

    #[test]
    fn groups_processes_by_exe() {
        let p = |pid, name: &str, path: Option<&str>| Process {
            pid,
            parent: 0,
            name: name.into(),
            path: path.map(Into::into),
            threads: 1,
            mem: 0,
            cpu: 0.0,
            io: 0.0,
        };
        let mut g = Groups::default();
        let a = g.id(&p(1, "app.exe", Some("C:\\X\\app.exe")));
        let b = g.id(&p(2, "app.exe", Some("c:\\x\\APP.exe")));
        let c = g.id(&p(3, "app.exe", Some("C:\\Other\\app.exe")));
        let d = g.id(&p(4, "svchost.exe", None));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(g.list[d as usize].protected);
        assert!(!g.list[a as usize].protected);
    }

    #[test]
    fn samples_into_history() {
        let r = Resources {
            enabled: AtomicBool::new(true),
            wake: Notify::new(),
            watched_until: AtomicU64::new(0),
            inner: Mutex::default(),
        };
        r.sample_now();
        r.sample_now();
        let inner = r.inner.lock().unwrap();
        assert_eq!(inner.history.len(), 2);
        let h = inner.history.back().unwrap();
        assert!(h.used >= h.procs.min(h.used));
        assert!(!h.top.is_empty() && h.top.len() <= HISTORY_TOP);
        assert!(h.top.windows(2).all(|w| w[0].1 >= w[1].1), "biggest first");
    }
}
