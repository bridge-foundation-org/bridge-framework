# e2e-tests — Integration Test Suite

End-to-end and integration tests for the Bridge Framework.

## Overview

This crate contains two categories of tests:

| Category | Requires daemon? | Count | Command |
|----------|-----------------|-------|---------|
| **Unit integration** | No | 28 | `cargo test -p e2e-tests` |
| **Daemon integration** | Yes | 36 | `cargo test -p e2e-tests -- --include-ignored` |

Unit integration tests verify the compiler → codegen → protocol pipeline without any running process. Daemon tests exercise the full TCP + HTTP stack against a live daemon.

## Running Tests

### Unit integration (no setup required)

```bash
cargo test -p e2e-tests
```

All 28 tests pass without any external dependencies.

### Daemon integration tests

1. Build the daemon:
   ```bash
   cargo build -p daemon
   ```

2. Start it with the test addresses:
   ```bash
   $env:BRIDGE_TCP_ADDR="127.0.0.1:17878"
   $env:BRIDGE_HTTP_ADDR="127.0.0.1:18787"
   $env:BRIDGE_REDIS_ADDR="127.0.0.1:16399"
   cargo run -p daemon
   ```

3. In a second terminal, run all tests including ignored:
   ```bash
   cargo test -p e2e-tests -- --include-ignored
   ```

### Full workspace tests

```bash
cargo test --workspace
# 370 tests total, 36 daemon tests ignored
```

## Test Coverage

### Unit Integration (`mod unit`)

**Compiler → Codegen pipeline:**
- `compile_and_generate_single_service` — single service, path params, BridgeError class
- `compile_and_generate_multi_service` — multi-service, auth inheritance, root factory
- `compile_path_params_codegen` — multi-segment path params (`:x/:y`)
- `openapi_path_params_converted` — `:id` → `{id}` in OpenAPI
- `openapi_bearer_security` — security scheme generation
- `openapi_api_key_security` — API key security scheme

**Protocol encode/decode:**
- `protocol_encode_decode` — round-trip for special characters
- `protocol_parse_ping` — PING command parsing
- `protocol_parse_db_create` — DB_CREATE with name
- `protocol_parse_mode_set` — MODE_SET with valid/invalid modes
- `protocol_response_prefixes` — DATA/ERR/OK prefix detection

**Compiler validation:**
- `compiler_rejects_unknown_method` — bad HTTP method → error
- `compiler_rejects_bad_path` — path without `/` → error
- `compiler_rejects_duplicate_service` — duplicate names → error
- `compiler_rejects_duplicate_endpoint` — duplicate endpoint in service
- `compiler_rejects_endpoint_outside_service` — orphaned endpoint
- `compiler_rejects_route_conflict` — same method+path → error

**Miniredis RESP:**
- `resp_serialize_simple_string` — `+OK\r\n`
- `resp_serialize_error` — `-ERR ...\r\n`
- `resp_serialize_integer` — `:42\r\n`
- `resp_serialize_bulk_string` — `$5\r\nhello\r\n`
- `resp_serialize_null_bulk` — `$-1\r\n`
- `resp_serialize_array` — nested RESP array
- `resp_parse_round_trip` — parse serialized output
- `resp_parse_inline_command` — space-separated inline format
- `resp_null_bulk_string` — null bulk string parsing

### Daemon Integration (`mod daemon`) — requires live daemon

**TCP protocol:**
- `tcp_ping` — PING / PONG
- `tcp_version` — VERSION returns semver string
- `tcp_mode_get` — MODE_GET returns current mode
- `tcp_mode_set_and_get` — round-trip mode change
- `tcp_compile_simple` — inline source compilation
- `tcp_compile_file_not_found` — missing file → ERR

**HTTP API:**
- `http_health` — GET `/health`
- `http_ping` — GET `/api/v1/ping`
- `http_status` — GET `/api/v1/status`
- `http_compile` — POST `/api/v1/compile`
- `http_auth_set` — POST `/api/v1/auth/set`
- `http_middleware_crud` — GET/POST/DELETE `/api/v1/middleware`
- `http_ratelimit_crud` — GET/POST/DELETE `/api/v1/ratelimit`
- `http_metrics_prometheus` — GET `/api/v1/metrics/prometheus`
- `http_watch_files` — GET/POST/DELETE `/api/v1/watch/files`
- `http_config` — GET `/api/v1/config`

**Database / Redis:**
- `tcp_db_create_and_status` — DB_CREATE + DB_STATUS
- `tcp_redis_status` — REDIS_STATUS
- `tcp_redis_set_get` — REDIS_SET + REDIS_GET round-trip
- `tcp_redis_del` — REDIS_DEL removes key
- `tcp_redis_keys_pattern` — REDIS_KEYS with wildcard

## Helper Utilities

### `TcpHelper`

Sends a raw TCP command and reads the response:

```rust
let mut conn = TcpHelper::connect("127.0.0.1:17878")?;
let resp = conn.send("PING")?;
assert_eq!(resp.trim(), "PONG");
```

### `HttpHelper`

Makes HTTP requests to the daemon:

```rust
let h = HttpHelper::new("http://127.0.0.1:18787");
let status = h.get("/health")?;
assert_eq!(status, 200);

let (status, body) = h.post_json("/api/v1/compile", r#"{"source":"service s\nendpoint e GET /e\n"}"#)?;
assert_eq!(status, 200);
assert!(body.contains("createSClient"));
```

## Adding Tests

### Unit test (no daemon)

```rust
#[test]
fn my_compiler_test() {
    use compiler::parse;
    let file = parse("service s\nendpoint e GET /e\n").unwrap();
    assert_eq!(file.services[0].name, "s");
}
```

### Daemon test

```rust
#[test]
#[ignore]  // requires running daemon
fn my_daemon_test() {
    let mut conn = TcpHelper::connect("127.0.0.1:17878").unwrap();
    let resp = conn.send("PING").unwrap();
    assert_eq!(resp.trim(), "PONG");
}
```

The `#[ignore]` attribute makes the test skip by default but run when `--include-ignored` is passed.

## CI Integration

The CI pipeline runs unit integration tests on every push:

```yaml
- name: Run unit tests
  run: cargo test --workspace
```

Daemon tests run separately in an integration environment:

```yaml
- name: Run daemon tests
  run: |
    cargo build -p daemon
    cargo run -p daemon &
    sleep 2
    cargo test -p e2e-tests -- --include-ignored
```
