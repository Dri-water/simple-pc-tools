//! The check loop: checks every app, decides whether to act, and runs the
//! app's stop/start steps. Apps can be added, changed and removed while it runs.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Notify;

use super::probe::ProbeResult;
use super::processes::ProcessTable;
use super::{App, Store};
use crate::settings::now;

const EVENTS_KEPT: usize = 30;
const HOUR: u64 = 3600;
/// Seconds between checks while something is starting or failing.
const RECOVERY_SECS: u64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Unknown,
    Healthy,
    /// Failing; will be restarted once the failure threshold is reached.
    Failing,
    /// Recently started; failures are not counted yet.
    Starting,
    /// Unhealthy, but a hold condition says to leave it alone.
    Held,
    /// Not running, and not set to keep running (or the manager is paused).
    Stopped,
    /// Restarted too often in the last hour; waiting for a manual restart.
    GaveUp,
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub at: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    pub health: Health,
    pub consecutive_failures: u32,
    pub last_check: Option<u64>,
    pub probes: Vec<ProbeResult>,
    pub hold_reason: Option<String>,
    pub grace_until: Option<u64>,
    pub restarts: VecDeque<u64>,
    pub events: VecDeque<Event>,
    pub acting: bool,
}

impl Default for AppStatus {
    fn default() -> Self {
        Self {
            health: Health::Unknown,
            consecutive_failures: 0,
            last_check: None,
            probes: Vec::new(),
            hold_reason: None,
            grace_until: None,
            restarts: VecDeque::new(),
            events: VecDeque::new(),
            acting: false,
        }
    }
}

impl AppStatus {
    fn log(&mut self, message: impl Into<String>) {
        let message = message.into();
        tracing::info!("{message}");
        self.events.push_front(Event { at: now(), message });
        self.events.truncate(EVENTS_KEPT);
    }
}

pub struct Manager {
    /// Off when the App manager tool is switched off in Settings: nothing is checked or started.
    pub enabled: AtomicBool,
    pub store: Mutex<Store>,
    pub status: Mutex<HashMap<String, AppStatus>>,
    pub wake: Notify,
    pub next_check: Mutex<u64>,
    path: PathBuf,
    http: reqwest::Client,
}

/// Why a check is running; decides which switch may trigger a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Sign-in (the manager just started) or a newly added app.
    Launch,
    Poll,
}

impl Manager {
    pub fn new(store: Store, path: PathBuf) -> Arc<Self> {
        let status = store
            .apps
            .iter()
            .map(|a| (a.id.clone(), AppStatus::default()))
            .collect();
        Arc::new(Self {
            enabled: AtomicBool::new(true),
            store: Mutex::new(store),
            status: Mutex::new(status),
            wake: Notify::new(),
            next_check: Mutex::new(0),
            path,
            http: reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .expect("http client"),
        })
    }

    pub fn app(&self, id: &str) -> Option<App> {
        self.store.lock().unwrap().app(id).cloned()
    }

    /// Changes the saved apps; keeps the status table in step.
    pub fn update<T>(
        &self,
        change: impl FnOnce(&mut Store) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut store = self.store.lock().unwrap();
        let mut next = store.clone();
        let out = change(&mut next)?;
        next.save(&self.path)?;
        *store = next;
        let mut status = self.status.lock().unwrap();
        status.retain(|id, _| store.app(id).is_some());
        for app in &store.apps {
            status.entry(app.id.clone()).or_default();
        }
        Ok(out)
    }

    /// Runs forever: a sign-in pass, then a check every interval (sooner
    /// while something is recovering, or when the UI asks).
    pub async fn run(self: Arc<Self>) {
        self.check_all(Trigger::Launch).await;
        loop {
            let delay = self.next_delay();
            *self.next_check.lock().unwrap() = now() + delay.as_secs();
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = self.wake.notified() => {}
            }
            self.check_all(Trigger::Poll).await;
        }
    }

    fn next_delay(&self) -> Duration {
        let every = self.store.lock().unwrap().check_every_secs.max(5);
        let status = self.status.lock().unwrap();
        let recovering = status
            .values()
            .any(|s| matches!(s.health, Health::Failing | Health::Starting));
        Duration::from_secs(if recovering {
            RECOVERY_SECS.min(every)
        } else {
            every
        })
    }

    async fn check_all(self: &Arc<Self>, trigger: Trigger) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let apps = self.store.lock().unwrap().apps.clone();
        let processes = Arc::new(ProcessTable::snapshot());
        let mut tasks = tokio::task::JoinSet::new();
        for app in apps {
            let (me, processes) = (self.clone(), processes.clone());
            tasks.spawn(async move { me.check(&app, &processes, trigger).await });
        }
        while tasks.join_next().await.is_some() {}
    }

    /// Checks one app now (used right after an app is added or changed).
    pub async fn check_one(self: &Arc<Self>, id: &str, trigger: Trigger) {
        if let Some(app) = self.app(id) {
            let processes = ProcessTable::snapshot();
            self.check(&app, &processes, trigger).await;
        }
    }

    async fn check(&self, app: &App, processes: &ProcessTable, trigger: Trigger) {
        match self.status.lock().unwrap().get(&app.id) {
            Some(s) if !s.acting => {}
            _ => return,
        }
        let plan = app.plan();
        let mut probes = Vec::with_capacity(plan.health.len());
        for probe in &plan.health {
            probes.push(probe.run(&self.http, processes).await);
        }
        let healthy = !probes.is_empty() && probes.iter().all(|p| p.ok);
        let mut hold = None;
        if !healthy {
            for probe in &plan.hold_if {
                let result = probe.run(&self.http, processes).await;
                if result.ok {
                    hold = Some(result.detail);
                    break;
                }
            }
        }

        let paused = self.store.lock().unwrap().paused;
        let (may_start, watch) = (app.start_at_login, app.keep_running);
        let a = &app.advanced;
        let act = {
            let mut status = self.status.lock().unwrap();
            let Some(s) = status.get_mut(&app.id) else {
                return;
            };
            s.last_check = Some(now());
            s.probes = probes;
            s.hold_reason = hold.clone();
            let in_grace = s.grace_until.is_some_and(|t| now() < t);
            s.restarts.retain(|t| now() - t < HOUR);

            if plan.health.is_empty() {
                // Nothing to check (a script with no checks): start it at sign-in only.
                s.health = Health::Unknown;
                trigger == Trigger::Launch && may_start && !paused && s.restarts.is_empty()
            } else if healthy {
                if s.health != Health::Healthy {
                    s.log(format!("{}: running", app.name));
                }
                s.health = Health::Healthy;
                s.consecutive_failures = 0;
                s.grace_until = None;
                false
            } else if let Some(reason) = hold {
                if s.health != Health::Held {
                    s.log(format!(
                        "{}: not running, but left alone ({reason})",
                        app.name
                    ));
                }
                s.health = Health::Held;
                false
            } else if in_grace {
                s.health = Health::Starting;
                false
            } else if s.health == Health::GaveUp {
                false
            } else {
                s.consecutive_failures += 1;
                let failing: Vec<_> = s
                    .probes
                    .iter()
                    .filter(|p| !p.ok)
                    .map(|p| p.detail.clone())
                    .collect();
                let wants = match trigger {
                    Trigger::Launch => may_start || watch,
                    Trigger::Poll => watch && s.consecutive_failures >= a.failures_before_restart,
                };
                if paused || !watch && trigger == Trigger::Poll || !(may_start || watch) {
                    if s.health != Health::Stopped {
                        s.log(format!(
                            "{}: not running ({})",
                            app.name,
                            failing.join("; ")
                        ));
                    }
                    s.health = Health::Stopped;
                    false
                } else if wants && s.restarts.len() as u32 >= a.max_restarts_per_hour {
                    s.health = Health::GaveUp;
                    s.log(format!(
                        "{}: restarted {} times this hour; stopped trying until you restart it",
                        app.name,
                        s.restarts.len()
                    ));
                    false
                } else {
                    s.health = Health::Failing;
                    if trigger == Trigger::Launch {
                        s.log(format!(
                            "{}: not running yet ({})",
                            app.name,
                            failing.join("; ")
                        ));
                    } else if s.consecutive_failures == 1 {
                        s.log(format!("{}: failing ({})", app.name, failing.join("; ")));
                    }
                    wants
                }
            }
        };
        if act {
            let running = processes_running(app, processes);
            self.restart_inner(
                app,
                if trigger == Trigger::Launch {
                    "starting"
                } else {
                    "restarting"
                },
                running,
            )
            .await;
        }
    }

    /// Stops (when it is running) and starts the app now, on request.
    pub async fn restart(&self, app: &App) {
        let running = processes_running(app, &ProcessTable::snapshot());
        if let Some(s) = self.status.lock().unwrap().get_mut(&app.id) {
            if s.health == Health::GaveUp {
                s.restarts.clear();
            }
        }
        self.restart_inner(app, "restart requested", running).await;
    }

    /// Ends the app's process now (or runs its custom stop steps).
    pub async fn stop(&self, app: &App) -> Vec<String> {
        let plan = app.plan();
        let mut lines = Vec::new();
        for action in &plan.stop {
            match action.run(app.advanced.run_as_admin).await {
                Ok(line) => lines.push(line),
                Err(e) => lines.push(format!("error: {e:#}")),
            }
        }
        if let Some(s) = self.status.lock().unwrap().get_mut(&app.id) {
            s.log(format!("{}: stopped by you", app.name));
            for line in &lines {
                s.log(format!("{}: {line}", app.name));
            }
        }
        self.wake.notify_one();
        lines
    }

    async fn restart_inner(&self, app: &App, why: &str, running: bool) {
        {
            let mut status = self.status.lock().unwrap();
            let Some(s) = status.get_mut(&app.id) else {
                return;
            };
            if s.acting {
                return;
            }
            s.acting = true;
            s.log(format!("{}: {why}", app.name));
        }
        let plan = app.plan();
        let stop = if running { plan.stop.as_slice() } else { &[] };
        let mut lines = Vec::new();
        for action in stop.iter().chain(plan.start.iter()) {
            match action.run(app.advanced.run_as_admin).await {
                Ok(line) => lines.push(line),
                Err(e) => lines.push(format!("error: {e:#}")),
            }
        }
        let mut status = self.status.lock().unwrap();
        let Some(s) = status.get_mut(&app.id) else {
            return;
        };
        for line in lines {
            s.log(format!("{}: {line}", app.name));
        }
        s.acting = false;
        s.restarts.push_back(now());
        s.consecutive_failures = 0;
        s.grace_until = Some(now() + app.advanced.startup_grace_secs);
        s.health = Health::Starting;
        drop(status);
        self.wake.notify_one();
    }
}

fn processes_running(app: &App, processes: &ProcessTable) -> bool {
    app.process()
        .is_some_and(|m| !processes.find(&m).is_empty())
}
