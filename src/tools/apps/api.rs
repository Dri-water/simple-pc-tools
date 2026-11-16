//! JSON API for the Apps page.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use super::supervisor::{Manager, Trigger};
use super::App;
use crate::api::{bad_request, ok};

type Shared = State<Arc<Manager>>;

pub fn router(manager: Arc<Manager>) -> Router {
    Router::new()
        .route("/", get(list).post(create))
        .route("/settings", patch(settings))
        .route("/check", post(check_now))
        .route("/pick", post(pick_program))
        .route("/{id}", patch(toggle).put(replace).delete(remove))
        .route("/{id}/restart", post(restart))
        .route("/{id}/stop", post(stop))
        .with_state(manager)
}

async fn list(State(m): Shared) -> Json<Value> {
    let store = m.store.lock().unwrap().clone();
    let status = m.status.lock().unwrap().clone();
    let apps: Vec<Value> = store
        .apps
        .iter()
        .map(|app| {
            let mut v = serde_json::to_value(app).unwrap_or_default();
            v["status"] = json!(status.get(&app.id));
            v["watched_process"] = json!(app.process().map(|p| p.name));
            v
        })
        .collect();
    Json(json!({
        "paused": store.paused,
        "check_every_secs": store.check_every_secs,
        "next_check": *m.next_check.lock().unwrap(),
        "apps": apps,
    }))
}

async fn create(State(m): Shared, Json(mut app): Json<App>) -> Response {
    let result = m.update(|s| {
        app.id = s.new_id(&app.name);
        app.validate()?;
        s.apps.push(app.clone());
        Ok(app.id.clone())
    });
    match result {
        Ok(id) => {
            let m2 = m.clone();
            let id2 = id.clone();
            tokio::spawn(async move { m2.check_one(&id2, Trigger::Launch).await });
            (StatusCode::CREATED, Json(json!({ "id": id }))).into_response()
        }
        Err(e) => bad_request(e),
    }
}

async fn replace(State(m): Shared, Path(id): Path<String>, Json(mut app): Json<App>) -> Response {
    app.id = id.clone();
    let result = m.update(|s| {
        app.validate()?;
        let Some(slot) = s.apps.iter_mut().find(|a| a.id == id) else {
            anyhow::bail!("That app no longer exists.");
        };
        *slot = app;
        Ok(())
    });
    m.wake.notify_one();
    ok(result)
}

async fn remove(State(m): Shared, Path(id): Path<String>) -> Response {
    ok(m.update(|s| {
        s.apps.retain(|a| a.id != id);
        Ok(())
    }))
}

#[derive(Deserialize)]
struct Toggle {
    start_at_login: Option<bool>,
    keep_running: Option<bool>,
}

async fn toggle(State(m): Shared, Path(id): Path<String>, Json(t): Json<Toggle>) -> Response {
    let result = m.update(|s| {
        let Some(app) = s.apps.iter_mut().find(|a| a.id == id) else {
            anyhow::bail!("That app no longer exists.");
        };
        if let Some(v) = t.start_at_login {
            app.start_at_login = v;
        }
        if let Some(v) = t.keep_running {
            app.keep_running = v;
        }
        app.validate()
    });
    m.wake.notify_one();
    ok(result)
}

#[derive(Deserialize)]
struct SettingsChange {
    paused: Option<bool>,
    check_every_secs: Option<u64>,
}

async fn settings(State(m): Shared, Json(c): Json<SettingsChange>) -> Response {
    let result = m.update(|s| {
        if let Some(p) = c.paused {
            s.paused = p;
        }
        if let Some(secs) = c.check_every_secs {
            anyhow::ensure!(
                (5..=86_400).contains(&secs),
                "Check between every 5 seconds and once a day."
            );
            s.check_every_secs = secs;
        }
        Ok(())
    });
    m.wake.notify_one();
    ok(result)
}

async fn check_now(State(m): Shared) -> StatusCode {
    m.wake.notify_one();
    StatusCode::ACCEPTED
}

async fn restart(State(m): Shared, Path(id): Path<String>) -> StatusCode {
    let Some(app) = m.app(&id) else {
        return StatusCode::NOT_FOUND;
    };
    tokio::spawn(async move { m.restart(&app).await });
    StatusCode::ACCEPTED
}

async fn stop(State(m): Shared, Path(id): Path<String>) -> StatusCode {
    let Some(app) = m.app(&id) else {
        return StatusCode::NOT_FOUND;
    };
    tokio::spawn(async move { m.stop(&app).await });
    StatusCode::ACCEPTED
}

/// Opens Windows' file picker so the user can choose a program.
async fn pick_program() -> Json<Value> {
    let file = rfd::AsyncFileDialog::new()
        .set_title("Choose a program to start")
        .add_filter("Programs and scripts", &["exe", "bat", "cmd", "ps1", "lnk"])
        .add_filter("All files", &["*"])
        .pick_file()
        .await;
    Json(match file {
        Some(f) => {
            let path = f.path();
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            json!({ "program": path.display().to_string(), "name": nice_name(&stem) })
        }
        None => json!({ "program": null }),
    })
}

/// "my_server-app" -> "My server app".
fn nice_name(stem: &str) -> String {
    let words = stem.replace(['_', '-', '.'], " ");
    let mut c = words.trim().chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
