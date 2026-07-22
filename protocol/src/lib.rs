//! Bridge wire protocol — shared between CLI and daemon.
//!
//! # Design
//!
//! Commands are line-oriented and URL-percent-encoded so that any payload
//! (source code, SQL, tokens) can safely be sent over a single TCP line.
//!
//! This protocol supports:
//! - **Traces**: Complete request traces with spans, metrics, logs
//! - **Auth**: Bearer token and API key authentication schemes  
//! - **Metrics**: Counter, gauge, histogram instrumentation
//! - **Streaming**: Real-time endpoint streaming support (future)
//!
//! # Response format
//!
//! | Prefix      | Meaning                              | Example                        |
//! |-------------|--------------------------------------|--------------------------------|
//! | `PONG`      | Ping reply                           | `PONG`                         |
//! | `OK …`      | Successful command, optional note    | `OK compiled successfully`     |
//! | `DATA …`    | Payload (percent-encoded)            | `DATA trace_id%3D123`          |
//! | `ERR …`     | Error message                        | `ERR connection refused`       |
//! | `MODE …`    | Current mode value                   | `MODE full`                    |
//! | `TRACE …`   | Trace data (JSON)                    | `TRACE {...}`                  |
//! | `METRIC …`  | Metric data point                    | `METRIC counter=http_requests` |

use std::collections::HashMap;
use std::fmt;

/// Bridge framework version
pub const VERSION: &str = "0.2.0";

// ══════════════════════════════════════════════════════════════════════════════
// Core types
// ══════════════════════════════════════════════════════════════════════════════

/// Daemon operational mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonMode {
    /// Minimal functionality — only essential services
    Lite,
    /// Full stack — all services including Docker, Redis
    Full,
    /// Enhanced mode — with metrics, tracing instrumentation
    Ultra,
    /// Daemon stopped
    Off,
}

impl DaemonMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DaemonMode::Lite => "lite",
            DaemonMode::Full => "full",
            DaemonMode::Ultra => "ultra",
            DaemonMode::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "lite" => Ok(DaemonMode::Lite),
            "full" => Ok(DaemonMode::Full),
            "ultra" => Ok(DaemonMode::Ultra),
            "off" => Ok(DaemonMode::Off),
            other => Err(format!("unknown mode: {other} (use: lite|full|ultra|off)")),
        }
    }
}

impl fmt::Display for DaemonMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Authentication scheme
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthScheme {
    Bearer,
    ApiKey,
}

impl AuthScheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthScheme::Bearer => "bearer",
            AuthScheme::ApiKey => "api_key",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "bearer" => Ok(AuthScheme::Bearer),
            "api_key" | "apikey" => Ok(AuthScheme::ApiKey),
            other => Err(format!("unknown auth scheme: {other} (use: bearer|api_key)")),
        }
    }
}

/// Export format for traces and metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Csv,
    Text,
}

impl ExportFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
            ExportFormat::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "json" => Ok(ExportFormat::Json),
            "csv" => Ok(ExportFormat::Csv),
            "text" | "txt" => Ok(ExportFormat::Text),
            other => Err(format!("unknown format: {other} (use: json|csv|text)")),
        }
    }
}

/// Filter for trace queries
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceFilter {
    pub service: Option<String>,
    pub endpoint: Option<String>,
    pub min_duration_ms: Option<u64>,
    pub status_code: Option<u16>,
}

// ══════════════════════════════════════════════════════════════════════════════
// Commands
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    // ── Core ─────────────────────────────────────────────────────────────────
    Ping,
    Help,
    Stop,
    Version,
    Health,

    // ── Mode ─────────────────────────────────────────────────────────────────
    GetMode,
    SetMode(DaemonMode),

    // ── Compiler pipeline ────────────────────────────────────────────────────
    Compile { source: String },
    CompileFile { path: String },

    // ── Service / route introspection ────────────────────────────────────────
    ServicesList,
    RoutesList,

    // ── In-memory KV store ───────────────────────────────────────────────────
    DbPut { ns: String, key: String, value: String },
    DbGet { ns: String, key: String },
    DbDel { ns: String, key: String },
    DbKeys { ns: String },
    DbFlush { ns: String },

    // ── Docker Postgres ──────────────────────────────────────────────────────
    PgCreate { name: String },
    PgStatus,
    PgMigrate { sql: String },
    PgDestroy { name: String },

    // ── Embedded Redis (miniredis) ───────────────────────────────────────────
    RedisStatus,
    RedisPing,
    RedisGet { key: String },
    RedisSet { key: String, value: String },
    RedisSetEx { key: String, seconds: u64, value: String },
    RedisDel { key: String },
    RedisKeys { pattern: String },
    RedisTtl { key: String },
    RedisExpire { key: String, seconds: u64 },
    RedisFlush,

    // ── Auth ─────────────────────────────────────────────────────────────────
    AuthStatus,
    AuthSet { scheme: AuthScheme, token: String },
    AuthClear,

    // ── Traces ───────────────────────────────────────────────────────────────
    TraceList { limit: Option<usize>, filter: Option<TraceFilter> },
    TraceGet { id: String },
    TraceClear,
    TraceExport { format: ExportFormat },

    // ── Metrics ──────────────────────────────────────────────────────────────
    MetricsList,
    MetricsGet { name: String },
    MetricsClear,
    MetricsExport { format: ExportFormat },
}

// ══════════════════════════════════════════════════════════════════════════════
// Responses
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Pong,
    Mode(DaemonMode),
    Ok(String),
    Data(String),
    Err(String),
    TraceData(Trace),
    MetricData(Metric),
    List(Vec<String>),
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Response::Pong => write!(f, "PONG"),
            Response::Mode(mode) => write!(f, "MODE {}", mode.as_str()),
            Response::Ok(msg) => write!(f, "OK {msg}"),
            Response::Data(data) => write!(f, "DATA {}", percent_encode(data)),
            Response::Err(msg) => write!(f, "ERR {msg}"),
            Response::TraceData(trace) => write!(f, "TRACE {}", trace.to_json()),
            Response::MetricData(metric) => write!(f, "METRIC {metric}"),
            Response::List(items) => {
                write!(f, "LIST")?;
                for item in items {
                    write!(f, " {}", percent_encode(item))?;
                }
                Ok(())
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Traces
// ══════════════════════════════════════════════════════════════════════════════

/// A complete request trace
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub id: String,
    pub start_time: u64,  // Unix timestamp in ms
    pub duration_ms: u64,
    pub service: String,
    pub endpoint: String,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub spans: Vec<Span>,
    pub logs: Vec<LogEntry>,
}

impl Trace {
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"id":"{}","start_time":{},"duration_ms":{},"service":"{}","endpoint":"{}","method":"{}","path":"{}","status_code":{},"spans":[],"logs":[]}}"#,
            self.id, self.start_time, self.duration_ms, self.service,
            self.endpoint, self.method, self.path, self.status_code
        )
    }
}

/// A trace span representing a single operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub id: String,
    pub parent_id: Option<String>,
    pub operation: String,
    pub start_offset_ms: u64,
    pub duration_ms: u64,
    pub tags: HashMap<String, String>,
}

/// A log entry within a trace
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: u64,  // Unix timestamp in ms
    pub level: LogLevel,
    pub message: String,
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Metrics
// ══════════════════════════════════════════════════════════════════════════════

/// A metric data point
#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    pub name: String,
    pub kind: MetricKind,
    pub value: f64,
    pub timestamp: u64,
    pub labels: HashMap<String, String>,
}

impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={} kind={:?} value={}", self.name, self.kind.as_str(), self.kind, self.value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Counter,
    Gauge,
    Histogram,
}

impl MetricKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricKind::Counter => "counter",
            MetricKind::Gauge => "gauge",
            MetricKind::Histogram => "histogram",
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Protocol parsing
// ══════════════════════════════════════════════════════════════════════════════

/// Parse a command from a line of text
pub fn parse_command(line: &str) -> Result<Command, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err("empty command".to_string());
    }

    let mut parts = trimmed.split_whitespace();
    let cmd = parts.next().ok_or("missing command")?;

    match cmd.to_uppercase().as_str() {
        "PING" => Ok(Command::Ping),
        "HELP" => Ok(Command::Help),
        "STOP" => Ok(Command::Stop),
        "VERSION" => Ok(Command::Version),
        "HEALTH" => Ok(Command::Health),

        "MODE-GET" | "GET-MODE" => Ok(Command::GetMode),
        "MODE-SET" | "SET-MODE" => {
            let mode_str = parts.next().ok_or("MODE-SET requires mode argument")?;
            let mode = DaemonMode::parse(mode_str)?;
            Ok(Command::SetMode(mode))
        }

        "COMPILE" => {
            let source = parts.collect::<Vec<_>>().join(" ");
            let decoded = percent_decode(&source)?;
            Ok(Command::Compile { source: decoded })
        }

        "COMPILE-FILE" => {
            let path = parts.next().ok_or("COMPILE-FILE requires path")?.to_string();
            Ok(Command::CompileFile { path })
        }

        "SERVICES" => Ok(Command::ServicesList),
        "ROUTES" => Ok(Command::RoutesList),

        "DB-PUT" => {
            let ns = parts.next().ok_or("DB-PUT requires namespace")?.to_string();
            let key = parts.next().ok_or("DB-PUT requires key")?.to_string();
            let value = parts.collect::<Vec<_>>().join(" ");
            let decoded = percent_decode(&value)?;
            Ok(Command::DbPut { ns, key, value: decoded })
        }

        "DB-GET" => {
            let ns = parts.next().ok_or("DB-GET requires namespace")?.to_string();
            let key = parts.next().ok_or("DB-GET requires key")?.to_string();
            Ok(Command::DbGet { ns, key })
        }

        "DB-DEL" => {
            let ns = parts.next().ok_or("DB-DEL requires namespace")?.to_string();
            let key = parts.next().ok_or("DB-DEL requires key")?.to_string();
            Ok(Command::DbDel { ns, key })
        }

        "DB-KEYS" => {
            let ns = parts.next().ok_or("DB-KEYS requires namespace")?.to_string();
            Ok(Command::DbKeys { ns })
        }

        "DB-FLUSH" => {
            let ns = parts.next().ok_or("DB-FLUSH requires namespace")?.to_string();
            Ok(Command::DbFlush { ns })
        }

        "PG-CREATE" => {
            let name = parts.next().ok_or("PG-CREATE requires name")?.to_string();
            Ok(Command::PgCreate { name })
        }

        "PG-STATUS" => Ok(Command::PgStatus),

        "PG-MIGRATE" => {
            let sql = parts.collect::<Vec<_>>().join(" ");
            let decoded = percent_decode(&sql)?;
            Ok(Command::PgMigrate { sql: decoded })
        }

        "PG-DESTROY" => {
            let name = parts.next().ok_or("PG-DESTROY requires name")?.to_string();
            Ok(Command::PgDestroy { name })
        }

        "REDIS-STATUS" => Ok(Command::RedisStatus),
        "REDIS-PING" => Ok(Command::RedisPing),

        "REDIS-GET" => {
            let key = parts.next().ok_or("REDIS-GET requires key")?.to_string();
            Ok(Command::RedisGet { key })
        }

        "REDIS-SET" => {
            let key = parts.next().ok_or("REDIS-SET requires key")?.to_string();
            let value = parts.collect::<Vec<_>>().join(" ");
            let decoded = percent_decode(&value)?;
            Ok(Command::RedisSet { key, value: decoded })
        }

        "REDIS-SETEX" => {
            let key = parts.next().ok_or("REDIS-SETEX requires key")?.to_string();
            let seconds_str = parts.next().ok_or("REDIS-SETEX requires seconds")?;
            let seconds = seconds_str.parse::<u64>()
                .map_err(|_| format!("invalid seconds: {seconds_str}"))?;
            let value = parts.collect::<Vec<_>>().join(" ");
            let decoded = percent_decode(&value)?;
            Ok(Command::RedisSetEx { key, seconds, value: decoded })
        }

        "REDIS-DEL" => {
            let key = parts.next().ok_or("REDIS-DEL requires key")?.to_string();
            Ok(Command::RedisDel { key })
        }

        "REDIS-KEYS" => {
            let pattern = parts.next().unwrap_or("*").to_string();
            Ok(Command::RedisKeys { pattern })
        }

        "REDIS-TTL" => {
            let key = parts.next().ok_or("REDIS-TTL requires key")?.to_string();
            Ok(Command::RedisTtl { key })
        }

        "REDIS-EXPIRE" => {
            let key = parts.next().ok_or("REDIS-EXPIRE requires key")?.to_string();
            let seconds_str = parts.next().ok_or("REDIS-EXPIRE requires seconds")?;
            let seconds = seconds_str.parse::<u64>()
                .map_err(|_| format!("invalid seconds: {seconds_str}"))?;
            Ok(Command::RedisExpire { key, seconds })
        }

        "REDIS-FLUSH" => Ok(Command::RedisFlush),

        "AUTH-STATUS" => Ok(Command::AuthStatus),

        "AUTH-SET" => {
            let scheme_str = parts.next().ok_or("AUTH-SET requires scheme (bearer|api_key)")?;
            let scheme = AuthScheme::parse(scheme_str)?;
            let token = parts.collect::<Vec<_>>().join(" ");
            let decoded = percent_decode(&token)?;
            Ok(Command::AuthSet { scheme, token: decoded })
        }

        "AUTH-CLEAR" => Ok(Command::AuthClear),

        "TRACE-LIST" => {
            // TODO: parse limit and filter from args
            Ok(Command::TraceList { limit: None, filter: None })
        }

        "TRACE-GET" => {
            let id = parts.next().ok_or("TRACE-GET requires trace ID")?.to_string();
            Ok(Command::TraceGet { id })
        }

        "TRACE-CLEAR" => Ok(Command::TraceClear),

        "TRACE-EXPORT" => {
            let format_str = parts.next().unwrap_or("json");
            let format = ExportFormat::parse(format_str)?;
            Ok(Command::TraceExport { format })
        }

        "METRICS-LIST" => Ok(Command::MetricsList),

        "METRICS-GET" => {
            let name = parts.next().ok_or("METRICS-GET requires metric name")?.to_string();
            Ok(Command::MetricsGet { name })
        }

        "METRICS-CLEAR" => Ok(Command::MetricsClear),

        "METRICS-EXPORT" => {
            let format_str = parts.next().unwrap_or("json");
            let format = ExportFormat::parse(format_str)?;
            Ok(Command::MetricsExport { format })
        }

        unknown => Err(format!("unknown command: {unknown}")),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Encoding utilities
// ══════════════════════════════════════════════════════════════════════════════

/// URL-percent-encode a string (public API)
pub fn encode(s: &str) -> String {
    percent_encode(s)
}

/// URL-percent-decode a string (public API)
pub fn decode(s: &str) -> Result<String, String> {
    percent_decode(s)
}

/// URL-percent-encode a string
fn percent_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            other => format!("%{:02X}", other as u8),
        })
        .collect()
}

/// URL-percent-decode a string
fn percent_decode(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("incomplete percent encoding".to_string());
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| "invalid percent encoding")?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| format!("invalid hex in percent encoding: {hex}"))?;
            result.push(byte as char);
            i += 3;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    Ok(result)
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ping() {
        assert_eq!(parse_command("PING").unwrap(), Command::Ping);
    }

    #[test]
    fn parse_mode_set() {
        match parse_command("MODE-SET full").unwrap() {
            Command::SetMode(DaemonMode::Full) => {},
            other => panic!("expected SetMode(Full), got {other:?}"),
        }
    }

    #[test]
    fn parse_compile() {
        let src = "service%20hello%0Aendpoint%20ping%20GET%20%2Fping";
        let cmd = parse_command(&format!("COMPILE {src}")).unwrap();
        match cmd {
            Command::Compile { source } => {
                assert!(source.contains("service"));
                assert!(source.contains("endpoint"));
            },
            other => panic!("expected Compile, got {other:?}"),
        }
    }

    #[test]
    fn percent_encoding_roundtrip() {
        let original = "hello world\nservice test";
        let encoded = percent_encode(original);
        let decoded = percent_decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn daemon_mode_parsing() {
        assert_eq!(DaemonMode::parse("lite").unwrap(), DaemonMode::Lite);
        assert_eq!(DaemonMode::parse("full").unwrap(), DaemonMode::Full);
        assert_eq!(DaemonMode::parse("ultra").unwrap(), DaemonMode::Ultra);
        assert_eq!(DaemonMode::parse("off").unwrap(), DaemonMode::Off);
        assert!(DaemonMode::parse("invalid").is_err());
    }

    #[test]
    fn auth_scheme_parsing() {
        assert_eq!(AuthScheme::parse("bearer").unwrap(), AuthScheme::Bearer);
        assert_eq!(AuthScheme::parse("api_key").unwrap(), AuthScheme::ApiKey);
        assert_eq!(AuthScheme::parse("apikey").unwrap(), AuthScheme::ApiKey);
        assert!(AuthScheme::parse("invalid").is_err());
    }

    #[test]
    fn export_format_parsing() {
        assert_eq!(ExportFormat::parse("json").unwrap(), ExportFormat::Json);
        assert_eq!(ExportFormat::parse("csv").unwrap(), ExportFormat::Csv);
        assert_eq!(ExportFormat::parse("text").unwrap(), ExportFormat::Text);
        assert_eq!(ExportFormat::parse("txt").unwrap(), ExportFormat::Text);
        assert!(ExportFormat::parse("invalid").is_err());
    }
}
