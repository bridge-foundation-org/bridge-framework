# API Reference

## Base URL

`http://127.0.0.1:8787` (override with `BRIDGE_HTTP_ADDR` env var)

All endpoints are also available at legacy paths without the `/api/v1` prefix for backwards compatibility.

## CORS

Every response includes:
```
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS
Access-Control-Allow-Headers: Content-Type, Authorization, X-Bridge-Token, X-Api-Key
```

`OPTIONS` requests always return `204 No Content`.

## Request ID

Every response includes `X-Bridge-Request-Id: req-xxxxxxxx` for tracing.

## Auth

When an auth token is configured (`POST /api/v1/auth/set`), all endpoints except health and auth management require one of:

- `Authorization: Bearer <token>`
- `X-Api-Key: <token>`
- `X-Bridge-Token: <token>`

Unauthorized requests get `401` with body `{"error":"unauthenticated","message":"..."}`.

---

## Endpoint Index

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/health` | Daemon health + metadata |
| GET | `/api/v1/version` | Version string |
| GET | `/api/v1/mode` | Current daemon mode |
| POST | `/api/v1/mode` | Set mode (`lite\|full\|ultra\|off`) |
| POST | `/api/v1/compile` | Compile Bridge DSL → TypeScript |
| GET | `/api/v1/services` | List registered services |
| GET | `/api/v1/routes` | List all routes |
| GET | `/api/v1/codegen/latest` | Latest codegen output |
| GET | `/api/v1/auth/status` | Auth token status |
| POST | `/api/v1/auth/set` | Set auth token |
| DELETE | `/api/v1/auth/clear` | Clear auth token |
| GET | `/api/v1/traces` | List recent traces |
| GET | `/api/v1/traces/:id` | Get trace by ID |
| DELETE | `/api/v1/traces` | Clear all traces |
| GET | `/api/v1/metrics` | Request metrics summary |
| GET | `/api/v1/metrics/prometheus` | Prometheus text format |
| DELETE | `/api/v1/metrics` | Reset metrics |
| POST | `/api/v1/sampling` | Set trace sampling rate (0.0–1.0) |
| GET | `/api/v1/openapi` | OpenAPI 3.0 spec (requires prior compile) |
| GET | `/api/v1/middleware` | List middleware rules |
| POST | `/api/v1/middleware` | Register middleware |
| DELETE | `/api/v1/middleware` | Remove middleware |
| GET | `/api/v1/ratelimit` | List rate-limit rules |
| POST | `/api/v1/ratelimit` | Add rate-limit rule |
| DELETE | `/api/v1/ratelimit` | Remove rate-limit rule |
| GET | `/api/v1/watch` | Watcher status |
| POST | `/api/v1/watch/files` | Add file to watch |
| DELETE | `/api/v1/watch/files` | Remove file from watch |
| POST | `/api/v1/watch/dirs` | Add directory to watch |
| GET | `/api/v1/watch/events` | SSE hot-reload event stream |
| GET | `/api/v1/config` | Runtime config summary |
| GET | `/api/v1/pg/status` | Docker Postgres status |
| POST | `/api/v1/pg/create` | Create Postgres container |
| POST | `/api/v1/pg/migrate` | Run SQL migration |
| DELETE | `/api/v1/pg/destroy` | Destroy Postgres container |
| GET | `/api/v1/redis/status` | Miniredis status |
| GET | `/api/v1/pubsub` | Pub/Sub broker status |
| POST | `/api/v1/pubsub/topics` | Create topic |
| POST | `/api/v1/pubsub/publish` | Publish message |
| GET | `/api/v1/pubsub/subscriptions` | List subscriptions |
| POST | `/api/v1/pubsub/subscriptions` | Subscribe |
| GET | `/api/v1/pubsub/subscriptions/:topic/:subscriber` | Subscription detail |
| POST | `/api/v1/pubsub/subscriptions/:topic/:subscriber/pull` | Pull next message |
| POST | `/api/v1/pubsub/ack` | Acknowledge message |
| POST | `/api/v1/pubsub/nack` | Negative-acknowledge (retry/DLQ) |
| GET | `/api/v1/pubsub/dlq/:topic/:subscriber` | Dead-letter queue contents |
| GET | `/api/v1/cache` | Cache status (keyspaces/entries/hits/misses) |
| GET | `/api/v1/cache/keyspaces` | List keyspaces |
| POST | `/api/v1/cache/keyspaces` | Declare keyspace |
| GET | `/api/v1/cache/keyspaces/:ks` | Keyspace stats (+`?entries=1`) |
| DELETE | `/api/v1/cache/keyspaces/:ks?pattern=` | Invalidate (glob or all) |
| GET | `/api/v1/cache/entry/:ks/:key` | Get entry (miss → 404) |
| PUT | `/api/v1/cache/entry/:ks/:key?ttl_ms=` | Set entry |
| DELETE | `/api/v1/cache/entry/:ks/:key` | Delete entry |
| POST | `/api/v1/cache/mget` | Batch get |
| POST | `/api/v1/cache/mset` | Batch set |

---

## Core

### GET /api/v1/health

```json
{
  "status": "ok",
  "version": "0.2.0",
  "app": "bridge",
  "mode": "full",
  "redis": "127.0.0.1:6399",
  "redis_connections": 0,
  "services": 1,
  "traces": 42,
  "sample_rate": 1.0
}
```

### POST /api/v1/compile

Body: raw Bridge DSL source. Returns TypeScript client source (plain text).

```bash
curl -X POST http://localhost:8787/api/v1/compile \
  --data 'service hello\nendpoint ping GET /ping'
```

---

## Auth

### POST /api/v1/auth/set

Accepts plain token or JSON body:

```json
{"scheme": "bearer", "token": "my-secret-token"}
```

### GET /api/v1/auth/status

```json
{"configured": true, "scheme": "bearer"}
```

---

## Traces

### GET /api/v1/traces?limit=N

```json
[
  {"id":"t00000001","method":"GET","path":"/api/v1/health","status":200,"duration_ms":1,"timestamp":1720000000}
]
```

---

## Metrics

### GET /api/v1/metrics

```json
{
  "total_requests": 42,
  "total_errors": 0,
  "endpoints": [
    {"endpoint":"GET /api/v1/health","requests":10,"errors":0,"avg_ms":1}
  ]
}
```

### GET /api/v1/metrics/prometheus

Returns Prometheus text format (`Content-Type: text/plain; version=0.0.4`).

---

## Middleware

See [middleware.md](middleware.md) for full documentation.

### POST /api/v1/middleware

```json
{
  "name":   "logger",
  "scope":  "global",
  "before": "log",
  "after":  "header:X-Powered-By:bridge"
}
```

Supported `scope` values: `"global"`, `"service:NAME"`, `"METHOD:/path"`.

Supported `before` specs: `"log"`, `"reject:STATUS:msg"`.

Supported `after` specs: `"log"`, `"header:KEY:VALUE"`.

---

## Rate Limiting

See [ratelimit.md](ratelimit.md) for full documentation.

### POST /api/v1/ratelimit

```json
{
  "method":      "POST",
  "path":        "/api/v1/compile",
  "capacity":    60,
  "refill_rate": 1.0
}
```

On rate-limited requests (`429`):
```
Retry-After: 5
X-RateLimit-Remaining: 0
```

On allowed requests:
```
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 59
X-RateLimit-Reset: 1720001000
```

---

## Hot Reload

See [watcher.md](watcher.md) for full documentation.

### GET /api/v1/watch/events

SSE stream. Events:

```
event: reload
data: {"file":"app.bridge","status":"ok","ts":1720000000}

event: error
data: {"file":"app.bridge","status":"error","message":"parse error at line 3","ts":1720000000}

: keepalive
```

---

## Config

See [config.md](config.md) for full documentation.

### GET /api/v1/config

```json
{
  "app": "my-app",
  "version": "0.1.0",
  "mode": "full",
  "middleware": ["logger", "powered-by"],
  "ratelimit": [
    {"method":"POST","path":"/api/v1/compile","capacity":60,"refill_rate":1.0,"remaining":60}
  ],
  "watch": {"enabled": true, "poll_ms": 500, "files": ["app.bridge"]}
}
```

---

## Pub/Sub

In-process message broker (Encore topics/subscriptions semantics):
fan-out publish, at-least-once pull delivery, ack/nack with retry and
dead-letter queues, optional strict FIFO ordering.

### POST /api/v1/pubsub/topics

```json
{"name": "orders"}
```

`409` when the topic already exists. Topics also materialize implicitly on
first publish or subscribe.

### POST /api/v1/pubsub/publish

```json
{
  "topic": "orders",
  "payload": {"id": 1},
  "ordering_key": "user-42",
  "attrs": {"source": "web"}
}
```

Only `topic` is required; `payload` defaults to `null`. Fans out to every
attached subscriber. Response reports the real fan-out width:

```json
{"message":"published","id":"msg-1720000000-7","topic":"orders","subscribers":2}
```

### POST /api/v1/pubsub/subscriptions

```json
{
  "topic": "orders",
  "subscriber": "billing",
  "max_retries": 3,
  "ack_deadline_secs": 30,
  "message_ordering": true
}
```

All fields except `topic`/`subscriber` are optional. Re-subscribing updates
the config in place (never double-delivers) and responds
`{"message":"subscription updated",...}`.

### POST /api/v1/pubsub/subscriptions/:topic/:subscriber/pull

Delivers the next pending message and marks it in-flight:

```json
{"message":{"id":"msg-1720000000-7","topic":"orders","payload":{"id":1},"published_at":1720000000,"attempt":1,"ordering_key":"user-42","source":"web"},"topic":"orders","subscriber":"billing"}
```

Empty or ordering-blocked queues return `{"message":null,...,"reason":"empty or ordering-blocked"}`.
Unknown subscription → `404`. With `message_ordering: true`, no further
messages are delivered until the in-flight head is acked or nacked.

### POST /api/v1/pubsub/ack · /api/v1/pubsub/nack

```json
{"id": "msg-1720000000-7", "reason": "transient failure"}
```

`ack` settles the message. `nack` requeues it until `attempt` exceeds
`max_retries`, after which it moves to that subscription's dead-letter
queue (`reason` optional, default `"error"`). Settling an id that is not in
flight → `404`.

### GET /api/v1/pubsub/dlq/:topic/:subscriber

```json
{"topic":"jobs","subscriber":"w","messages":[{"id":"msg-...","payload":{"t":1},...}]}
```

---

## Cache

In-memory keyspace cache (Encore `RedisCluster` in-memory mode): named
keyspaces with per-keyspace capacity + TTL defaults, LRU eviction, and
hit/miss counters. Values are raw JSON tokens — stored verbatim, returned
verbatim.

### POST /api/v1/cache/keyspaces

```json
{"name": "sessions", "max_entries": 1000, "default_ttl_ms": 300000}
```

Only `name` is required; omitted limits fall back to the `[cache]`
section of `bridge.toml`. Re-declaring updates the config but keeps data.

### PUT /api/v1/cache/entry/:ks/:key?ttl_ms=60000

Body is any JSON value, stored as-is:

```json
{"id": 1, "name": "ann"}
```

`ttl_ms=0` means never expires. Response includes how many entries were
LRU-evicted to enforce `max_entries`:

```json
{"message":"cached","keyspace":"ks","key":"user:1","evicted":0}
```

### GET /api/v1/cache/entry/:ks/:key

```json
{"key":"user:1","value":{"id":1,"name":"ann"},"ttl_ms_left":59999}
```

`ttl_ms_left` is `null` for entries without expiry. Unknown or expired
keys return `404` with `{"error":"cache miss"}` — both count toward the
keyspace's `misses` stat.

### DELETE /api/v1/cache/keyspaces/:ks?pattern=user:*

Invalidates by glob (`*` = any run, `?` = one char), or everything when
the pattern is omitted. Reports live entries killed:

```json
{"message":"invalidated","keyspace":"sess","entries":2}
```

### POST /api/v1/cache/mget · /api/v1/cache/mset

Batch reads return one row per requested key (`value: null` on miss):

```json
{"keyspace": "batch", "keys": ["a", "missing", "b"]}
```

```json
{"values":[{"key":"a","value":"v1"},{"key":"missing","value":null},{"key":"b","value":"v2"}]}
```

Batch writes take a flat pairs object plus an optional shared TTL:

```json
{"keyspace": "batch", "pairs": {"a": "\"v1\"", "b": "\"v2\""}, "ttl_ms": 5000}
```

---

## TCP Protocol

Connect to `127.0.0.1:7878`. Send one newline-terminated command, receive one newline-terminated response, connection closes.

```
PING                          → PONG
VERSION                       → DATA <encoded-version>
HEALTH                        → DATA <encoded-json>
HELP                          → DATA <encoded-help>
MODE GET                      → MODE <mode>
MODE SET <mode>               → OK MODE=<mode>
COMPILE <encoded-source>      → DATA <encoded-typescript>
SERVICES LIST                 → DATA <encoded-json>
ROUTES LIST                   → DATA <encoded-json>
AUTH STATUS                   → DATA <encoded-json>
AUTH SET <scheme> <token>     → OK token set
AUTH CLEAR                    → OK cleared
DB PUT <ns> <key> <value>     → OK stored
DB GET <ns> <key>             → DATA <value> | ERR not found
DB DEL <ns> <key>             → OK deleted
DB KEYS <ns>                  → DATA <encoded-json>
DB FLUSH <ns>                 → OK flushed
TRACE LIST [limit]            → DATA <encoded-json>
TRACE CLEAR                   → OK cleared
METRICS LIST                  → DATA <encoded-json>
METRICS CLEAR                 → OK cleared
PG CREATE <name>              → OK <message>
PG STATUS                     → DATA <status>
PG MIGRATE <encoded-sql>      → DATA <result>
PG DESTROY <name>             → OK <message>
REDIS STATUS                  → DATA <encoded-json>
REDIS PING                    → DATA PONG
REDIS FLUSH                   → OK flushed
```

Values containing spaces or special characters are percent-encoded. Use `protocol::encode()` / `protocol::decode()` from the `protocol` crate.
