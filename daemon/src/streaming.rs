//! Streaming endpoints for real-time data
//!
//! Implements Server-Sent Events (SSE) for streaming data to clients.
//! No external dependencies - uses only std.
//!
//! Supports:
//! - Request trace streaming
//! - Metrics streaming
//! - Service catalog streaming
//! - Custom event types

// Parts of this module are forward-scaffolding: their public API is
// intentionally ahead of its call sites. Trim this allow item-by-item as the
// dead surface shrinks.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::SystemTime;

// ── SSE Frame ──────────────────────────────────────────────────────────────

pub struct SseFrame {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
}

impl SseFrame {
    pub fn new(event: impl Into<String>, data: impl Into<String>) -> Self {
        SseFrame {
            event: event.into(),
            data: data.into(),
            id: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Encode SSE frame to HTTP-compatible format
    pub fn encode(&self) -> String {
        let mut output = String::new();

        if let Some(id) = &self.id {
            output.push_str("id: ");
            output.push_str(id);
            output.push('\n');
        }

        output.push_str("event: ");
        output.push_str(&self.event);
        output.push('\n');

        for line in self.data.lines() {
            output.push_str("data: ");
            output.push_str(line);
            output.push('\n');
        }

        output.push('\n');
        output
    }
}

// ── Streaming Events ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum StreamEvent {
    /// Trace event: {"trace_id", "service", "method", "path", "status", "duration_ms"}
    Trace {
        trace_id: String,
        service: String,
        method: String,
        path: String,
        status: u16,
        duration_ms: u64,
    },
    /// Metrics snapshot: {"requests_total", "latency_p50", "latency_p99"}
    Metrics {
        requests_total: u64,
        latency_p50: u64,
        latency_p99: u64,
        success_rate: f32,
    },
    /// Service update: {"name", "endpoints", "auth"}
    Service {
        name: String,
        endpoints: u32,
        auth: String,
    },
    /// Custom event
    Custom { event_type: String, payload: String },
}

impl StreamEvent {
    pub fn to_sse_frame(&self) -> SseFrame {
        match self {
            StreamEvent::Trace {
                trace_id,
                service,
                method,
                path,
                status,
                duration_ms,
            } => {
                let data = format!(
                    r#"{{"trace_id":"{}","service":"{}","method":"{}","path":"{}","status":{},"duration_ms":{}}}"#,
                    trace_id, service, method, path, status, duration_ms
                );
                SseFrame::new("trace", data).with_id(trace_id.clone())
            }
            StreamEvent::Metrics {
                requests_total,
                latency_p50,
                latency_p99,
                success_rate,
            } => {
                let data = format!(
                    r#"{{"requests_total":{},"latency_p50":{},"latency_p99":{},"success_rate":{}}}"#,
                    requests_total, latency_p50, latency_p99, success_rate
                );
                let ts = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis().to_string())
                    .unwrap_or_default();
                SseFrame::new("metrics", data).with_id(ts)
            }
            StreamEvent::Service {
                name,
                endpoints,
                auth,
            } => {
                let data = format!(
                    r#"{{"name":"{}","endpoints":{},"auth":"{}"}}"#,
                    name, endpoints, auth
                );
                SseFrame::new("service", data).with_id(name.clone())
            }
            StreamEvent::Custom {
                event_type,
                payload,
            } => SseFrame::new(event_type, payload.clone()),
        }
    }
}

// ── Stream Buffer ──────────────────────────────────────────────────────────

pub struct StreamBuffer {
    events: Arc<Mutex<Vec<StreamEvent>>>,
    capacity: usize,
}

impl StreamBuffer {
    pub fn new(capacity: usize) -> Self {
        StreamBuffer {
            events: Arc::new(Mutex::new(Vec::with_capacity(capacity))),
            capacity,
        }
    }

    pub fn push(&self, event: StreamEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
            if events.len() > self.capacity {
                events.remove(0); // Keep only last N events
            }
        }
    }

    pub fn get_recent(&self, count: usize) -> Vec<StreamEvent> {
        self.events
            .lock()
            .ok()
            .map(|events| {
                let start = if events.len() > count {
                    events.len() - count
                } else {
                    0
                };
                events[start..].to_vec()
            })
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut events) = self.events.lock() {
            events.clear();
        }
    }
}

impl Clone for StreamBuffer {
    fn clone(&self) -> Self {
        StreamBuffer {
            events: Arc::clone(&self.events),
            capacity: self.capacity,
        }
    }
}

// ── Streaming Endpoint Registry ────────────────────────────────────────────
//
// Tracks the catalog of streamable endpoints and their live sessions.
// An "endpoint" is a known SSE path; a "session" is one connected client.
// Sessions are opened by HTTP handlers (`handle_sse_*` in http.rs) and
// closed on disconnect, so `active_count()` reflects reality.

/// Known streaming endpoints served by the daemon.
pub const STREAM_ENDPOINTS: &[(&str, &str)] = &[
    ("/api/v1/stream/traces", "request trace events"),
    ("/api/v1/stream/metrics", "metrics snapshots"),
    ("/api/v1/stream/services", "service catalog updates"),
];

#[derive(Default)]
pub struct StreamRegistry {
    /// Live session IDs per endpoint path.
    active: std::collections::HashMap<String, Vec<String>>,
    /// Monotonic session counter.
    counter: u64,
}

impl StreamRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Catalog JSON: known endpoints plus per-endpoint active session counts.
    pub fn endpoints_json(&self) -> String {
        let eps: Vec<String> = STREAM_ENDPOINTS
            .iter()
            .map(|(path, desc)| {
                let n = self.active.get(*path).map(|v| v.len()).unwrap_or(0);
                format!(r#"{{"path":"{path}","description":"{desc}","active":{n}}}"#)
            })
            .collect();
        let total: usize = self.active.values().map(|v| v.len()).sum();
        format!(
            r#"{{"endpoints":[{}],"count":{},"active":{}}}"#,
            eps.join(","),
            STREAM_ENDPOINTS.len(),
            total
        )
    }

    /// Number of currently-open stream sessions across all endpoints.
    pub fn active_count(&self) -> u32 {
        self.active.values().map(|v| v.len() as u32).sum()
    }

    /// Look up an endpoint by path. Returns its canonical path when known,
    /// so callers can normalize before opening a session.
    pub fn open(&self, path: &str) -> Option<String> {
        STREAM_ENDPOINTS
            .iter()
            .find(|(p, _)| *p == path || path.trim_end_matches('/') == *p)
            .map(|(p, _)| p.to_string())
    }

    /// Register a live session on an endpoint; returns the session ID.
    pub fn set_open(&mut self, endpoint_path: &str) -> String {
        self.counter += 1;
        let id = format!("s{:06}", self.counter);
        self.active
            .entry(endpoint_path.to_string())
            .or_default()
            .push(id.clone());
        id
    }

    /// Remove a session by ID from whichever endpoint holds it.
    pub fn close(&mut self, id: &str) -> bool {
        for sessions in self.active.values_mut() {
            if let Some(pos) = sessions.iter().position(|s| s == id) {
                sessions.remove(pos);
                return true;
            }
        }
        false
    }
}

// ── Streaming Snapshot Renderers ───────────────────────────────────────
//
// One-shot renders of daemon state as SSE frames. The HTTP layer polls
// these on an interval while a client stays connected, so each function
// is pure state → text with no I/O.

use crate::state::State;

/// Recent traces as SSE frames (oldest first), capped at `count`.
pub fn render_traces(state: &State, count: usize) -> String {
    let recent: Vec<&crate::state::TraceEntry> = state.traces.iter().rev().take(count).collect();
    let mut out = String::new();
    for t in recent.iter().rev() {
        let frame = SseFrame::new("trace", t.to_json()).with_id(t.id.clone());
        out.push_str(&frame.encode());
    }
    out
}

/// Current metrics snapshot as a single SSE frame.
pub fn render_metrics(state: &State) -> String {
    // Aggregate latency stats from per-endpoint totals.
    let total_dur: u64 = state.metrics.duration_totals.values().sum();
    let total_req = state.metrics.total_requests.max(1);
    let avg_ms = total_dur / total_req;
    let success_rate = 100.0f32 - (state.metrics.total_errors as f32 / total_req as f32 * 100.0);
    let data = format!(
        r#"{{"requests_total":{req},"errors_total":{err},"avg_ms":{avg_ms},"success_rate":{sr:.2},"sample_rate":{sample}}}"#,
        req = state.metrics.total_requests,
        err = state.metrics.total_errors,
        sr = success_rate.clamp(0.0, 100.0),
        sample = state.trace_sample_rate,
    );
    SseFrame::new("metrics", &data).encode()
}

/// Service catalog as SSE frames, one per service plus a summary frame.
pub fn render_services(state: &State) -> String {
    let mut out = String::new();
    if let Some(file) = &state.service_registry {
        for svc in &file.services {
            let auth = match svc.auth {
                compiler::Auth::None => "none",
                compiler::Auth::Bearer => "bearer",
                compiler::Auth::ApiKey => "api-key",
            };
            let data = format!(
                r#"{{"name":{},"endpoints":{},"auth":"{auth}"}}"#,
                json_str(&svc.name),
                svc.endpoints.len()
            );
            out.push_str(
                &SseFrame::new("service", &data)
                    .with_id(svc.name.clone())
                    .encode(),
            );
        }
        let summary = format!(r#"{{"services":{}}}"#, file.services.len());
        out.push_str(&SseFrame::new("services", &summary).encode());
    } else {
        out.push_str(&SseFrame::new("services", r#"{"services":0}"#).encode());
    }
    out
}

/// Minimal JSON string escaping for identifiers we interpolate.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_frame_encoding() {
        let frame = SseFrame::new("test", "hello").with_id("1");
        let encoded = frame.encode();
        assert!(encoded.contains("event: test"));
        assert!(encoded.contains("data: hello"));
        assert!(encoded.contains("id: 1"));
    }

    #[test]
    fn test_trace_event_to_sse() {
        let event = StreamEvent::Trace {
            trace_id: "req-001".into(),
            service: "users".into(),
            method: "GET".into(),
            path: "/users".into(),
            status: 200,
            duration_ms: 45,
        };
        let frame = event.to_sse_frame();
        assert_eq!(frame.event, "trace");
        assert!(frame.data.contains("users"));
        assert!(frame.data.contains("200"));
    }

    #[test]
    fn test_stream_buffer() {
        let buffer = StreamBuffer::new(5);
        for i in 0..10 {
            buffer.push(StreamEvent::Custom {
                event_type: "test".into(),
                payload: format!("event {}", i),
            });
        }
        let recent = buffer.get_recent(5);
        assert_eq!(recent.len(), 5);
    }

    #[test]
    fn test_multiline_data_encoding() {
        let frame = SseFrame::new("multiline", "line1\nline2\nline3");
        let encoded = frame.encode();
        assert!(encoded.contains("data: line1"));
        assert!(encoded.contains("data: line2"));
        assert!(encoded.contains("data: line3"));
    }

    // ── StreamRegistry ─────────────────────────────────────────────────────

    #[test]
    fn registry_catalog_lists_known_endpoints() {
        let r = StreamRegistry::new();
        let json = r.endpoints_json();
        assert!(json.contains("/api/v1/stream/traces"));
        assert!(json.contains("/api/v1/stream/metrics"));
        assert!(json.contains("/api/v1/stream/services"));
        assert!(json.contains(r#""count":3"#));
    }

    #[test]
    fn registry_open_normalizes_known_paths() {
        let r = StreamRegistry::new();
        assert_eq!(
            r.open("/api/v1/stream/traces").as_deref(),
            Some("/api/v1/stream/traces")
        );
        assert_eq!(
            r.open("/api/v1/stream/metrics/").as_deref(),
            Some("/api/v1/stream/metrics")
        );
        assert_eq!(r.open("/api/v1/nope"), None);
    }

    #[test]
    fn registry_sessions_track_lifecycle() {
        let mut r = StreamRegistry::new();
        let ep = r.open("/api/v1/stream/traces").unwrap();
        let s1 = r.set_open(&ep);
        let s2 = r.set_open(&ep);
        assert_eq!(r.active_count(), 2);
        // Catalog reflects per-endpoint counts.
        assert!(r.endpoints_json().contains(r#""active":2"#));
        // Closing an unknown id is a no-op returning false.
        assert!(!r.close("s000000"));
        // Closing known ids works and empties the endpoint.
        assert!(r.close(&s1));
        assert!(r.close(&s2));
        assert_eq!(r.active_count(), 0);
    }

    #[test]
    fn render_metrics_reflects_state() {
        let mut state = crate::state::State::new(None, None);
        state.push_trace("GET", "/a", 200, 10);
        state.push_trace("GET", "/b", 500, 30);
        let frame = render_metrics(&state);
        assert!(frame.contains("event: metrics"));
        assert!(frame.contains(r#""requests_total":2"#));
        assert!(frame.contains(r#""errors_total":1"#));
    }

    #[test]
    fn render_traces_emits_recent_first_capped() {
        let mut state = crate::state::State::new(None, None);
        for i in 0..5 {
            state.push_trace("GET", &format!("/p{i}"), 200, i);
        }
        let out = render_traces(&state, 3);
        // Capped at 3 frames.
        assert_eq!(out.matches("event: trace").count(), 3);
        // Newest last (SSE replay order): p4 must appear after p3.
        let p3 = out.find("/p3").expect("p3 present");
        let p4 = out.find("/p4").expect("p4 present");
        assert!(p3 < p4, "traces must stream oldest-first");
    }

    #[test]
    fn render_services_without_registry_is_empty_catalog() {
        let state = crate::state::State::new(None, None);
        let out = render_services(&state);
        assert!(out.contains(r#"{"services":0}"#));
    }
}
