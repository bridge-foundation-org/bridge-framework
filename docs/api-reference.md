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
| GET | `/api/v1/secrets` | List registered secrets (no plaintext) |
| POST | `/api/v1/secrets/set` | Register secret (inline/env/file/vault) |
| POST | `/api/v1/secrets/get` | Display value (redacted unless reveal) |
| POST | `/api/v1/secrets/check` | Verify named secrets resolve (409 on missing) |
| DELETE | `/api/v1/secrets/:name` | Remove secret from registry |
| GET | `/api/v1/infra` | Full infra snapshot (env/services/databases/tls) |
| POST | `/api/v1/infra/env` | Set env var (empty value removes) |
| DELETE | `/api/v1/infra/env` | Clear all env vars |
| POST | `/api/v1/infra/services` | Register/replace service endpoint |
| GET | `/api/v1/infra/services` | List discovered services |
| POST | `/api/v1/infra/databases` | Upsert database config (validated) |
| GET | `/api/v1/infra/databases` | List database configs |
| POST | `/api/v1/infra/tls` | Set gateway TLS status |
| GET | `/api/v1/testing` | Test harness snapshot (mode/databases/mocks) |
| POST | `/api/v1/testing/mode/enter` | Enter test mode (default quiet logs) |
| POST | `/api/v1/testing/mode/exit` | Exit test mode (404 if not active) |
| POST | `/api/v1/testing/databases` | Provision isolated test database |
| DELETE | `/api/v1/testing/databases` | Destroy all test databases |
| POST | `/api/v1/testing/mocks/auth` | Mock auth with canned principal |
| POST | `/api/v1/testing/mocks/services` | Register canned service response |
| DELETE | `/api/v1/testing/mocks` | Clear all mocks |
| GET | `/api/v1/deploy` | List deployments |
| POST | `/api/v1/deploy` | Create deployment (validated platform/revision) |
| POST | `/api/v1/deploy/status` | Advance status (state-machine enforced) |
| POST | `/api/v1/deploy/rollback` | Roll target back to superseded revision |
| GET | `/api/v1/deploy/dockerfile` | Generated multi-platform Dockerfile |
| POST | `/api/v1/mcp` | MCP JSON-RPC 2.0 endpoint (tools/list, tools/call) |
| GET | `/api/v1/ws` | WebSocket room catalog (members per room) |
| POST | `/api/v1/ws/handshake` | Validate upgrade request, return 101 response |
| POST | `/api/v1/ws/join` | Join a connection to a room (409 on duplicate) |
| POST | `/api/v1/ws/leave` | Leave a room (404 when not a member) |
| POST | `/api/v1/ws/broadcast` | Fan-out: returns recipient list for a room |

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

## Secrets

Secret registry backing Encore-style `secrets.get()`. Values resolve
lazily from four source kinds; plaintext never appears in listings and is
revealed only by explicit opt-in.

### POST /api/v1/secrets/set

```json
{"name": "db_pw", "source": {"kind": "inline", "value": "hunter2"}}
```

Source kinds:

- `{"kind":"inline","value":"..."}` — literal value (dev/test)
- `{"kind":"env","env_var":"DB_PW"}` — resolved from the environment at read time
- `{"kind":"file","path":"/run/secrets/pw"}` — file read lazily, trimmed
- `{"kind":"vault","provider":"hashicorp","path":"secret/app"}` — external vault stub (falls back to `NAME_UPPERCASED` env var locally)

Response: `{"message":"secret set","name":"db_pw","redacted":true}`.

### POST /api/v1/secrets/get

Redacted by default:

```json
{"name": "db_pw"}          → {"name":"db_pw","value":"***"}
{"name": "db_pw", "reveal": true} → {"name":"db_pw","value":"hunter2"}
```

A registered-but-unresolvable secret (missing env var / unreadable file)
returns `409 {"error":"secret not resolvable"}` on reveal. Unknown names →
`404`.

### POST /api/v1/secrets/check

```json
{"names": ["db_pw", "stripe_key"]}
```

All resolve → `200 {"ok":true,...}`. Any missing →
`409 {"ok":false,"missing":["stripe_key"],"results":[...]}`.

---

## Infra Config

Runtime infrastructure configuration (Encore `infra.Config` parity): env
vars, discovered services, database configs, and gateway TLS status. All
state lives in the daemon snapshot; `GET /api/v1/infra` returns it in full.

### GET /api/v1/infra

```json
{"env_vars":{"LOG_LEVEL":"debug"},"services":[],"databases":[],"tls":{"configured":false}}
```

Env vars are always sorted; empty value on set removes the var.

### POST /api/v1/infra/env

```json
{"name": "LOG_LEVEL", "value": "debug"}   → 200 {"message":"env updated"}
{"name": "LOG_LEVEL", "value": ""}        → removes the var
```

### POST /api/v1/infra/services

Register or replace a service endpoint:

```json
{"name": "auth", "addr": "127.0.0.1:9001"} → 200 {"message":"service registered","name":"auth"}
```

Re-registering an existing name updates its addr in place. Empty name/addr → `400`.

### GET /api/v1/infra/services

`{"services":[{"name":"auth","addr":"127.0.0.1:9001"}]}`

### POST /api/v1/infra/databases

```json
{"name": "main", "engine": "postgres", "host": "localhost", "port": 5432}
→ 200 {"message":"database configured","name":"main","engine":"postgres"}
```

Validates engine ∈ {postgres, mysql, sqlite} and port 1-65535; violations → `400`.

### GET /api/v1/infra/databases

`{"databases":[{"name":"main","engine":"postgres","host":"localhost","port":5432}]}`

### POST /api/v1/infra/tls

```json
{"enabled": true, "cert_path": "/certs/a.pem"}
```

Snapshot then reports `"tls":{"enabled":true,"cert":"/certs/a.pem"}`.
Before any TLS update: `"tls":{"configured":false}`. Empty `cert_path`
records enabled-with-no-cert.

---

## Testing

Test harness support (Encore `testing` parity): isolated test databases,
test mode with quiet default logs, and auth/service mocking. All state is
daemon-side; `GET /api/v1/testing` returns the full snapshot.

### GET /api/v1/testing

```json
{"mode":{"active":false},"databases":[],"mocks":{"auth":{"enabled":false},"services":{}}}
```

### POST /api/v1/testing/mode/enter / POST /api/v1/testing/mode/exit

```json
{"log_level": "warn"}
```

Unknown/empty levels fall back to `error` (quiet-by-default). Exit when
not active → `404`.

### POST /api/v1/testing/databases

```json
{"name": "users", "superuser": true}
→ 200 {"namespace":"t1_users","superuser":true}
```

Each instance gets a unique namespace (`t{seq}_{name}`) so same-name
tests never collide. `superuser` maps to Encore's migrator/superuser
test roles.

### DELETE /api/v1/testing/databases

Destroys every live test database:
`200 {"message":"cleaned up","destroyed":2}`.

### POST /api/v1/testing/mocks/auth

```json
{"principal": "u_123"} → 200 {"message":"auth mocked","principal":"u_123"}
```

While set, auth checks pass as this principal (Encore commit 1737/1819).
Blank principal → `400`.

### POST /api/v1/testing/mocks/services

```json
{"service": "auth", "response": {"user": "u_1"}}
```

Response body is stored verbatim and echoed in the snapshot under
`mocks.services`. Missing service name → `400`.

### DELETE /api/v1/testing/mocks

Clears all mocks: `200 {"message":"mocks cleared","count":2}`.

---

## Deployments

Deployment tracking (Encore CLI deploy parity): create deployments per
named target, drive them through an enforced lifecycle, roll back to the
exact revision that was superseded, and generate the build Dockerfile.

### GET /api/v1/deploy

```json
{"deployments":[{"id":"dep-1","target":"production","platform":"linux/arm64","revision":"abc123","status":"queued"}]}
```

### POST /api/v1/deploy

```json
{"target": "production", "platform": "linux/arm64", "revision": "abc123"}
→ 200 {"id":"dep-1","status":"queued","platform":"linux/arm64"}
```

Platform must be `os/arch[/variant]` (defaults `linux/amd64`); empty
target/revision → `400`.

### POST /api/v1/deploy/status

```json
{"id": "dep-1", "status": "building"}
```

Legal transitions only: `queued → building → deploying → deployed`, any
mid-flight state may go `failed`; terminal states are final. Illegal
moves or unknown status strings → `400`. When a deployment goes live,
any prior live deployment on the same target is demoted to `failed`
with `superseded_by` recording who replaced it.

### POST /api/v1/deploy/rollback

```json
{"target": "production"}
→ 200 {"message":"rolled back","id":"dep-1","revision":"v1","status":"deployed"}
```

Promotes exactly the revision the current live deployment displaced
(ping-pong works). No live deployment or no predecessor → `404`;
missing target → `400`.

### GET /api/v1/deploy/dockerfile

Returns the generated multi-stage Dockerfile as a JSON string:
platform-aware (`BUILDPLATFORM`/`TARGETPLATFORM` for buildx) with a
manifest-first dependency layer for cache hits (Encore 2083/2188).

---

## MCP

Model Context Protocol endpoint (Encore commit 1828 parity): exposes
the daemon control plane as JSON-RPC 2.0 tools an AI agent can list
and invoke. See [llm-instructions](./llm-instructions.md) for the
agent-side contract.

### POST /api/v1/mcp

```json
{"method": "tools/list"}
```

→ catalog of 14 tools (`compile`, `services_list`, `traces_list`,
`secrets_set`, `cache_write`, `publish_event`, `test_db_create`,
`mock_auth`, `deploy_create`, ...) each with input-schema hints.

```json
{"method": "tools/call", "params": {"name": "infra_snapshot"}}
{"method": "tools/call", "params": {"name": "secrets_set", "body": "{\"name\":\"k\",\"source\":{\"kind\":\"inline\",\"value\":\"v\"}}"}}
```

Tool calls dispatch through the real HTTP router; the response is
returned as content text with `isError:true` when status ≥ 400.
Protocol errors use JSON-RPC codes: `-32601` unknown method, `-32602`
unknown tool. `initialize` and `ping` support handshakes/reconnect.

---

## WebSockets

RFC 6455 support (Encore commits 1434-1445, 1565 parity): handshake
validation, frame codec primitives, and a room hub for
service-to-service fan-out.

### GET /api/v1/ws

```json
{"rooms":[{"room":"chat","members":["ws000001","ws000002"]}],"count":1}
```

### POST /api/v1/ws/handshake

Body: a raw HTTP upgrade request. Validates `Upgrade: websocket`,
`Connection: Upgrade`, and `Sec-WebSocket-Key`, returning the exact
`101 Switching Protocols` response to write back (accept value computed
via SHA-1 + base64 per RFC 6455 §1.3). Invalid upgrades → `400`.

### POST /api/v1/ws/join · POST /api/v1/ws/leave

```json
{"conn": "ws000001", "room": "chat"}
```

Duplicate join → `409`; leave when not a member → `404`. Empty rooms
are pruned automatically; disconnecting removes a conn from all rooms.

### POST /api/v1/ws/broadcast

```json
{"room": "chat", "sender": "ws000001", "message": {"text": "hi"}}
→ 200 {"room":"chat","recipients":["ws000002"],"count":1,"message":{"text":"hi"}}
```

Returns the recipients the caller fans out to (everyone except the
sender). The message is echoed verbatim for auditability.

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
