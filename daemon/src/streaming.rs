//! Bridge streaming — Server-Sent Events, WebSocket endpoints, stream registry.
//!
//! Inspired by Encore commit 1428 (TS Add support for streaming apis),
//! 1434-1445 (WebSocket documentation/impl),
//! 1462-1464 (stream fixes), 1565 (stream service-to-service),
//! 1652 (public stream types), 1723 (stream handshake fix).
//!
//! Zero external dependencies — pure std.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Stream direction ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDirection {
    /// Server sends events; client reads only.
    ServerToClient,
    /// Client sends messages; server reads only.
    ClientToServer,
    /// Full duplex — both sides send and receive.
    Bidirectional,
}

impl StreamDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            StreamDirection::ServerToClient => "server_to_client",
            StreamDirection::ClientToServer => "client_to_server",
            StreamDirection::Bidirectional  => "bidirectional",
        }
    }
}

// ── Stream endpoint definition ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StreamEndpoint {
    pub name:        String,
    pub path:        String,
    pub direction:   StreamDirection,
    pub tags:        Vec<String>,
    pub description: Option<String>,
    pub exposed:     bool,
    pub auth:        bool,
}

impl StreamEndpoint {
    pub fn new(name: &str, path: &str, direction: StreamDirection) -> Self {
        StreamEndpoint {
            name:        name.to_string(),
            path:        path.to_string(),
            direction,
            tags:        Vec::new(),
            description: None,
            exposed:     true,
            auth:        false,
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn require_auth(mut self) -> Self {
        self.auth = true;
        self
    }

    pub fn private(mut self) -> Self {
        self.exposed = false;
        self
    }

    pub fn to_json(&self) -> String {
        let tags = self.tags.iter()
            .map(|t| format!("\"{}\"", t))
            .collect::<Vec<_>>().join(",");
        let desc = self.description.as_deref()
            .map(|d| format!(",\"description\":\"{}\"", d))
            .unwrap_or_default();
        format!(
            r#"{{"name":"{name}","path":"{path}","direction":"{dir}","tags":[{tags}],"exposed":{exp},"auth":{auth}{desc}}}"#,
            name = self.name,
            path = self.path,
            dir  = self.direction.as_str(),
            tags = tags,
            exp  = self.exposed,
            auth = self.auth,
            desc = desc,
        )
    }
}

// ── Active stream ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamStatus {
    Handshaking,
    Open,
    Closing,
    Closed,
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub id:         String,
    pub endpoint:   String,
    pub direction:  StreamDirection,
    pub status:     StreamStatus,
    pub opened_at:  u64,
    pub message_count: u64,
}

impl StreamInfo {
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"id":"{id}","endpoint":"{ep}","direction":"{dir}","status":"{status}","opened_at":{ts},"messages":{msgs}}}"#,
            id     = self.id,
            ep     = self.endpoint,
            dir    = self.direction.as_str(),
            status = match self.status {
                StreamStatus::Handshaking => "handshaking",
                StreamStatus::Open        => "open",
                StreamStatus::Closing     => "closing",
                StreamStatus::Closed      => "closed",
            },
            ts   = self.opened_at,
            msgs = self.message_count,
        )
    }
}

// ── Stream registry ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct StreamRegistry(Arc<Mutex<RegistryInner>>);

struct RegistryInner {
    endpoints: HashMap<String, StreamEndpoint>,
    active:    HashMap<String, StreamInfo>,
    counter:   u64,
}

impl StreamRegistry {
    pub fn new() -> Self {
        StreamRegistry(Arc::new(Mutex::new(RegistryInner {
            endpoints: HashMap::new(),
            active:    HashMap::new(),
            counter:   0,
        })))
    }

    /// Register a stream endpoint definition.
    pub fn register(&self, ep: StreamEndpoint) {
        self.0.lock().unwrap().endpoints.insert(ep.path.clone(), ep);
    }

    /// Open a new active stream. Returns stream ID.
    pub fn open(&self, endpoint_path: &str) -> Option<String> {
        let mut inner = self.0.lock().unwrap();
        // Copy direction before taking any mutable borrow
        let direction = inner.endpoints.get(endpoint_path)?.direction;
        inner.counter += 1;
        let id = format!("stream-{}", inner.counter);
        inner.active.insert(id.clone(), StreamInfo {
            id:            id.clone(),
            endpoint:      endpoint_path.to_string(),
            direction,
            status:        StreamStatus::Handshaking,
            opened_at:     now_ms(),
            message_count: 0,
        });
        Some(id)
    }

    /// Transition a stream to Open.
    pub fn set_open(&self, stream_id: &str) {
        if let Some(s) = self.0.lock().unwrap().active.get_mut(stream_id) {
            s.status = StreamStatus::Open;
        }
    }

    /// Record a message on an active stream.
    pub fn record_message(&self, stream_id: &str) {
        if let Some(s) = self.0.lock().unwrap().active.get_mut(stream_id) {
            s.message_count += 1;
        }
    }

    /// Close a stream.
    pub fn close(&self, stream_id: &str) {
        let mut inner = self.0.lock().unwrap();
        if let Some(s) = inner.active.get_mut(stream_id) {
            s.status = StreamStatus::Closed;
        }
        inner.active.remove(stream_id);
    }

    /// List all registered endpoint definitions as JSON.
    pub fn endpoints_json(&self) -> String {
        let inner = self.0.lock().unwrap();
        let parts: Vec<_> = inner.endpoints.values().map(|e| e.to_json()).collect();
        format!("[{}]", parts.join(","))
    }

    /// List all active stream infos as JSON.
    pub fn active_json(&self) -> String {
        let inner = self.0.lock().unwrap();
        let parts: Vec<_> = inner.active.values().map(|s| s.to_json()).collect();
        format!("[{}]", parts.join(","))
    }

    /// Count of active open streams.
    pub fn active_count(&self) -> usize {
        let inner = self.0.lock().unwrap();
        inner.active.values()
            .filter(|s| s.status == StreamStatus::Open)
            .count()
    }
}

impl Default for StreamRegistry {
    fn default() -> Self { Self::new() }
}

// ── SSE helpers ───────────────────────────────────────────────────────────────

/// Write a Server-Sent Event to a writer.
/// Format: `data: <payload>\n\n`
pub fn write_sse_event(w: &mut impl Write, event: Option<&str>, data: &str) -> std::io::Result<()> {
    if let Some(name) = event {
        write!(w, "event: {name}\n")?;
    }
    for line in data.lines() {
        write!(w, "data: {line}\n")?;
    }
    write!(w, "\n")?;
    w.flush()
}

/// SSE response headers.
pub fn sse_headers() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Content-Type", "text/event-stream"),
        ("Cache-Control", "no-cache"),
        ("Connection", "keep-alive"),
        ("X-Accel-Buffering", "no"),
    ]
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> StreamRegistry {
        let reg = StreamRegistry::new();
        reg.register(StreamEndpoint::new(
            "chat",
            "/api/chat/stream",
            StreamDirection::Bidirectional,
        ).with_tag("realtime").with_description("Chat stream"));
        reg.register(StreamEndpoint::new(
            "events",
            "/api/events",
            StreamDirection::ServerToClient,
        ).require_auth());
        reg
    }

    #[test]
    fn register_and_list_endpoints() {
        let reg = make_registry();
        let json = reg.endpoints_json();
        assert!(json.contains("\"name\":\"chat\""));
        assert!(json.contains("\"direction\":\"bidirectional\""));
        assert!(json.contains("\"name\":\"events\""));
    }

    #[test]
    fn open_and_transition() {
        let reg = make_registry();
        let id = reg.open("/api/chat/stream").expect("should open");
        reg.set_open(&id);
        assert_eq!(reg.active_count(), 1);
    }

    #[test]
    fn message_count() {
        let reg = make_registry();
        let id = reg.open("/api/chat/stream").unwrap();
        reg.set_open(&id);
        reg.record_message(&id);
        reg.record_message(&id);
        let json = reg.active_json();
        assert!(json.contains("\"messages\":2"));
    }

    #[test]
    fn close_removes_stream() {
        let reg = make_registry();
        let id = reg.open("/api/chat/stream").unwrap();
        reg.set_open(&id);
        assert_eq!(reg.active_count(), 1);
        reg.close(&id);
        assert_eq!(reg.active_count(), 0);
    }

    #[test]
    fn open_unknown_endpoint_returns_none() {
        let reg = StreamRegistry::new();
        assert!(reg.open("/nonexistent").is_none());
    }

    #[test]
    fn sse_event_format() {
        let mut buf = Vec::new();
        write_sse_event(&mut buf, Some("update"), "hello\nworld").unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("event: update\n"));
        assert!(s.contains("data: hello\n"));
        assert!(s.contains("data: world\n"));
        assert!(s.ends_with("\n\n"));
    }

    #[test]
    fn sse_event_no_name() {
        let mut buf = Vec::new();
        write_sse_event(&mut buf, None, "ping").unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(!s.contains("event:"));
        assert!(s.contains("data: ping\n"));
    }

    #[test]
    fn endpoint_json_includes_auth() {
        let ep = StreamEndpoint::new("secure", "/ws/secure", StreamDirection::Bidirectional)
            .require_auth();
        let json = ep.to_json();
        assert!(json.contains("\"auth\":true"));
    }

    #[test]
    fn private_endpoint_not_exposed() {
        let ep = StreamEndpoint::new("internal", "/ws/internal", StreamDirection::ServerToClient)
            .private();
        let json = ep.to_json();
        assert!(json.contains("\"exposed\":false"));
    }
}
