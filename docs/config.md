# bridge.toml

`bridge.toml` is the project configuration file for Bridge Framework. It controls daemon addresses, hot-reload settings, middleware, and rate limiting.

## Location and Loading

Place `bridge.toml` in your project root. The daemon looks for it in this order:

1. Path specified by `BRIDGE_CONFIG` environment variable
2. `./bridge.toml` in the current working directory
3. No config — all defaults apply

```bash
# Default: reads ./bridge.toml if present
cargo run -p daemon

# Custom path
BRIDGE_CONFIG=/etc/bridge/prod.toml cargo run -p daemon
```

## Full Example

```toml
# bridge.toml — Bridge Framework project configuration

[project]
name    = "my-app"
version = "0.1.0"

[daemon]
http_addr  = "127.0.0.1:8787"   # HTTP REST API
tcp_addr   = "127.0.0.1:7878"   # TCP line-protocol server
redis_addr = "127.0.0.1:6399"   # Embedded miniredis
mode       = "full"             # lite | full | ultra | off

[watch]
enabled = true
poll_ms = 500
dirs    = ["."]
files   = ["app.bridge"]

[[middleware.rules]]
name   = "powered-by"
scope  = "global"
after  = "header:X-Powered-By:bridge"

[[middleware.rules]]
name   = "api-logger"
scope  = "service:api"
before = "log"

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

## Section Reference

### [project]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | `""` | Project name — shown in health endpoint and logs |
| `version` | string | `""` | Project version |

### [daemon]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `http_addr` | string | `"127.0.0.1:8787"` | HTTP server bind address |
| `tcp_addr` | string | `"127.0.0.1:7878"` | TCP server bind address |
| `redis_addr` | string | `"127.0.0.1:6399"` | Miniredis bind address |
| `mode` | string | `"full"` | `lite` \| `full` \| `ultra` \| `off` |

Environment variables `BRIDGE_HTTP_ADDR`, `BRIDGE_TCP_ADDR`, `BRIDGE_REDIS_ADDR` take precedence over `bridge.toml` values.

### [watch]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Start the hot-reload background thread |
| `poll_ms` | integer | `500` | File poll interval (clamped to minimum 100ms) |
| `dirs` | string array | `[]` | Directories to scan for `.bridge` files |
| `files` | string array | `[]` | Explicit `.bridge` file paths to watch |

### [[middleware.rules]]

Each `[[middleware.rules]]` entry registers one middleware. See [Middleware](middleware.md) for full documentation.

| Key | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | string | yes | Unique middleware name |
| `scope` | string | no (default `"global"`) | `global` \| `service:NAME` \| `METHOD:/path` |
| `before` | string | no | Before hook spec: `log` \| `reject:STATUS:MSG` |
| `after` | string | no | After hook spec: `log` \| `header:KEY:VALUE` |

Entries with an empty `name` are silently skipped.

### [[ratelimit.rules]]

Each `[[ratelimit.rules]]` entry creates one token-bucket rule. See [Rate Limiting](ratelimit.md) for full documentation.

| Key | Type | Required | Description |
|-----|------|----------|-------------|
| `method` | string | no (default `"*"`) | HTTP method or `*` |
| `path` | string | no (default `"*"`) | Exact path or `*` |
| `capacity` | integer | yes | Maximum burst tokens (must be > 0) |
| `refill_rate` | float | yes | Tokens refilled per second |

Entries with `capacity = 0` are silently skipped.

## How bridge init Generates bridge.toml

Running `bridge init <dir>` creates a new project directory with:

```
<dir>/
├── app.bridge       # Sample Bridge DSL
├── bridge.toml      # Project config with sensible defaults
└── README.md
```

The generated `bridge.toml` includes a `powered-by` after hook and a compile endpoint rate limit.

## GET /api/v1/config

The daemon exposes a read-only summary of its current effective configuration:

```bash
curl http://localhost:8787/api/v1/config
```

```json
{
  "app":     "my-app",
  "version": "0.2.0",
  "mode":    "full",
  "middleware": ["powered-by", "api-logger"],
  "ratelimit": [
    {"method":"POST","path":"/api/v1/compile","capacity":60,"refill_rate":1.0,"remaining":60}
  ],
  "watch": {
    "enabled": true,
    "poll_ms":  500,
    "files": ["app.bridge"]
  }
}
```

This reflects the **live runtime state**, not necessarily what is in `bridge.toml` — changes made via HTTP API (registering middleware, adding rate-limit rules) are included.

## Validation

The parser returns an error for:

- Unknown keys in `[project]`, `[daemon]`, `[watch]`, `[[middleware.rules]]`, or `[[ratelimit.rules]]`
- Non-boolean value for `enabled` (must be `true` or `false`)
- Non-integer value for `poll_ms` or `capacity`
- Non-float value for `refill_rate`

Unknown top-level sections (e.g., `[custom]`) are silently ignored so you can add your own metadata without breaking the parse.

## Inline Comments

TOML inline comments (after `#`) are supported for unquoted values:

```toml
mode    = "full"   # lite | full | ultra | off
poll_ms = 500      # milliseconds
```

Comments inside quoted strings are preserved as part of the string value.
