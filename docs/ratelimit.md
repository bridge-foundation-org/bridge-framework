# Rate Limiting

Bridge implements per-endpoint token-bucket rate limiting. Rules can be configured via HTTP API or `bridge.toml` and take effect immediately.

## Token Bucket Algorithm

Each rule has a bucket with:

- **Capacity** — maximum burst size (tokens)
- **Refill rate** — tokens added per second (fractional allowed)

Each request consumes one token. If the bucket is empty the request gets a `429 Too Many Requests` response immediately. Tokens are refilled lazily on the next request based on elapsed time.

```
tokens = min(capacity, tokens + elapsed_seconds × refill_rate)

if tokens >= 1:  consume 1 token → allow request
else:            reject with 429, Retry-After = ceil((1 - tokens) / refill_rate)
```

## Wildcard Rules

Both `method` and `path` support `"*"` as a wildcard:

| method | path | Matches |
|--------|------|---------|
| `"GET"` | `"/ping"` | Only `GET /ping` |
| `"*"` | `"/ping"` | Any method on `/ping` |
| `"POST"` | `"*"` | Any `POST` request |
| `"*"` | `"*"` | Every request (global default) |

## Specificity Order

When multiple rules match a request, the **most specific** bucket is used:

```
1. Exact method + exact path          (highest priority)
2. Any method (*) + exact path
3. Exact method + any path (*)
4. Global wildcard (* + *)            (lowest priority)
```

This lets you set a permissive global default and tighten specific endpoints.

## Response Headers

These headers are injected on every request matched by a rate-limit rule:

| Header | Meaning |
|--------|---------|
| `X-RateLimit-Limit` | Bucket capacity |
| `X-RateLimit-Remaining` | Tokens left after this request |
| `X-RateLimit-Reset` | Unix timestamp when bucket will be full |
| `Retry-After` | Seconds to wait — **only on 429 responses** |

## HTTP API

### List rules

```
GET /api/v1/ratelimit
```

Response:
```json
[
  {"method":"POST","path":"/api/v1/compile","capacity":60,"refill_rate":1.0,"remaining":58},
  {"method":"*","path":"*","capacity":1000,"refill_rate":100.0,"remaining":999}
]
```

### Add rule

```
POST /api/v1/ratelimit
Content-Type: application/json

{
  "method":      "POST",
  "path":        "/api/v1/compile",
  "capacity":    60,
  "refill_rate": 1.0
}
```

Adding a rule with the same method+path replaces the existing bucket.

Response:
```json
{"message":"rate limit added","method":"POST","path":"/api/v1/compile","capacity":60,"refill_rate":1.0}
```

### Remove rule

```
DELETE /api/v1/ratelimit
Content-Type: application/json

{"method":"POST","path":"/api/v1/compile"}
```

Returns `404` if the rule was not found.

## curl Examples

```bash
# Global default: 1000 req burst, 100 req/s sustained
curl -X POST http://localhost:8787/api/v1/ratelimit \
  -H 'Content-Type: application/json' \
  -d '{"method":"*","path":"*","capacity":1000,"refill_rate":100}'

# Tight limit on compile endpoint: 60 burst, 1 req/s
curl -X POST http://localhost:8787/api/v1/ratelimit \
  -H 'Content-Type: application/json' \
  -d '{"method":"POST","path":"/api/v1/compile","capacity":60,"refill_rate":1.0}'

# Check what rules are active
curl http://localhost:8787/api/v1/ratelimit

# Remove the compile limit
curl -X DELETE http://localhost:8787/api/v1/ratelimit \
  -d '{"method":"POST","path":"/api/v1/compile"}'
```

## bridge.toml Configuration

```toml
[[ratelimit.rules]]
method      = "POST"
path        = "/api/v1/compile"
capacity    = 60
refill_rate = 1.0

[[ratelimit.rules]]
method      = "*"
path        = "*"
capacity    = 1000
refill_rate = 100.0
```

Rules are applied in order. If two rules have the same method+path the later one wins (it replaces the bucket).

Rules with `capacity = 0` are silently ignored.

## TypeScript Client Usage

```typescript
import { createDaemonClient } from "./daemon-client";

const client = createDaemonClient("http://localhost:8787");

// List rules
const rules = await client.rateLimitList();

// Add a rule
await client.rateLimitAdd({
  method:      "POST",
  path:        "/api/v1/compile",
  capacity:    60,
  refill_rate: 1.0,
});

// Remove a rule
await client.rateLimitRemove("POST", "/api/v1/compile");
```

## Common Patterns

### Protect the compile endpoint

```toml
[[ratelimit.rules]]
method      = "POST"
path        = "/api/v1/compile"
capacity    = 10
refill_rate = 0.2   # 1 compile per 5 seconds sustained
```

### Global API limit with a tighter per-endpoint rule

```toml
# Default: 500 req burst, 50 req/s
[[ratelimit.rules]]
method      = "*"
path        = "*"
capacity    = 500
refill_rate = 50.0

# Tighter: admin endpoint gets only 5 req/min
[[ratelimit.rules]]
method      = "*"
path        = "/admin"
capacity    = 5
refill_rate = 0.083
```

### Disable rate limiting

Simply do not add any `[[ratelimit.rules]]` entries. The rate limiter only activates for endpoints with an explicit rule.
