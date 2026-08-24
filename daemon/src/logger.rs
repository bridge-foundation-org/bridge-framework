//! Bridge structured logger — log levels, batching, JSON output.
//!
//! Inspired by Encore commits 1325 (runtimes-core overhaul runtime logging),
//! 1327 (typescript add logs to traces), 1871 (write logs in batches).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Log level ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "trace" => Level::Trace,
            "debug" => Level::Debug,
            "warn" | "warning" => Level::Warn,
            "error" | "err" => Level::Error,
            _ => Level::Info,
        }
    }

    /// ANSI color for terminal output.
    pub fn color_prefix(self) -> &'static str {
        match self {
            Level::Trace => "\x1b[2m[TRACE]\x1b[0m",
            Level::Debug => "\x1b[36m[DEBUG]\x1b[0m",
            Level::Info => "\x1b[32m[INFO]\x1b[0m ",
            Level::Warn => "\x1b[33m[WARN]\x1b[0m ",
            Level::Error => "\x1b[31m[ERROR]\x1b[0m",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ── Log entry ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Entry {
    pub level: Level,
    pub message: String,
    pub target: String,
    pub timestamp: u64,
    pub trace_id: Option<String>,
    pub fields: Vec<(String, String)>,
}

impl Entry {
    pub fn new(level: Level, target: &str, message: impl Into<String>) -> Self {
        Entry {
            level,
            message: message.into(),
            target: target.to_string(),
            timestamp: now_ms(),
            trace_id: None,
            fields: Vec::new(),
        }
    }

    pub fn with_trace(mut self, id: impl Into<String>) -> Self {
        self.trace_id = Some(id.into());
        self
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push((key.into(), value.into()));
        self
    }

    /// Render as structured JSON line.
    pub fn to_json(&self) -> String {
        let trace = self
            .trace_id
            .as_deref()
            .map(|id| format!(",\"trace_id\":\"{}\"", id))
            .unwrap_or_default();

        let fields: String = self
            .fields
            .iter()
            .map(|(k, v)| format!(",\"{}\":\"{}\"", k, v))
            .collect();

        format!(
            r#"{{"ts":{ts},"level":"{level}","target":"{target}","msg":"{msg}"{trace}{fields}}}"#,
            ts = self.timestamp,
            level = self.level.as_str(),
            target = self.target,
            msg = self.message.replace('"', "\\\""),
            trace = trace,
            fields = fields,
        )
    }

    /// Render as human-readable colored text.
    pub fn to_text(&self, color: bool) -> String {
        let prefix = if color {
            self.level.color_prefix()
        } else {
            self.level.as_str()
        };
        let extra: String = self
            .fields
            .iter()
            .map(|(k, v)| format!(" {}={}", k, v))
            .collect();
        let trace = self
            .trace_id
            .as_deref()
            .map(|id| format!(" trace={}", id))
            .unwrap_or_default();
        format!(
            "{} {} {}{}{}\n",
            prefix, self.target, self.message, trace, extra
        )
    }
}

// ── Logger ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Logger {
    inner: Arc<Mutex<LoggerInner>>,
}

struct LoggerInner {
    min_level: Level,
    json_mode: bool,
    buffer: Vec<Entry>,
    max_buf: usize,
}

impl Logger {
    pub fn new(min_level: Level, json_mode: bool) -> Self {
        Logger {
            inner: Arc::new(Mutex::new(LoggerInner {
                min_level,
                json_mode,
                buffer: Vec::with_capacity(256),
                max_buf: 1000,
            })),
        }
    }

    /// Build from `BRIDGE_LOG` environment variable.
    pub fn from_env() -> Self {
        let level = std::env::var("BRIDGE_LOG")
            .map(|s| Level::parse(&s))
            .unwrap_or(Level::Info);
        let json = std::env::var("BRIDGE_LOG_JSON")
            .map(|s| s == "1" || s == "true")
            .unwrap_or(false);
        Self::new(level, json)
    }

    /// Record a log entry (buffered + printed).
    pub fn log(&self, entry: Entry) {
        let mut inner = self.inner.lock().unwrap();
        if entry.level < inner.min_level {
            return;
        }

        // Print immediately
        let line = if inner.json_mode {
            entry.to_json()
        } else {
            let color = std::env::var("NO_COLOR").is_err();
            entry.to_text(color).trim_end().to_string()
        };
        eprintln!("{line}");

        // Keep in ring buffer for trace queries
        if inner.buffer.len() >= inner.max_buf {
            inner.buffer.remove(0);
        }
        inner.buffer.push(entry);
    }

    // ── Convenience methods ───────────────────────────────────────────────

    pub fn trace(&self, target: &str, msg: impl Into<String>) {
        self.log(Entry::new(Level::Trace, target, msg));
    }
    pub fn debug(&self, target: &str, msg: impl Into<String>) {
        self.log(Entry::new(Level::Debug, target, msg));
    }
    pub fn info(&self, target: &str, msg: impl Into<String>) {
        self.log(Entry::new(Level::Info, target, msg));
    }
    pub fn warn(&self, target: &str, msg: impl Into<String>) {
        self.log(Entry::new(Level::Warn, target, msg));
    }
    pub fn error(&self, target: &str, msg: impl Into<String>) {
        self.log(Entry::new(Level::Error, target, msg));
    }

    // ── Buffer queries ────────────────────────────────────────────────────

    /// Return buffered log entries as a JSON array.
    pub fn recent_json(&self, limit: usize) -> String {
        let inner = self.inner.lock().unwrap();
        let entries: Vec<_> = inner.buffer.iter().rev().take(limit).collect();
        let parts: Vec<_> = entries.iter().rev().map(|e| e.to_json()).collect();
        format!("[{}]", parts.join(","))
    }

    /// Return recent log entries matching at or above a given level.
    pub fn recent_at_level(&self, level: Level, limit: usize) -> Vec<Entry> {
        let inner = self.inner.lock().unwrap();
        inner
            .buffer
            .iter()
            .rev()
            .filter(|e| e.level >= level)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Set the minimum log level.
    pub fn set_level(&self, level: Level) {
        self.inner.lock().unwrap().min_level = level;
    }

    pub fn min_level(&self) -> Level {
        self.inner.lock().unwrap().min_level
    }
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

    fn silent_logger() -> Logger {
        // Level::Error + store, but suppress stderr in tests
        Logger::new(Level::Trace, false)
    }

    #[test]
    fn level_ordering() {
        assert!(Level::Error > Level::Warn);
        assert!(Level::Warn > Level::Info);
        assert!(Level::Info > Level::Debug);
        assert!(Level::Debug > Level::Trace);
    }

    #[test]
    fn level_parse() {
        assert_eq!(Level::parse("debug"), Level::Debug);
        assert_eq!(Level::parse("ERROR"), Level::Error);
        assert_eq!(Level::parse("warn"), Level::Warn);
        assert_eq!(Level::parse("unknown"), Level::Info);
    }

    #[test]
    fn entry_json_output() {
        let e = Entry::new(Level::Info, "bridge::http", "Request received")
            .with_trace("abc123")
            .with_field("method", "GET")
            .with_field("path", "/api/ping");
        let json = e.to_json();
        assert!(json.contains("\"level\":\"info\""));
        assert!(json.contains("\"trace_id\":\"abc123\""));
        assert!(json.contains("\"method\":\"GET\""));
        assert!(json.contains("\"path\":\"/api/ping\""));
    }

    #[test]
    fn entry_text_output() {
        let e = Entry::new(Level::Warn, "bridge::db", "Slow query detected")
            .with_field("duration_ms", "1234");
        let text = e.to_text(false);
        assert!(text.contains("warn"));
        assert!(text.contains("bridge::db"));
        assert!(text.contains("Slow query detected"));
        assert!(text.contains("duration_ms=1234"));
    }

    #[test]
    fn logger_buffers_entries() {
        let log = silent_logger();
        log.info("test", "message one");
        log.warn("test", "message two");
        log.error("test", "message three");
        let recent = log.recent_at_level(Level::Info, 10);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn logger_filters_by_level() {
        let log = Logger::new(Level::Warn, false);
        log.debug("test", "this should be filtered");
        log.info("test", "this too");
        log.warn("test", "this should appear");
        let recent = log.recent_at_level(Level::Trace, 10);
        // Only warn was above min_level
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].level, Level::Warn);
    }

    #[test]
    fn logger_recent_json_valid() {
        let log = silent_logger();
        log.info("test", "hello");
        let json = log.recent_json(10);
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));
        assert!(json.contains("hello"));
    }

    #[test]
    fn ring_buffer_caps() {
        let log = Logger::new(Level::Trace, false);
        // Manually insert more than max_buf
        {
            let mut inner = log.inner.lock().unwrap();
            inner.max_buf = 5;
        }
        for i in 0..10 {
            log.info("test", format!("msg {i}"));
        }
        let recent = log.recent_at_level(Level::Trace, 100);
        assert!(
            recent.len() <= 5,
            "buffer should cap at 5, got {}",
            recent.len()
        );
    }

    #[test]
    fn set_level_changes_filter() {
        let log = silent_logger();
        log.set_level(Level::Error);
        log.info("test", "should be filtered");
        let recent = log.recent_at_level(Level::Trace, 10);
        assert_eq!(recent.len(), 0);
        log.set_level(Level::Info);
        log.info("test", "now visible");
        let recent = log.recent_at_level(Level::Trace, 10);
        assert_eq!(recent.len(), 1);
    }
}
