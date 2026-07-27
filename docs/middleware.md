# Middleware

Bridge middleware provides composable before/after hooks that run around every HTTP request. Hooks can log, reject, or inject response headers.

## Architecture

```
request
  │
  ▼
[before₁] → [before₂] → ... → handler → [after₂] → [after₁]
  │                                                       │
  └── any hook can call ctx.reject() to short-circuit ────┘
                                                       │
                                                    response
```

Before hooks run in registration order. After hooks run in reverse order (outermost wraps first).

## Scopes

A middleware can target different parts of the application:

| Scope | Format | Matches |
|-------|--------|---------|
| Global | `"global"` | Every request |
| Service | `"service:NAME"` | All paths starting with `/NAME` |
| Endpoint | `"METHOD:/path"` | One exact method + path |

Examples:
```
"global"            → all requests
"service:users"     → /users, /users/create, /users/123
"GET:/api/v1/health"→ only GET /api/v1/health
"POST:*"            → not valid; use global with method check
```

## Hook Specs

Built-in specs are short strings that describe what a hook does:

**Before hooks:**

| Spec | Effect |
|------|--------|
| `"log"` | Tags the context with `"logged"` (visible in traces) |
| `"reject:STATUS:MSG"` | Returns HTTP `STATUS` with `{"error":"MSG"}` immediately |

**After hooks:**

| Spec | Effect |
|------|--------|
| `"log"` | Tags the context with `"logged-after"` |
| `"header:KEY:VALUE"` | Injects `KEY: VALUE` response header |

## HTTP API

### List middleware

```
GET /api/v1/middleware
```

Response:
```json
[
  {"name":"logger","scope":"global","before":true,"after":false},
  {"name":"cors-tag","scope":"global","before":false,"after":true}
]
```

### Register middleware

```
POST /api/v1/middleware
Content-Type: application/json

{
  "name":   "my-logger",
  "scope":  "global",
  "before": "log",
  "after":  "header:X-Powered-By:bridge"
}
```

Fields `before` and `after` are optional. Registering with an existing name replaces it.

Response:
```json
{"message":"middleware registered","name":"my-logger","index":0}
```

### Remove middleware

```
DELETE /api/v1/middleware
Content-Type: application/json

{"name":"my-logger"}
```

Response:
```json
{"message":"middleware removed","name":"my-logger"}
```

Returns `404` if the name was not found.

## curl Examples

```bash
# Register a logger
curl -X POST http://localhost:8787/api/v1/middleware \
  -H 'Content-Type: application/json' \
  -d '{"name":"logger","scope":"global","before":"log"}'

# Inject a header after every response
curl -X POST http://localhost:8787/api/v1/middleware \
  -H 'Content-Type: application/json' \
  -d '{"name":"powered-by","scope":"global","after":"header:X-Powered-By:bridge"}'

# Block all access to /admin with 403
curl -X POST http://localhost:8787/api/v1/middleware \
  -H 'Content-Type: application/json' \
  -d '{"name":"admin-guard","scope":"GET:/admin","before":"reject:403:forbidden"}'

# List registered middleware
curl http://localhost:8787/api/v1/middleware

# Remove
curl -X DELETE http://localhost:8787/api/v1/middleware \
  -d '{"name":"admin-guard"}'
```

## bridge.toml Configuration

```toml
[[middleware.rules]]
name   = "powered-by"
scope  = "global"
after  = "header:X-Powered-By:bridge"

[[middleware.rules]]
name   = "api-logger"
scope  = "service:api"
before = "log"

[[middleware.rules]]
name   = "health-block"
scope  = "GET:/api/v1/health"
before = "log"
after  = "header:X-Endpoint-Type:health"
```

All `[[middleware.rules]]` entries are applied in order at daemon startup via `config::apply()`.

## TypeScript Client Usage

```typescript
import { createDaemonClient } from "./daemon-client";

const client = createDaemonClient("http://localhost:8787");

// List
const list = await client.middlewareList();
console.log(list);

// Register
await client.middlewareRegister({
  name:   "my-hook",
  scope:  "global",
  before: "log",
  after:  "header:X-Bridge-Version:1",
});

// Remove
await client.middlewareRemove("my-hook");
```

## Interaction with Auth and Rate Limiting

The middleware chain runs **after** auth enforcement and **after** rate limiting. The execution order per request is:

```
1. Auth check (configured token, if any)
2. Rate-limit check (token bucket)
3. Middleware before hooks (in registration order)
4. Handler
5. Middleware after hooks (in reverse order)
6. Response sent
```

This means a `reject:401` middleware cannot bypass a configured auth token — the auth check happens first. Rate limit `429` responses also bypass the middleware chain entirely.
