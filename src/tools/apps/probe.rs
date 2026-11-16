//! Probes: ways to tell whether something is true about an app (its process
//! runs, its port answers, its HTTP endpoint is up). Add a variant here to
//! teach the manager a new way of detecting trouble.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::env::expand;
use super::processes::{ProcessMatch, ProcessTable};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Probe {
    /// A matching process is running.
    Process(ProcessMatch),
    /// The URL answers with a status below 500 within the timeout.
    Http {
        url: String,
        #[serde(default = "ten")]
        timeout_secs: u64,
    },
    /// Something accepts TCP connections on the port.
    Tcp {
        #[serde(default = "localhost")]
        host: String,
        port: u16,
        #[serde(default = "three")]
        timeout_secs: u64,
    },
    /// The file exists (for example a "stopped on purpose" marker).
    FileExists { path: String },
}

fn ten() -> u64 {
    10
}
fn three() -> u64 {
    3
}
fn localhost() -> String {
    "127.0.0.1".into()
}

/// Outcome of one probe: whether it passed and a short human reason.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeResult {
    pub ok: bool,
    pub detail: String,
}

impl ProbeResult {
    fn new(ok: bool, detail: impl Into<String>) -> Self {
        Self {
            ok,
            detail: detail.into(),
        }
    }
}

impl Probe {
    pub async fn run(&self, http: &reqwest::Client, processes: &ProcessTable) -> ProbeResult {
        match self {
            Probe::Process(m) => match processes.find(m).first() {
                Some(pid) => ProbeResult::new(true, format!("{} running (pid {pid})", m.name)),
                None => ProbeResult::new(false, format!("{} not running", m.name)),
            },
            Probe::Http { url, timeout_secs } => {
                let url = expand(url);
                match http
                    .get(&url)
                    .timeout(Duration::from_secs(*timeout_secs))
                    .send()
                    .await
                {
                    Ok(r) if r.status().as_u16() < 500 => {
                        ProbeResult::new(true, format!("{url} -> {}", r.status()))
                    }
                    Ok(r) => ProbeResult::new(false, format!("{url} -> {}", r.status())),
                    Err(e) => {
                        ProbeResult::new(false, format!("{url} unreachable: {}", short_error(&e)))
                    }
                }
            }
            Probe::Tcp {
                host,
                port,
                timeout_secs,
            } => {
                let addr = format!("{host}:{port}");
                let connect = tokio::net::TcpStream::connect(addr.clone());
                match tokio::time::timeout(Duration::from_secs(*timeout_secs), connect).await {
                    Ok(Ok(_)) => ProbeResult::new(true, format!("{addr} accepting")),
                    Ok(Err(e)) => ProbeResult::new(false, format!("{addr} refused: {e}")),
                    Err(_) => ProbeResult::new(false, format!("{addr} timed out")),
                }
            }
            Probe::FileExists { path } => {
                let path = expand(path);
                let exists = std::path::Path::new(&path).exists();
                ProbeResult::new(
                    exists,
                    format!("{path} {}", if exists { "exists" } else { "absent" }),
                )
            }
        }
    }
}

fn short_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timed out".into()
    } else if e.is_connect() {
        "connection refused".into()
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tcp_probe_sees_listener_and_its_absence() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let http = reqwest::Client::new();
        let table = ProcessTable::default();
        let probe = Probe::Tcp {
            host: localhost(),
            port,
            timeout_secs: 3,
        };
        assert!(probe.run(&http, &table).await.ok);
        drop(listener);
        assert!(!probe.run(&http, &table).await.ok);
    }

    #[tokio::test]
    async fn file_probe() {
        let http = reqwest::Client::new();
        let table = ProcessTable::default();
        let here = Probe::FileExists {
            path: "Cargo.toml".into(),
        };
        let gone = Probe::FileExists {
            path: "no-such-file".into(),
        };
        assert!(here.run(&http, &table).await.ok);
        assert!(!gone.run(&http, &table).await.ok);
    }
}
