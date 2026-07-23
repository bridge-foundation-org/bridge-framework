# Bridge Framework — Implementation Plan

Bridge is a lightweight Encore-inspired framework. This document tracks the implementation status of all features, following the Encore design patterns from [latest-encore.xml](latest-encore.xml).

## ✅ Completed Features

### Core Infrastructure
- [x] **Protocol crate** — Command/Response protocol with DB/Redis commands
- [x] **Daemon** — TCP + HTTP server with modular architecture (state, sqldb, tcp, http)
- [x] **CLI** — Full command suite including db-create, db-status, db-migrate, db-destroy, redis-status
- [x] **Miniredis** — Embedded Redis-compatible cache server with RESP protocol
- [x] **Database management** — Docker Postgres lifecycle via sqldb module
- [x] **Compiler** — Bridge DSL parser with route conflict detection, middleware validation
- [x] **Codegen** — TypeScript client generation + OpenAPI 3.0 spec generation
- [x] **In-memory store** — db crate with namespace/key-value storage, TTL, transactions

### Auth & Security
- [x] **Auth middleware** — HTTP auth enforcement (Bearer, X-Api-Key, X-Bridge-Token)
- [x] **Request ID tracking** — X-Bridge-Request-Id header on every response
- [x] **Structured 401 responses** — JSON error body on auth failure
- [x] **JSON auth_set** — POST /api/v1/auth/set accepts `{scheme, token}` body

### Middleware System
- [x] **Middleware registry** — Composable before/after hook chain
- [x] **Scoped middleware** — Global / Service / Endpoint scope targeting
- [x] **Built-in hook specs** — `log`, `reject:<status>:<msg>`, `header:<key>:<value>`
- [x] **HTTP API** — GET/POST/DELETE /api/v1/middleware
- [x] **Chain short-circuit** — Before hooks can reject with any HTTP status

### Hot Reload
- [x] **File watcher** — Background thread polls .bridge files every N ms
- [x] **Auto-recompile** — Recompiles on mtime change, updates service_registry
- [x] **SSE events** — `event: reload` / `event: error` broadcast to connected clients
- [x] **HTTP API** — GET/POST/DELETE /api/v1/watch/files, POST /api/v1/watch/dirs
- [x] **SSE stream** — GET /api/v1/watch/events (chunked keep-alive)
- [x] **BRIDGE_WATCH_DIR** — Auto-watch directory via env var on startup

### Rate Limiting
- [x] **Token-bucket algorithm** — Lazy refill, per-endpoint buckets
- [x] **Wildcard rules** — `*` in method or path matches any value
- [x] **Specificity order** — Exact > any-method > any-path > global wildcard
- [x] **429 responses** — JSON error body with retry_after
- [x] **Rate-limit headers** — X-RateLimit-Limit/Remaining/Reset, Retry-After
- [x] **HTTP API** — GET/POST/DELETE /api/v1/ratelimit
- [x] **Middleware integration** — `RateLimiter::as_middleware()` for registry use

### Project Configuration
- [x] **bridge.toml** — Pure-std TOML parser, no external crates
- [x] **[project] section** — name, version
- [x] **[daemon] section** — http_addr, tcp_addr, redis_addr, mode
- [x] **[watch] section** — enabled, poll_ms, dirs, files
- [x] **[[middleware.rules]]** — name, scope, before, after
- [x] **[[ratelimit.rules]]** — method, path, capacity, refill_rate
- [x] **Auto-load at startup** — BRIDGE_CONFIG env var or ./bridge.toml
- [x] **bridge init** — Writes bridge.toml with sensible defaults
- [x] **GET /api/v1/config** — Runtime config summary endpoint

### Observability
- [x] **Trace recording** — Every HTTP request recorded with method/path/status/duration
- [x] **Prometheus metrics** — GET /api/v1/metrics/prometheus in text/plain; version=0.0.4 format
- [x] **Structured logging** — Per-request log entries with level/timestamp/fields
- [x] **Sampling rate** — Configurable trace sampling (0.0–1.0)

### Miniredis
- [x] **String commands** — GET/SET/MGET/MSET/INCR/DECR/INCRBY/DECRBY/SETNX/SETEX
- [x] **List commands** — LPUSH/RPUSH/LRANGE/LLEN/LINDEX
- [x] **Hash commands** — HSET/HSETNX/HGET/HMGET/HGETALL/HDEL/HLEN/HEXISTS/HKEYS/HVALS/HINCRBY
- [x] **Key commands** — DEL/EXISTS/EXPIRE/TTL/KEYS/TYPE/FLUSHDB

### Advanced Daemon Modules
- [x] **Auth registry** — Bearer token + API key validation with expiry
- [x] **Metrics registry** — Counter/Gauge/Histogram with Prometheus export
- [x] **Pub/Sub broker** — In-memory topic/subscription message queue
- [x] **Secrets registry** — Inline secret storage with required-check
- [x] **Streaming registry** — Endpoint registration and SSE support
- [x] **Structured errors** — Error codes (NotFound/Unauthenticated/Internal etc.)
- [x] **Logger** — Structured log entries with trace correlation

### Frontend
- [x] **Dev Dashboard** — Encore-inspired UI with Overview, API Explorer, Infrastructure, Docs tabs
- [x] **Database Panel** — Create/Status/Migrate/Destroy operations with Docker management
- [x] **Redis Panel** — Status monitoring and connection tracking
- [x] **Service Catalog** — Endpoint parser and visualization
- [x] **API Tester** — Interactive HTTP endpoint explorer
- [x] **Daemon client** — TypeScript client for all daemon operations
- [x] **Tailwind v4** — Modern styling with Encore-inspired design system

### Testing
- [x] **370 workspace tests** — All passing (0 failures)
- [x] **28 e2e unit tests** — Compiler→Codegen pipeline, protocol, db, miniredis RESP
- [x] **36 daemon e2e tests** — Full TCP+HTTP+Redis integration (require running daemon)
- [x] **210 daemon unit tests** — TCP, HTTP, auth, metrics, middleware, watcher, ratelimit, config

### Documentation
- [x] **Index** — Overview and quick start
- [x] **Installation guide** — Setup instructions
- [x] **Benefits** — Framework value proposition
- [x] **CLI Reference** — Complete command documentation
- [x] **Architecture** — System design overview
- [x] **Database guide** — Docker Postgres management
- [x] **Caching guide** — Miniredis integration
- [x] **Deployment** — Production deployment strategies
- [x] **API Reference** — HTTP endpoint documentation
- [x] **Tutorials** — Step-by-step guides

## 🚧 In Progress

### Code Modularity
- [ ] **Module README files** — Comprehensive documentation for each crate
- [ ] **Docker Compose** — One-command infrastructure setup
- [ ] **Enhanced error messages** — More context in compiler/runtime errors

## 📋 Planned Features

### Developer Experience
- [ ] **CLI autocomplete** — Shell completions (bash/zsh/fish) ← already in CLI, needs `bridge completions` docs
- [ ] **Project templates** — Additional scaffold templates beyond default

### Advanced Features
- [ ] **WebSocket support** — Real-time communication via SSE streaming
- [ ] **Multi-service routing** — Service mesh capabilities
- [ ] **Config file** — `bridge.toml` ✅ (completed)

## Architecture Principles

Following Encore's design:

1. **Modular** — Each crate has a single responsibility
2. **Zero-config** — Sensible defaults, minimal setup
3. **Type-safe** — Generated clients match backend contracts
4. **Docker-first** — Infrastructure via containers
5. **Developer-friendly** — Clear errors, great docs, easy contribution

## Docker Dependency

> [!IMPORTANT]
> PostgreSQL management requires Docker. The daemon gracefully handles missing Docker with clear error messages.

## No External Dependencies

> [!NOTE]
> All Rust code uses only `std` — no tokio, no serde. This keeps the project minimal and compilation fast.


