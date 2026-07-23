# protocol — Bridge Wire Protocol

Shared command/response types used by both the CLI and the daemon.

## Overview

The `protocol` crate defines the message types that flow over the TCP connection between `bridge` CLI and `bridged` daemon. It also provides:

- `Command` / `Response` enums covering all operations
- Rich types for traces, metrics, auth, and structured errors
- Percent-encode/decode helpers for safe binary-over-line transport
- `parse_command(line: &str) → Option<Command>` — one-line parser used by the daemon

**Protocol:** Line-oriented text over TCP. Each command or response is a single `\n`-terminated UTF-8 line with percent-encoded payloads.

## Response Prefixes

| Prefix | Meaning | Example |
|--------|---------|---------|
| `PONG` | Ping reply | `PONG` |
| `OK …` | Success, optional note | `OK compiled successfully` |
| `DATA …` | Encoded payload | `DATA %7B%22services%22%3A%5B%5D%7D` |
| `ERR …` | Error message | `ERR connection refused` |
| `MODE …` | Current daemon mode | `MODE full` |
| `TRACE …` | Trace data (JSON) | `TRACE %7B%22id%22%3A%22abc%22%7D` |
| `METRIC …` | Metric data point | `METRIC counter=http_requests value=42` |

## Command Reference

### Core

| Command | TCP line | Description |
|---------|----------|-------------|
| `Ping` | `PING` | Health check |
| `Version` | `VERSION` | Get daemon version |
| `Stop` | `STOP` | Shutdown daemon |
| `ModeGet` | `MODE_GET` | Get current mode |
| `ModeSet(mode)` | `MODE_SET <mode>` | Set mode (`lite|full|ultra|off`) |

### Compilation

| Command | TCP line | Description |
|---------|----------|-------------|
| `Compile(source)` | `COMPILE <encoded>` | Compile inline source |
| `CompileFile(path)` | `COMPILE_FILE <path>` | Compile file at path |

### Database

| Command | TCP line | Description |
|---------|----------|-------------|
| `DbCreate(name)` | `DB_CREATE <name>` | Create Postgres container |
| `DbStatus` | `DB_STATUS` | Get container status |
| `DbMigrate(name, sql)` | `DB_MIGRATE <name> <encoded>` | Run SQL migration |
| `DbDestroy(name)` | `DB_DESTROY <name>` | Remove container |

### Redis / Cache

| Command | TCP line | Description |
|---------|----------|-------------|
| `RedisStatus` | `REDIS_STATUS` | Check miniredis status |
| `RedisGet(key)` | `REDIS_GET <key>` | Get a key |
| `RedisSet(key, val)` | `REDIS_SET <key> <encoded>` | Set a key |
| `RedisDel(key)` | `REDIS_DEL <key>` | Delete a key |
| `RedisKeys(pattern)` | `REDIS_KEYS <pattern>` | List keys |

### Auth

| Command | TCP line | Description |
|---------|----------|-------------|
| `AuthSet(scheme, token)` | `AUTH_SET <scheme> <encoded>` | Set auth token |
| `AuthGet` | `AUTH_GET` | Get current auth config |
| `AuthClear` | `AUTH_CLEAR` | Remove auth token |

### Tracing

| Command | TCP line | Description |
|---------|----------|-------------|
| `TraceList` | `TRACE_LIST` | List recent traces |
| `TraceGet(id)` | `TRACE_GET <id>` | Get trace by ID |
| `TraceExport(fmt)` | `TRACE_EXPORT <format>` | Export traces (json|prometheus) |
| `TraceClear` | `TRACE_CLEAR` | Clear trace buffer |

### Metrics

| Command | TCP line | Description |
|---------|----------|-------------|
| `MetricsList` | `METRICS_LIST` | List all metrics |
| `MetricsGet(name)` | `METRICS_GET <name>` | Get metric by name |
| `MetricsReset` | `METRICS_RESET` | Reset all counters |

## API Reference

### `parse_command(line: &str) → Option<Command>`

Parses a single TCP line into a `Command`. Returns `None` for empty/unknown lines. Used by `daemon/tcp.rs`.

```rust
use protocol::parse_command;

let cmd = parse_command("PING").unwrap();
assert!(matches!(cmd, Command::Ping));

let cmd = parse_command("DB_CREATE myapp").unwrap();
assert!(matches!(cmd, Command::DbCreate(n) if n == "myapp"));
```

### `encode(s: &str) → String`

Percent-encodes a string for safe TCP transport (spaces, newlines, special chars).

```rust
use protocol::encode;
let line = format!("COMPILE {}", encode("service users\nendpoint list GET /users\n"));
```

### `decode(s: &str) → String`

Percent-decodes a received payload.

```rust
use protocol::decode;
let source = decode("service%20users%0Aendpoint%20list%20GET%20%2Fusers");
```

## Key Types

### `DaemonMode`

```rust
pub enum DaemonMode { Lite, Full, Ultra, Off }
```

Modes control which daemon subsystems are active:

| Mode | Active features |
|------|----------------|
| `lite` | TCP + compiler only |
| `full` | TCP + HTTP + compiler + Docker + Redis |
| `ultra` | full + metrics + traces + rate-limiting |
| `off` | Daemon is stopped |

### `Trace` / `Span` / `LogEntry`

Rich trace types for the observability subsystem:

```rust
pub struct Trace {
    pub id:         String,
    pub service:    String,
    pub endpoint:   String,
    pub spans:      Vec<Span>,
    pub logs:       Vec<LogEntry>,
    pub start_ms:   u64,
    pub duration_ms: u64,
    pub status:     u16,
    pub sampled:    bool,
}
```

### `Metric`

```rust
pub struct Metric {
    pub name:   String,
    pub kind:   MetricKind,   // Counter | Gauge | Histogram
    pub value:  f64,
    pub labels: HashMap<String, String>,
    pub ts_ms:  u64,
}
```

### `AuthScheme`

```rust
pub enum AuthScheme { Bearer, ApiKey }
```

## Design Notes

- **Line-oriented** — each message is exactly one `\n`-terminated line, making it easy to implement with a simple `BufRead` reader in both CLI and daemon.
- **Percent-encoding** — all freeform payloads (source code, SQL, tokens) are percent-encoded so they never contain `\n`, allowing the simple line framing to work with any content.
- **Shared types** — both `cli` and `daemon` depend on this crate, ensuring the CLI can never send a command the daemon doesn't understand.
- **Zero external dependencies** — pure `std`.
- **VERSION constant** — single source of truth for the protocol version (`"0.2.0"`).
