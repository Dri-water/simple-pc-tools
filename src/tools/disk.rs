//! Disk usage: scans a drive or folder (see the `disk-scan` crate) and lets
//! the UI browse the result, biggest first. Read-only: it never deletes.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use disk_scan::{Method, NodeId, Progress, Tree};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::bad_request;

/// Children listed per folder; the rest are summed into one "other" row.
const LIST_LIMIT: usize = 400;

#[derive(Default)]
pub struct Disk {
    scan: Mutex<Option<Scan>>,
}

struct Scan {
    path: PathBuf,
    progress: Arc<Progress>,
    started: Instant,
    elapsed: Option<f64>,
    result: Option<Result<Arc<Tree>, String>>,
}

type Shared = State<Arc<Disk>>;

pub fn router(disk: Arc<Disk>) -> Router {
    Router::new()
        .route("/drives", get(drives))
        .route("/scan", post(start_scan))
        .route("/cancel", post(cancel))
        .route("/status", get(status))
        .route("/list", get(list))
        .route("/reveal", post(reveal))
        .route("/pick", post(pick_folder))
        .with_state(disk)
}

async fn drives() -> Json<Value> {
    let list: Vec<Value> = disk_scan::drives()
        .into_iter()
        .map(|d| {
            json!({
                "root": d.root.display().to_string(),
                "label": d.label,
                "file_system": d.file_system,
                "total": d.total,
                "free": d.free,
            })
        })
        .collect();
    Json(json!({ "drives": list, "fast": disk_scan::is_elevated() }))
}

#[derive(Deserialize)]
struct ScanRequest {
    path: String,
}

async fn start_scan(State(d): Shared, Json(req): Json<ScanRequest>) -> Response {
    let path = PathBuf::from(req.path.trim());
    if !path.is_dir() {
        return bad_request(anyhow::anyhow!(
            "{} isn't a folder or drive.",
            path.display()
        ));
    }
    let progress = Arc::new(Progress::default());
    {
        let mut slot = d.scan.lock().unwrap();
        if let Some(old) = slot.as_ref() {
            old.progress.cancel.store(true, Ordering::Relaxed);
        }
        *slot = Some(Scan {
            path: path.clone(),
            progress: progress.clone(),
            started: Instant::now(),
            elapsed: None,
            result: None,
        });
    }
    let d2 = d.clone();
    tokio::task::spawn_blocking(move || {
        let result = disk_scan::scan(&path, &progress)
            .map(Arc::new)
            .map_err(|e| format!("{e:#}"));
        let mut slot = d2.scan.lock().unwrap();
        // Only record it if this is still the current scan.
        if let Some(s) = slot
            .as_mut()
            .filter(|s| Arc::ptr_eq(&s.progress, &progress))
        {
            s.elapsed = Some(s.started.elapsed().as_secs_f64());
            s.result = Some(result);
        }
    });
    StatusCode::ACCEPTED.into_response()
}

async fn cancel(State(d): Shared) -> StatusCode {
    if let Some(s) = d.scan.lock().unwrap().as_ref() {
        s.progress.cancel.store(true, Ordering::Relaxed);
    }
    StatusCode::NO_CONTENT
}

async fn status(State(d): Shared) -> Json<Value> {
    let slot = d.scan.lock().unwrap();
    let Some(s) = slot.as_ref() else {
        return Json(json!({ "state": "idle" }));
    };
    let p = &s.progress;
    let mut v = json!({
        "path": s.path.display().to_string(),
        "files": p.files.load(Ordering::Relaxed),
        "bytes": p.bytes.load(Ordering::Relaxed),
        "phase": p.phase.lock().map(|x| x.clone()).unwrap_or_default(),
        "elapsed": s.elapsed.unwrap_or_else(|| s.started.elapsed().as_secs_f64()),
    });
    match &s.result {
        None => v["state"] = json!("scanning"),
        Some(Err(e)) => {
            v["state"] = json!("error");
            v["error"] = json!(e);
        }
        Some(Ok(tree)) => {
            v["state"] = json!("done");
            v["method"] = json!(match tree.method {
                Method::FileTable => "file_table",
                Method::FolderWalk => "folder_walk",
            });
            v["skipped"] = json!(tree.skipped.len());
            v["skipped_examples"] = json!(tree
                .skipped
                .iter()
                .take(5)
                .map(|(p, why)| format!("{}: {why}", p.display()))
                .collect::<Vec<_>>());
        }
    }
    Json(v)
}

#[derive(Deserialize)]
struct ListQuery {
    /// Node id; the scanned root when absent.
    id: Option<NodeId>,
}

fn node_json(tree: &Tree, id: NodeId, parent_size: u64) -> Value {
    let n = &tree.nodes[id as usize];
    json!({
        "id": id,
        "name": &*n.name,
        "is_dir": n.is_dir,
        "size": n.size,
        "allocated": n.allocated,
        "files": n.files,
        "dirs": n.dirs,
        "modified": n.modified,
        "share": if parent_size == 0 { 0.0 } else { n.size as f64 / parent_size as f64 },
        "has_children": !tree.children(id).is_empty(),
    })
}

async fn list(State(d): Shared, Query(q): Query<ListQuery>) -> Response {
    let tree = {
        let slot = d.scan.lock().unwrap();
        match slot.as_ref().and_then(|s| s.result.as_ref()) {
            Some(Ok(t)) => t.clone(),
            _ => {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({ "error": "No finished scan yet." })),
                )
                    .into_response()
            }
        }
    };
    let id = q.id.unwrap_or(tree.root);
    if id as usize >= tree.nodes.len() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let node = &tree.nodes[id as usize];
    let kids = tree.children(id);
    let shown: Vec<Value> = kids
        .iter()
        .take(LIST_LIMIT)
        .map(|&c| node_json(&tree, c, node.size))
        .collect();
    let rest = &kids[kids.len().min(LIST_LIMIT)..];
    let other = (!rest.is_empty()).then(|| {
        let size: u64 = rest.iter().map(|&c| tree.nodes[c as usize].size).sum();
        json!({ "count": rest.len(), "size": size })
    });
    // Breadcrumb from the root down to this node.
    let mut crumbs = Vec::new();
    let mut cur = id;
    loop {
        crumbs.push(json!({ "id": cur, "name": &*tree.nodes[cur as usize].name }));
        if cur == tree.root || tree.nodes[cur as usize].parent == cur {
            break;
        }
        cur = tree.nodes[cur as usize].parent;
    }
    crumbs.reverse();
    Json(json!({
        "node": node_json(&tree, id, node.size),
        "path": tree.path(id).display().to_string(),
        "crumbs": crumbs,
        "children": shown,
        "other": other,
    }))
    .into_response()
}

#[derive(Deserialize)]
struct RevealRequest {
    id: NodeId,
}

/// Opens File Explorer with the item selected.
async fn reveal(State(d): Shared, Json(r): Json<RevealRequest>) -> Response {
    let path = {
        let slot = d.scan.lock().unwrap();
        match slot.as_ref().and_then(|s| s.result.as_ref()) {
            Some(Ok(t)) if (r.id as usize) < t.nodes.len() => t.path(r.id),
            _ => return StatusCode::NOT_FOUND.into_response(),
        }
    };
    if !path.exists() {
        return bad_request(anyhow::anyhow!("{} no longer exists.", path.display()));
    }
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("explorer.exe");
    cmd.raw_arg(format!("/select,\"{}\"", path.display()));
    match cmd.spawn() {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bad_request(e.into()),
    }
}

async fn pick_folder() -> Json<Value> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Choose a folder to scan")
        .pick_folder()
        .await;
    Json(json!({ "path": folder.map(|f| f.path().display().to_string()) }))
}
