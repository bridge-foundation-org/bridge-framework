//! Shared daemon state — referenced by every server module.
//!
//! Everything lives in one `State` struct wrapped in `Arc<Mutex<State>>`.

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::SystemTime;

use compiler::BridgeFile;
use db::Db;
use protocol::DaemonMode;

use crate::metrics::Registry as MetricsRegistry;
use crate::middleware::MiddlewareRegistry;
use crate::pubsub::Broker;
use crate::secrets::SecretsRegistry;
use crate::streaming::StreamRegistry;

// ── Trace entry ───────────────────────────────────────────────────────────────

/// A single recorded request trace.
#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub id:          String,
    pub method:      String,
    pub path:        String,
    pub status:      u16,
    pub duration_ms: u64,
    pub timestamp:   u64, // Unix seconds
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

// ── Metrics ───────────────────────────────────────────────────────────────────

/// In-memory metrics store.
#[derive(Debug, Default, Clone)]
pub struct Metrics {
    /// Total requests by method+path key
    pub request_counts:    HashMap<String, u64>,
    /// Error counts (status >= 400)
    pub error_counts:      HashMap<String, u64>,
    /// Cumulative durations (ms) for average calculation
    pub duration_totals:   HashMap<String, u64>,
    /// Global counters
    pub total_requests:    u64,
    pub total_errors:      u64,
}

impl Metrics {
    pub fn record(&mut self, method: &str, path: &str, status: u16, duration_ms: u64) {
        let key = format!("{} {}", method, path);
        *self.request_counts.entry(key.clone()).or_insert(0) += 1;
        *self.duration_totals.entry(key.clone()).or_insert(0) += duration_ms;
        self.total_requests += 1;
        if status >= 400 {
            *self.error_counts.entry(key).or_insert(0) += 1;
            self.total_errors += 1;
        }
    }

    pub fn to_json(&self) -> String {
        let mut entries = Vec::new();
        for (key, count) in &self.request_counts {
            let errs    = self.error_counts.get(key).copied().unwrap_or(0);
            let dur_tot = self.duration_totals.get(key).copied().unwrap_or(0);
            let avg_ms  = if *count > 0 { dur_tot / count } else { 0 };
            entries.push(format!(
                r#"{{"endpoint":"{key}","requests":{count},"errors":{errs},"avg_ms":{avg_ms}}}"#
            ));
        }
        format!(
            r#"{{"total_requests":{},"total_errors":{},"endpoints":[{}]}}"#,
            self.total_requests, self.total_errors,
            entries.join(",")
        )
    }
}

// ── Structured log ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel { Debug, Info, Warn, Error }

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self { Self::Debug => "DEBUG", Self::Info => "INFO",
                     Self::Warn  => "WARN",  Self::Error => "ERROR" }
    }
}

pub struct LogEntry {
    pub level:     LogLevel,
    pub message:   String,
    pub timestamp: u64,
    pub fields:    HashMap<String, String>,
}

impl LogEntry {
    pub fn to_json(&self) -> String {
        let fields: Vec<String> = self.fields.iter()
            .map(|(k, v)| format!(r#""{k}":"{v}""#)).collect();
        let fields_json = if fields.is_empty() {
            String::new()
        } else {
            format!(",{}", fields.join(","))
        };
        format!(
            r#"{{"ts":{},"level":"{}","msg":"{}"{}}}"#,
            self.timestamp, self.level.as_str(), self.message, fields_json
        )
    }
}

/// Write a structured log line to stderr.
pub fn log(level: LogLevel, msg: &str, fields: &[(&str, &str)]) {
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);
    let fstr: Vec<String> = fields.iter()
        .map(|(k, v)| format!(r#""{k}":"{v}""#)).collect();
    let extra = if fstr.is_empty() { String::new() } else { format!(" {}", fstr.join(" ")) };
    eprintln!("[bridge] ts={ts} level={} msg={:?}{}", level.as_str(), msg, extra);
}

// ── State ─────────────────────────────────────────────────────────────────────

pub struct State {
    /// Running mode: lite | full | ultra | off.
    pub mode:              DaemonMode,
    /// In-memory KV store.
    pub store:             Db,
    /// Active auth token.
    pub auth_token:        Option<String>,
    /// Last parsed Bridge file (service registry).
    pub service_registry:  Option<BridgeFile>,
    /// Recent request traces (capped at MAX_TRACES).
    pub traces:            Vec<TraceEntry>,
    /// In-memory metrics (legacy simple counters).
    pub metrics:           Metrics,
    /// Full metrics registry with Prometheus support.
    pub metric_registry:   MetricsRegistry,
    /// Recent log entries (capped at MAX_LOGS).
    pub logs:              Vec<LogEntry>,
    /// Miniredis address.
    pub redis_addr:        Option<String>,
    /// Miniredis live connection counter.
    pub redis_connections: Option<Arc<AtomicUsize>>,
    /// Sampling rate 0.0–1.0 (1.0 = 100%).
    pub trace_sample_rate: f64,
    /// App metadata
    pub app_name:          String,
    pub app_version:       String,
    /// Pub/Sub broker.
    pub pubsub:            Broker,
    /// Secrets registry.
    pub secrets:           SecretsRegistry,
    /// Streaming endpoints registry.
    pub streams:           StreamRegistry,
    /// Middleware registry.
    pub middleware:        MiddlewareRegistry,
    /// Monotonically increasing trace ID counter.
    trace_counter:         u64,
    /// RNG state for sampling (simple LCG).
    rng_state:             u64,
}

const MAX_TRACES: usize = 500;
const MAX_LOGS:   usize = 1000;

impl State {
    pub fn new(redis_addr: Option<String>, redis_connections: Option<Arc<AtomicUsize>>) -> Self {
        let metric_registry = MetricsRegistry::new();
        crate::metrics::register_defaults(&metric_registry);
        Self {
            mode:              DaemonMode::Full,
            store:             Db::new(),
            auth_token:        None,
            service_registry:  None,
            traces:            Vec::new(),
            metrics:           Metrics::default(),
            metric_registry,
            logs:              Vec::new(),
            redis_addr,
            redis_connections,
            trace_sample_rate: 1.0,
            app_name:          "bridge".to_string(),
            app_version:       protocol::VERSION.to_string(),
            pubsub:            Broker::new(),
            secrets:           SecretsRegistry::new(),
            streams:           StreamRegistry::new(),
            middleware:        MiddlewareRegistry::new(),
            trace_counter:     0,
            rng_state:         12345678901234567,
        }
    }

    /// Record a trace entry (respects sampling rate).
    pub fn push_trace(&mut self, method: &str, path: &str, status: u16, duration_ms: u64) {
        // Record metrics unconditionally
        self.metrics.record(method, path, status, duration_ms);

        // Respect sampling rate for trace storage
        if !self.should_sample() { return; }

        self.trace_counter += 1;
        let id = format!("t{:08}", self.trace_counter);
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs()).unwrap_or(0);
        self.traces.push(TraceEntry {
            id, method: method.to_string(), path: path.to_string(),
            status, duration_ms, timestamp: ts,
        });
        if self.traces.len() > MAX_TRACES {
            self.traces.remove(0);
        }
    }

    /// Decide whether to sample this trace (simple LCG RNG).
    pub fn should_sample(&mut self) -> bool {
        if self.trace_sample_rate >= 1.0 { return true; }
        if self.trace_sample_rate <= 0.0 { return false; }
        self.rng_state = self.rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let r = (self.rng_state >> 33) as f64 / (u32::MAX as f64);
        r < self.trace_sample_rate
    }

    /// Find a trace by ID.
    pub fn find_trace(&self, id: &str) -> Option<&TraceEntry> {
        self.traces.iter().find(|t| t.id == id)
    }

    /// Add a structured log entry.
    pub fn push_log(&mut self, level: LogLevel, message: &str, fields: HashMap<String, String>) {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs()).unwrap_or(0);
        self.logs.push(LogEntry { level, message: message.to_string(), timestamp: ts, fields });
        if self.logs.len() > MAX_LOGS { self.logs.remove(0); }
    }

    /// Current Redis connection count.
    pub fn redis_connections_count(&self) -> usize {
        self.redis_connections.as_ref()
            .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Build a full health JSON object.
    pub fn health_json(&self) -> String {
        let redis  = self.redis_addr.as_deref().unwrap_or("off");
        let conns  = self.redis_connections_count();
        let svcs   = self.service_registry.as_ref().map(|f| f.services.len()).unwrap_or(0);
        let traces = self.traces.len();
        format!(
            r#"{{"status":"ok","version":"{ver}","app":"{app}","mode":"{mode}","redis":"{redis}","redis_connections":{conns},"services":{svcs},"traces":{traces},"sample_rate":{rate}}}"#,
            ver   = self.app_version,
            app   = self.app_name,
            mode  = self.mode,
            rate  = self.trace_sample_rate,
        )
    }
}

/// Shared handle type.
pub type SharedState = Arc<std::sync::Mutex<State>>;
