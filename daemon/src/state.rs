//! Shared daemon state — referenced by every server module.
//!
//! Everything lives in one `State` struct wrapped in `Arc<Mutex<State>>`.
//! All fields are public so tcp.rs / http.rs can read them without extra
//! accessor boilerplate.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::SystemTime;

use compiler::BridgeFile;
use db::Db;
use protocol::DaemonMode;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single recorded request trace.
#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub id: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub duration_ms: u64,
    pub timestamp: u64, // Unix seconds
}

impl TraceEntry {
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"id":"{id}","method":"{method}","path":"{path}","status":{status},"duration_ms":{dur},"timestamp":{ts}}}"#,
            id = self.id, method = self.method, path = self.path,
            status = self.status, dur = self.duration_ms, ts = self.timestamp,
        )
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Central daemon state — wrap in `Arc<Mutex<State>>`.
pub struct State {
    /// Running mode: lite | full | ultra | off.
    pub mode: DaemonMode,
    /// In-memory KV store (code-gen cache, session data, etc.).
    pub store: Db,
    /// Active auth token (set via `AUTH SET`).
    pub auth_token: Option<String>,
    /// Last parsed Bridge file (service registry).
    pub service_registry: Option<BridgeFile>,
    /// Recent request traces (capped at `MAX_TRACES`).
    pub traces: Vec<TraceEntry>,
    /// Miniredis address for status reporting.
    pub redis_addr: Option<String>,
    /// Miniredis live connection counter.
    pub redis_connections: Option<Arc<AtomicUsize>>,
    /// Monotonically increasing trace ID counter.
    trace_counter: u64,
}

const MAX_TRACES: usize = 500;

impl State {
    pub fn new(redis_addr: Option<String>, redis_connections: Option<Arc<AtomicUsize>>) -> Self {
        Self {
            mode: DaemonMode::Full,
            store: Db::new(),
            auth_token: None,
            service_registry: None,
            traces: Vec::new(),
            redis_addr,
            redis_connections,
            trace_counter: 0,
        }
    }

    /// Record a trace entry; oldest entries are discarded once limit is hit.
    pub fn push_trace(&mut self, method: &str, path: &str, status: u16, duration_ms: u64) {
        self.trace_counter += 1;
        let id = format!("t{:08}", self.trace_counter);
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.traces.push(TraceEntry { id, method: method.to_string(), path: path.to_string(), status, duration_ms, timestamp: ts });
        if self.traces.len() > MAX_TRACES {
            self.traces.remove(0);
        }
    }

    /// Find a trace by ID.
    pub fn find_trace(&self, id: &str) -> Option<&TraceEntry> {
        self.traces.iter().find(|t| t.id == id)
    }

    /// Current Redis connection count (0 if Redis is not running).
    pub fn redis_connections_count(&self) -> usize {
        self.redis_connections.as_ref()
            .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(0)
    }
}

/// Shared handle type.
pub type SharedState = Arc<std::sync::Mutex<State>>;
