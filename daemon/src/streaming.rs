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

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

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
            output.push_str("\n");
        }

        output.push_str("event: ");
        output.push_str(&self.event);
        output.push_str("\n");

        for line in self.data.lines() {
            output.push_str("data: ");
            output.push_str(line);
            output.push_str("\n");
        }

        output.push_str("\n");
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

pub struct StreamRegistry;

impl StreamRegistry {
    pub fn new() -> Self {
        StreamRegistry
    }

    pub fn endpoints_json(&self) -> String {
        r#"{"endpoints":[],"count":0}"#.into()
    }

    pub fn active_count(&self) -> u32 {
        0
    }

    pub fn open(&self, _path: &str) -> Option<String> {
        None // No streaming endpoints implemented yet
    }

    pub fn set_open(&mut self, _id: &str) {
        // TODO: Implement
    }

    pub fn close(&mut self, _id: &str) {
        // TODO: Implement
    }
}

impl Default for StreamRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Streaming Endpoint Handlers ────────────────────────────────────────────

pub fn stream_traces_sse(count: usize) -> String {
    // This would be called from http.rs
    // Stream recent traces as SSE events
    let mut output = String::from("data: Connected to trace stream\n\n");
    // Add more events as needed
    output
}

pub fn stream_metrics_sse() -> String {
    // Stream metrics updates
    let frame = SseFrame::new("metrics", r#"{"status":"ok"}"#);
    frame.encode()
}

pub fn stream_services_sse(count: usize) -> String {
    // Stream service updates
    let frame = SseFrame::new("services", r#"{"status":"ok","count":0}"#);
    frame.encode()
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
}
