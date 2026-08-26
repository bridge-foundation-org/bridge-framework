//! Distributed Tracing - OpenTelemetry-compatible tracing
//!
//! Trace requests across services with spans and context propagation

// Parts of this module are forward-scaffolding: their public API is
// intentionally ahead of its call sites. Trim this allow item-by-item as the
// dead surface shrinks.
#![allow(dead_code)]

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Trace level/severity
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TraceLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl TraceLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            TraceLevel::Debug => "DEBUG",
            TraceLevel::Info => "INFO",
            TraceLevel::Warn => "WARN",
            TraceLevel::Error => "ERROR",
        }
    }
}

/// Span attributes
pub type SpanAttributes = HashMap<String, String>;

/// Span event
#[derive(Clone, Debug)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: u64,
    pub attributes: SpanAttributes,
}

impl SpanEvent {
    pub fn new(name: impl Into<String>) -> Self {
        SpanEvent {
            name: name.into(),
            timestamp: current_timestamp_ms(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// Span status
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanStatus {
    Unset,
    Ok,
    Error,
}

/// Distributed trace span
#[derive(Clone, Debug)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_time: u64,
    pub end_time: Option<u64>,
    pub status: SpanStatus,
    pub attributes: SpanAttributes,
    pub events: Vec<SpanEvent>,
}

impl Span {
    /// Create a new span
    pub fn new(
        trace_id: impl Into<String>,
        span_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Span {
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            parent_span_id: None,
            name: name.into(),
            start_time: current_timestamp_ms(),
            end_time: None,
            status: SpanStatus::Unset,
            attributes: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// Set parent span
    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_span_id = Some(parent_id.into());
        self
    }

    /// Add attribute
    pub fn add_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Add event
    pub fn add_event(mut self, event: SpanEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Set status
    pub fn set_status(mut self, status: SpanStatus) -> Self {
        self.status = status;
        self
    }

    /// End the span
    pub fn end(mut self) -> Self {
        self.end_time = Some(current_timestamp_ms());
        self
    }

    /// Get duration in milliseconds
    pub fn duration_ms(&self) -> Option<u64> {
        self.end_time.map(|end| end - self.start_time)
    }
}

/// Trace context (W3C Trace Context compatible)
#[derive(Clone, Debug)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub trace_flags: u8,
}

impl TraceContext {
    /// Create new trace context
    pub fn new(trace_id: impl Into<String>, span_id: impl Into<String>) -> Self {
        TraceContext {
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            trace_flags: 0x01, // sampled
        }
    }

    /// Parse W3C traceparent header
    pub fn from_traceparent(header: &str) -> Result<Self, String> {
        // Format: 00-trace_id-span_id-trace_flags
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return Err("Invalid traceparent format".to_string());
        }

        if parts[0] != "00" {
            return Err("Invalid version".to_string());
        }

        let flags = u8::from_str_radix(parts[3], 16).map_err(|_| "Invalid flags")?;

        Ok(TraceContext {
            trace_id: parts[1].to_string(),
            span_id: parts[2].to_string(),
            trace_flags: flags,
        })
    }

    /// Convert to W3C traceparent header
    pub fn to_traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            self.trace_id, self.span_id, self.trace_flags
        )
    }

    /// Check if trace is sampled
    pub fn is_sampled(&self) -> bool {
        self.trace_flags & 0x01 == 0x01
    }
}

/// In-memory trace collector
pub struct TraceCollector {
    spans: HashMap<String, Vec<Span>>,
    max_spans_per_trace: usize,
}

impl TraceCollector {
    pub fn new() -> Self {
        TraceCollector {
            spans: HashMap::new(),
            max_spans_per_trace: 1000,
        }
    }

    /// Record a span
    pub fn record_span(&mut self, span: Span) {
        let trace_id = span.trace_id.clone();
        let spans = self.spans.entry(trace_id).or_default();

        if spans.len() < self.max_spans_per_trace {
            spans.push(span);
        }
    }

    /// Get spans for a trace
    pub fn get_trace(&self, trace_id: &str) -> Option<Vec<&Span>> {
        self.spans.get(trace_id).map(|spans| spans.iter().collect())
    }

    /// List all traces
    pub fn list_traces(&self) -> Vec<&str> {
        self.spans.keys().map(|s| s.as_str()).collect()
    }

    /// Clear old traces (keep last N)
    pub fn cleanup(&mut self, keep_count: usize) {
        if self.spans.len() > keep_count {
            let mut traces: Vec<_> = self.spans.keys().cloned().collect();
            traces.sort();

            while self.spans.len() > keep_count {
                if let Some(oldest) = traces.first() {
                    self.spans.remove(oldest);
                    traces.remove(0);
                }
            }
        }
    }
}

impl Default for TraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current timestamp in milliseconds since epoch
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Generate a random trace ID (simple version)
pub fn generate_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:032x}", nanos)
}

/// Generate a random span ID
pub fn generate_span_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:016x}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_level_ordering() {
        assert!(TraceLevel::Debug < TraceLevel::Info);
        assert!(TraceLevel::Info < TraceLevel::Warn);
        assert!(TraceLevel::Warn < TraceLevel::Error);
    }

    #[test]
    fn test_trace_level_as_str() {
        assert_eq!(TraceLevel::Debug.as_str(), "DEBUG");
        assert_eq!(TraceLevel::Info.as_str(), "INFO");
        assert_eq!(TraceLevel::Error.as_str(), "ERROR");
    }

    #[test]
    fn test_span_event_new() {
        let event = SpanEvent::new("user_created");
        assert_eq!(event.name, "user_created");
        assert!(event.timestamp > 0);
    }

    #[test]
    fn test_span_event_with_attribute() {
        let event = SpanEvent::new("user_created").with_attribute("user_id", "123");
        assert_eq!(event.attributes.get("user_id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_span_new() {
        let span = Span::new("trace123", "span456", "get_user");
        assert_eq!(span.trace_id, "trace123");
        assert_eq!(span.span_id, "span456");
        assert_eq!(span.name, "get_user");
        assert!(span.start_time > 0);
        assert!(span.end_time.is_none());
    }

    #[test]
    fn test_span_with_parent() {
        let span = Span::new("trace123", "span456", "test").with_parent("parent789");
        assert_eq!(span.parent_span_id, Some("parent789".to_string()));
    }

    #[test]
    fn test_span_add_attribute() {
        let span = Span::new("trace123", "span456", "test")
            .add_attribute("http.method", "GET")
            .add_attribute("http.status_code", "200");

        assert_eq!(span.attributes.len(), 2);
        assert_eq!(span.attributes.get("http.method"), Some(&"GET".to_string()));
    }

    #[test]
    fn test_span_add_event() {
        let event = SpanEvent::new("checkpoint");
        let span = Span::new("trace123", "span456", "test").add_event(event);
        assert_eq!(span.events.len(), 1);
    }

    #[test]
    fn test_span_set_status() {
        let span = Span::new("trace123", "span456", "test").set_status(SpanStatus::Ok);
        assert_eq!(span.status, SpanStatus::Ok);
    }

    #[test]
    fn test_span_end() {
        let span = Span::new("trace123", "span456", "test").end();
        assert!(span.end_time.is_some());
    }

    #[test]
    fn test_span_duration_ms() {
        let mut span = Span::new("trace123", "span456", "test");
        assert!(span.duration_ms().is_none());

        span = span.end();
        let duration = span.duration_ms();
        assert!(duration.is_some());
    }

    #[test]
    fn test_trace_context_new() {
        let ctx = TraceContext::new("trace123", "span456");
        assert_eq!(ctx.trace_id, "trace123");
        assert_eq!(ctx.span_id, "span456");
        assert!(ctx.is_sampled());
    }

    #[test]
    fn test_trace_context_to_traceparent() {
        let ctx = TraceContext::new("trace123", "span456");
        let header = ctx.to_traceparent();
        assert!(header.contains("trace123"));
        assert!(header.contains("span456"));
        assert!(header.contains("01"));
    }

    #[test]
    fn test_trace_context_from_traceparent() {
        let header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = TraceContext::from_traceparent(header).unwrap();
        assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ctx.span_id, "00f067aa0ba902b7");
        assert!(ctx.is_sampled());
    }

    #[test]
    fn test_trace_context_roundtrip() {
        let ctx1 = TraceContext::new("abcd1234", "efgh5678");
        let header = ctx1.to_traceparent();
        let ctx2 = TraceContext::from_traceparent(&header).unwrap();
        assert_eq!(ctx1.trace_id, ctx2.trace_id);
        assert_eq!(ctx1.span_id, ctx2.span_id);
    }

    #[test]
    fn test_trace_collector_new() {
        let collector = TraceCollector::new();
        assert_eq!(collector.spans.len(), 0);
    }

    #[test]
    fn test_trace_collector_record_span() {
        let mut collector = TraceCollector::new();
        let span = Span::new("trace123", "span456", "test");
        collector.record_span(span);
        assert_eq!(collector.spans.len(), 1);
    }

    #[test]
    fn test_trace_collector_get_trace() {
        let mut collector = TraceCollector::new();
        let span = Span::new("trace123", "span456", "test");
        collector.record_span(span);

        let traces = collector.get_trace("trace123");
        assert!(traces.is_some());
        assert_eq!(traces.unwrap().len(), 1);
    }

    #[test]
    fn test_trace_collector_list_traces() {
        let mut collector = TraceCollector::new();
        collector.record_span(Span::new("trace1", "span1", "op1"));
        collector.record_span(Span::new("trace2", "span1", "op2"));

        let traces = collector.list_traces();
        assert_eq!(traces.len(), 2);
    }

    #[test]
    fn test_trace_collector_cleanup() {
        let mut collector = TraceCollector::new();
        for i in 0..10 {
            let trace_id = format!("trace{}", i);
            collector.record_span(Span::new(&trace_id, "span", "op"));
        }

        assert_eq!(collector.spans.len(), 10);
        collector.cleanup(5);
        assert_eq!(collector.spans.len(), 5);
    }

    #[test]
    fn test_generate_trace_id() {
        let id1 = generate_trace_id();
        let id2 = generate_trace_id();
        assert!(!id1.is_empty());
        assert!(!id2.is_empty());
    }

    #[test]
    fn test_generate_span_id() {
        let id1 = generate_span_id();
        let id2 = generate_span_id();
        assert!(!id1.is_empty());
        assert!(!id2.is_empty());
    }
}
