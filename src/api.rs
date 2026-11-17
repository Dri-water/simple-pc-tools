//! The UI's routes: the page itself, app-wide settings and updates, and each
//! tool's API nested under `/api/<tool>/`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::settings::{now, SettingsFile};
use crate::tools::{self, Tools};
use crate::update::{self, Updater};

#[derive(Clone)]
pub struct App {
    pub settings: Arc<SettingsFile>,
    pub updater: Arc<Updater>,
    pub tools: Tools,
    pub data_dir: PathBuf,
    pub exe: PathBuf,
    pub quit: Arc<dyn Fn() + Send + Sync>,
}

pub fn router(app: App) -> Router {
    let tools = app.tools.router();
    let mut router = Router::new();
    for &(path, mime, body) in UI_FILES {
        router = router.route(
            path,
            get(move || async move { ([(header::CONTENT_TYPE, mime)], body) }),
        );
    }
    router
        .route(
            "/icon.png",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/png")],
                    &include_bytes!("../assets/icon-256.png")[..],
                )
            }),
        )
        .route("/api/info", get(info))
        .route("/api/settings", patch(change_settings))
        .route("/api/update/check", post(check_update))
        .route("/api/update/install", post(install_update))
        .route("/api/update/later", post(snooze_update))
        .route("/api/update/skip", post(skip_update))
        .route("/api/quit", post(quit))
        .with_state(app)
        .merge(tools)
}

/// The UI: a page, a stylesheet and one script per page, built into the exe.
/// A new tool's `ui/js/<id>.js` goes here and in `ui/index.html`.
const UI_FILES: &[(&str, &str, &str)] = &[
    (
        "/",
        "text/html; charset=utf-8",
        include_str!("../ui/index.html"),
    ),
    (
        "/ui/style.css",
        "text/css; charset=utf-8",
        include_str!("../ui/style.css"),
    ),
    ("/ui/js/core.js", JS, include_str!("../ui/js/core.js")),
    ("/ui/js/apps.js", JS, include_str!("../ui/js/apps.js")),
    ("/ui/js/disk.js", JS, include_str!("../ui/js/disk.js")),
    (
        "/ui/js/resources.js",
        JS,
        include_str!("../ui/js/resources.js"),
    ),
    (
        "/ui/js/settings.js",
        JS,
        include_str!("../ui/js/settings.js"),
    ),
    ("/ui/js/demo.js", JS, include_str!("../ui/js/demo.js")),
];
const JS: &str = "text/javascript; charset=utf-8";

async fn info(State(a): State<App>) -> Json<Value> {
    let settings = a.settings.get();
    let update = a.updater.state.lock().unwrap().clone();
    let offer = update.latest.as_ref().filter(|r| {
        settings.updates.check
            && update::is_newer(&r.version)
            && settings.updates.skipped.as_deref() != Some(r.version.as_str())
            && now() >= settings.updates.snoozed_until
    });
    Json(json!({
        "version": update::VERSION,
        "repo": update::REPO,
        "elevated": crate::elevation::is_elevated(),
        "data_dir": a.data_dir.display().to_string(),
        "start_with_windows": crate::autostart::is_enabled(),
        "tools": tools::ALL
            .iter()
            .map(|t| json!({ "id": t.id, "name": t.name, "about": t.about, "on": settings.tool_on(t.id) }))
            .collect::<Vec<_>>(),
        "settings": settings,
        "update": update,
        "update_offer": offer,
    }))
}

#[derive(Deserialize)]
struct SettingsChange {
    theme: Option<String>,
    /// Tool id -> on/off.
    tools: Option<BTreeMap<String, bool>>,
    check_updates: Option<bool>,
    start_with_windows: Option<bool>,
}

async fn change_settings(State(a): State<App>, Json(c): Json<SettingsChange>) -> Response {
    if let Some(on) = c.start_with_windows {
        let result = if on {
            crate::autostart::enable(&a.exe)
        } else {
            crate::autostart::disable()
        };
        if let Err(e) = result {
            return bad_request(e);
        }
    }
    if let Some((id, _)) = c
        .tools
        .iter()
        .flatten()
        .find(|(id, _)| tools::find(id).is_none())
    {
        return bad_request(anyhow::anyhow!("Unknown tool {id:?}."));
    }
    if let Some(t) = &c.theme {
        if !["system", "light", "dark"].contains(&t.as_str()) {
            return bad_request(anyhow::anyhow!("Unknown theme {t:?}."));
        }
    }
    let result = a.settings.update(|s| {
        if let Some(t) = c.theme {
            s.theme = t;
        }
        for (id, on) in c.tools.iter().flatten() {
            s.tools.insert(id.clone(), *on);
        }
        if let Some(v) = c.check_updates {
            s.updates.check = v;
        }
    });
    if let Ok(s) = &result {
        for t in tools::ALL {
            a.tools.set_enabled(t.id, s.tool_on(t.id));
        }
    }
    ok(result.map(|_| ()))
}

async fn check_update(State(a): State<App>) -> Response {
    ok(a.updater.check().await)
}

async fn install_update(State(a): State<App>) -> Response {
    ok(a.updater.install().await)
}

async fn snooze_update(State(a): State<App>) -> Response {
    ok(a.settings
        .update(|s| s.updates.snoozed_until = now() + 3 * 24 * 3600)
        .map(|_| ()))
}

async fn skip_update(State(a): State<App>) -> Response {
    let latest = a
        .updater
        .state
        .lock()
        .unwrap()
        .latest
        .as_ref()
        .map(|r| r.version.clone());
    ok(a.settings
        .update(|s| s.updates.skipped = latest)
        .map(|_| ()))
}

async fn quit(State(a): State<App>) -> StatusCode {
    (a.quit)();
    StatusCode::ACCEPTED
}

/// 204 on success; 400 with `{ "error": "..." }` (a sentence for the user) on failure.
pub fn ok(result: anyhow::Result<()>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bad_request(e),
    }
}

pub fn bad_request(e: anyhow::Error) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": format!("{e:#}") })),
    )
        .into_response()
}
