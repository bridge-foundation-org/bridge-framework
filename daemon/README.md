# Bridge Daemon

The heart of the Bridge framework — a modular Rust server that orchestrates compilation, codegen, database management, caching, middleware, hot-reload watching, rate limiting, and configuration.

## Overview

The daemon exposes two interfaces:

- **TCP** `127.0.0.1:7878` — Line-oriented protocol for the CLI
- **HTTP** `127.0.0.1:8787` — REST API for the dev dashboard

On startup:

1. Reads `bridge.toml` via `config::BridgeConfig::load()` (optional)
2. Creates shared `Arc<Mutex<State>>`
3. Applies config with `config::apply()`
4. Starts miniredis in a background thread
5. Spawns HTTP server thread
6. Starts hot-reload watcher thread
7. Runs TCP server on the main thread (blocking)

## Architecture

```
                     ┌──────────────┐
                     │    main.rs   │
                     └──────┬───────┘
                            │
            ┌───────────────┼────────────────┐
            │               │                │
            ▼               ▼                ▼
    ┌─────────────┐  ┌──────────────┐  ┌───────────┐
    │   tcp.rs    │  │   http.rs    │  │ miniredis │
    │  CLI proto  │  │  REST API    │  │  (thread) │
    └──────┬──────┘  └──────┬───────┘  └───────────┘
           │                │
           └───────┬─────────┘
                   │
          ┌────────▼─────────┐
          │     state.rs     │
          │  Arc<Mutex<State>>│
          └─────────┬────────┘
                    │
     ┌──────────────┼──────────────────┐
     │              │                  │
     ▼              ▼                  ▼
┌─────────┐  ┌────────────┐   ┌───────────────┐
│ sqldb.rs│  │ config.rs  │   │ middleware.rs  │
│  Docker │  │ TOML parse │   │  Hook chains  │
└─────────┘  └────────────┘   └───────┬───────┘
                                       │
                               ┌───────┴────────┐
                               │                │
                          ┌────▼────┐    ┌──────▼─────┐
                          │watcher.rs│   │ratelimit.rs│
                          │hot-reload│   │token bucket│
                          └──────────┘   └────────────┘
```

## Modules

### main.rs

Entry point. Reads `BRIDGE_TCP_ADDR`, `BRIDGE_HTTP_ADDR`, `BRIDGE_REDIS_ADDR` env vars (defaults: `7878`, `8787`, `6399`). Loads `BRIDGE_CONFIG` or `./bridge.toml`, starts all servers.

### state.rs

`State` struct wrapped in `Arc<Mutex<State>>`:

```rust
pub struct State {
    pub mode:             DaemonMode,
    pub store:            Db,
    pub auth_token:       Option<String>,
    pub service_registry: Option<BridgeFile>,
    pub traces:           Vec<TraceEntry>,
    pub metrics:          Metrics,
    pub metric_registry:  MetricsRegistry,
    pub logs:             Vec<LogEntry>,
    pub redis_addr:       Option<String>,
    pub trace_sample_rate: f64,
    pub app_name:         String,
    pub app_version:      String,
    pub pubsub:           Broker,
    pub secrets:          SecretsRegistry,
    pub streams:          StreamRegistry,
    pub middleware:       MiddlewareRegistry,   // ← new
    pub watcher:          WatchRegistry,        // ← new
    pub rate_limiter:     RateLimiter,          // ← new
}
```

### tcp.rs

Line-protocol server. Reads one `\n`-terminated command, dispatches, writes one response, closes connection. See the [TCP Reference](#tcp-protocol-reference) below.

### http.rs

HTTP REST server. Every request goes through:

1. CORS preflight check
2. SSE path check (`/api/v1/watch/events` — handled inline)
3. Auth enforcement (if token configured)
4. **Rate-limit check** (token bucket)
5. **Middleware before hooks**
6. Route handler
7. **Middleware after hooks**
8. Trace + log recording

Exposes the following endpoints:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/health` | Daemon health + metadata |
| GET | `/api/v1/version` | Version |
| GET | `/api/v1/mode` | Current mode |
| POST | `/api/v1/mode` | Set mode |
| POST | `/api/v1/compile` | Compile Bridge DSL |
| GET | `/api/v1/services` | Registered services |
| GET | `/api/v1/routes` | All routes |
| GET | `/api/v1/codegen/latest` | Latest TypeScript output |
| GET | `/api/v1/auth/status` | Auth status |
| POST | `/api/v1/auth/set` | Set auth token |
| DELETE | `/api/v1/auth/clear` | Clear auth token |
| GET | `/api/v1/traces` | Recent traces (optional `?limit=N`) |
| GET | `/api/v1/traces/:id` | Specific trace |
| DELETE | `/api/v1/traces` | Clear traces |
| GET | `/api/v1/metrics` | Request metrics JSON |
| GET | `/api/v1/metrics/prometheus` | Prometheus text format |
| DELETE | `/api/v1/metrics` | Reset metrics |
| POST | `/api/v1/sampling` | Set trace sample rate (0.0–1.0) |
| GET | `/api/v1/openapi` | OpenAPI 3.0 spec |
| GET | `/api/v1/middleware` | List middleware |
| POST | `/api/v1/middleware` | Register middleware |
| DELETE | `/api/v1/middleware` | Remove middleware |
| GET | `/api/v1/ratelimit` | List rate-limit rules |
| POST | `/api/v1/ratelimit` | Add rule |
| DELETE | `/api/v1/ratelimit` | Remove rule |
| GET | `/api/v1/watch` | Watcher status |
| POST | `/api/v1/watch/files` | Add file to watch |
| DELETE | `/api/v1/watch/files` | Remove file from watch |
| POST | `/api/v1/watch/dirs` | Scan directory for `.bridge` files |
| GET | `/api/v1/watch/events` | SSE hot-reload stream |
| GET | `/api/v1/config` | Runtime config summary |
| GET | `/api/v1/pg/status` | Docker Postgres status |
| POST | `/api/v1/pg/create` | Create container |
| POST | `/api/v1/pg/migrate` | Run SQL migration |
| DELETE | `/api/v1/pg/destroy` | Destroy container |
| GET | `/api/v1/redis/status` | Miniredis status |

Legacy paths (without `/api/v1`) are also supported for backwards compatibility.

### sqldb.rs

Docker Postgres lifecycle via `std::process::Command`:

```rust
pub fn create(name: &str) -> Result<String, String>
pub fn status()            -> Result<String, String>
pub fn migrate(sql: &str)  -> Result<String, String>
pub fn destroy(name: &str) -> Result<String, String>
```

Gracefully handles missing Docker with a clear error message.

### auth.rs

`AuthRegistry` stores Bearer tokens and API keys with optional expiry. Used by `http.rs` auth enforcement via the `check_auth()` function.

### metrics.rs

`Registry` with `Counter`, `Gauge`, and `Histogram` metric types. `register_defaults()` sets up standard bridge metrics at startup. Prometheus export via `export_prometheus()`.

### pubsub.rs

`Broker` — in-memory topic/subscription message queue. Topics are created on first publish. Subscriptions pull messages via `pull()`. Dead-letter queue after `MAX_RETRIES`.

### secrets.rs

`SecretsRegistry` — stores secrets as inline values or `env:VAR_NAME` references. `check_required()` verifies all required secrets are present at startup.

### streaming.rs

`StreamRegistry` — SSE endpoint registration. Tracks open streams by ID. Provides `sse_event()` formatting and `write_sse_event()` helper.

### errors.rs

`BridgeError` with `Code` enum (`NotFound`, `Unauthenticated`, `Internal`, etc.). Serialises to JSON for HTTP responses and plain text for TCP. `pub type Result<T> = std::result::Result<T, BridgeError>`.

### logger.rs

`StructuredLogger` — writes JSON log lines to stderr. `LogEntry` with level, message, timestamp, and arbitrary fields.

---

### middleware.rs

Composable before/after hook chains for HTTP requests.

**Key types:**

| Type | Description |
|------|-------------|
| `MiddlewareRegistry` | Stores entries, runs chains |
| `MiddlewareEntry` | Named, scoped before+after pair |
| `MiddlewareContext` | Carries method, path, tags, extra headers, rejection |
| `Scope` | `Global` \| `Service(name)` \| `Endpoint{method,path}` |
| `Hook` | `Box<dyn Fn(&mut MiddlewareContext) + Send + Sync>` |

**Execution:**

```
run_before(): entries[0].before → entries[1].before → ... (stops on rejection)
run_after():  entries[n].after  → ... → entries[0].after  (reverse order)
```

**Built-in hook specs** (used by HTTP API and bridge.toml):

| Phase | Spec | Effect |
|-------|------|--------|
| before | `log` | Tags context with `"logged"` |
| before | `reject:STATUS:MSG` | Short-circuits with HTTP STATUS |
| after | `log` | Tags context with `"logged-after"` |
| after | `header:KEY:VALUE` | Injects response header |

**HTTP endpoints:** `GET/POST/DELETE /api/v1/middleware`

---

### watcher.rs

Hot-reload background thread. Polls `.bridge` file `mtime` every `poll_ms` ms. On change: recompiles, updates `State.service_registry`, broadcasts SSE event.

**Key types:**

| Type | Description |
|------|-------------|
| `WatchRegistry` | Stores files, dirs, SSE clients |
| `WatchedFile` | Path + last_mtime + change_count + last_result |
| `CompileResult` | `Ok(ts)` \| `Err(msg)` \| `Pending` |
| `SseSender` | Wraps `SyncSender<String>` |

**SSE event format:**

```
event: reload
data: {"file":"app.bridge","status":"ok","ts":1720000000}

event: error
data: {"file":"app.bridge","status":"error","message":"...","ts":1720000000}

: keepalive
```

**HTTP endpoints:** `GET/POST/DELETE /api/v1/watch/files`, `POST /api/v1/watch/dirs`, `GET /api/v1/watch/events` (SSE), `GET /api/v1/watch`

**`BRIDGE_WATCH_DIR` env var:** auto-watches a directory on startup.

---

### ratelimit.rs

Per-endpoint token-bucket throttling.

**Key types:**

| Type | Description |
|------|-------------|
| `RateLimiter` | `HashMap<BucketKey, TokenBucket>` |
| `TokenBucket` | Lazy-refill token counter |
| `BucketKey` | `{method, path}` with `*` wildcard support |

**Specificity order:** exact → any-method → any-path → global wildcard.

**Response headers on rate-limited routes:**
- `X-RateLimit-Limit` / `X-RateLimit-Remaining` / `X-RateLimit-Reset`
- `Retry-After` on `429` responses

**`as_middleware(Arc<Mutex<RateLimiter>>, name)`** — wraps the limiter as a `MiddlewareEntry` for the registry.

**HTTP endpoints:** `GET/POST/DELETE /api/v1/ratelimit`

---

### config.rs

Pure-std TOML parser. No external crates.

**Sections parsed:**

| Section | Keys |
|---------|------|
| `[project]` | `name`, `version` |
| `[daemon]` | `http_addr`, `tcp_addr`, `redis_addr`, `mode` |
| `[watch]` | `enabled`, `poll_ms`, `dirs`, `files` |
| `[[middleware.rules]]` | `name`, `scope`, `before`, `after` |
| `[[ratelimit.rules]]` | `method`, `path`, `capacity`, `refill_rate` |

**`apply(cfg, state)`** — wires parsed config into `State`:
- Sets `state.mode` from `[daemon].mode`
- Sets `state.app_name` from `[project].name`
- Configures `state.watcher` dirs/files/poll_ms
- Registers `[[middleware.rules]]` entries
- Adds `[[ratelimit.rules]]` token buckets

**HTTP endpoint:** `GET /api/v1/config`

---

## TCP Protocol Reference

```
PING                         → PONG
VERSION                      → DATA <encoded>
HEALTH                       → DATA <encoded-json>
HELP                         → DATA <encoded-text>
MODE GET                     → MODE <mode>
MODE SET <mode>              → OK MODE=<mode>
COMPILE <encoded-source>     → DATA <encoded-ts>
SERVICES LIST                → DATA <encoded-json>
ROUTES LIST                  → DATA <encoded-json>
AUTH STATUS                  → DATA <encoded-json>
AUTH SET <scheme> <token>    → OK token set
AUTH CLEAR                   → OK cleared
TRACE LIST [limit]           → DATA <encoded-json>
TRACE GET <id>               → DATA <encoded-json>
TRACE CLEAR                  → OK cleared
TRACE EXPORT [format]        → DATA <encoded>
METRICS LIST                 → DATA <encoded-json>
METRICS CLEAR                → OK cleared
DB PUT <ns> <key> <val>      → OK stored
DB GET <ns> <key>            → DATA <val> | ERR not found
DB DEL <ns> <key>            → OK deleted
DB KEYS <ns>                 → DATA <encoded-json>
DB FLUSH <ns>                → OK flushed
PG CREATE <name>             → OK <msg>
PG STATUS                    → DATA <status>
PG MIGRATE <encoded-sql>     → DATA <result>
PG DESTROY <name>            → OK <msg>
REDIS STATUS                 → DATA <encoded-json>
REDIS PING                   → DATA PONG
REDIS FLUSH                  → OK flushed
REDIS GET <key>              → DATA <val>
REDIS SET <key> <encoded-val>→ OK stored
REDIS DEL <key>              → OK deleted
REDIS KEYS [pattern]         → DATA <encoded-json>
```

Values with spaces/special characters are percent-encoded using `protocol::encode()`.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BRIDGE_TCP_ADDR` | `127.0.0.1:7878` | TCP server bind address |
| `BRIDGE_HTTP_ADDR` | `127.0.0.1:8787` | HTTP server bind address |
| `BRIDGE_REDIS_ADDR` | `127.0.0.1:6399` | Miniredis bind address |
| `BRIDGE_CONFIG` | `bridge.toml` | Config file path |
| `BRIDGE_WATCH_DIR` | — | Directory to auto-watch on startup |
| `BRIDGE_CORS_ORIGIN` | `*` | CORS origin header value |

## Testing

```bash
# Unit tests (no daemon needed)
cargo test -p daemon

# Run with custom ports
BRIDGE_TCP_ADDR=127.0.0.1:17878 \
BRIDGE_HTTP_ADDR=127.0.0.1:18787 \
cargo run -p daemon

# Health check
curl http://localhost:8787/api/v1/health

# Watch SSE stream
curl -N http://localhost:8787/api/v1/watch/events

# Inspect config
curl http://localhost:8787/api/v1/config
```

## Dependencies

All workspace-local crates, no external Rust dependencies:

- `protocol` — Command/Response types, encode/decode
- `compiler` — Bridge DSL parser
- `codegen` — TypeScript + OpenAPI generator
- `db` — In-memory KV store with TTL
- `miniredis` — Embedded Redis-compatible server

## License

MIT — see [LICENSE](../LICENSE).
