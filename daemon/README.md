# Bridge Daemon

The heart of the Bridge framework — a modular server that orchestrates compilation, codegen, database management, and caching.

## Overview

The daemon provides two interfaces:
- **TCP** — Line-oriented protocol for CLI
- **HTTP** — REST API for frontend dashboard

On startup:
1. Starts miniredis in a background thread
2. Launches HTTP server in a background thread
3. Runs TCP server on the main thread

## Architecture

```
                    ┌──────────────┐
                    │    main.rs   │
                    └───────┬──────┘
                            │
            ┌───────────────┼───────────────┐
            │               │               │
            ▼               ▼               ▼
    ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
    │    tcp.rs    │ │   http.rs    │ │  miniredis   │
    │ Line protocol│ │  REST API    │ │ Background   │
    └───────┬──────┘ └───────┬──────┘ └──────────────┘
            │                │
            └────────┬───────┘
                     │
            ┌────────▼─────────┐
            │     state.rs     │
            │ Arc<Mutex<State>>│
            │                  │
            │ - mode: String   │
            │ - store: Store   │
            │ - redis_info     │
            └────────┬─────────┘
                     │
            ┌────────▼─────────┐
            │    sqldb.rs      │
            │ Docker Postgres  │
            └──────────────────┘
```

## Modules

### main.rs

Entry point:
- Reads environment variables
- Starts miniredis
- Creates shared state
- Spawns HTTP thread
- Runs TCP server (blocking)

### state.rs

Shared state structure:

```rust
pub struct State {
    pub mode: String,
    pub store: Store,
    pub redis_addr: Option<String>,
    pub redis_connections: Option<Arc<AtomicUsize>>,
}
```

Wrapped in `Arc<Mutex<>>` for thread-safe access.

### tcp.rs

TCP server on port 7878:
- Accepts connections
- Reads line-delimited commands
- Parses with `protocol::parse_command`
- Dispatches to handlers
- Returns response

Handlers:
- `handle_ping()` → "PONG"
- `handle_compile(source)` → Calls compiler + codegen
- `handle_db_create(name)` → Delegates to sqldb module
- `handle_redis_status()` → Reads from state

### http.rs

HTTP REST API on port 8787:

**Routes:**

```
GET  /health          → "OK"
GET  /mode            → Current mode
POST /mode            → Set mode
POST /compile         → Compile source
GET  /db/latest       → Get latest codegen
POST /db/create       → Create Postgres container
GET  /db/status       → Check Docker status
POST /db/migrate      → Run SQL migration
DELETE /db/destroy    → Stop and remove container
GET  /redis/status    → Miniredis info (JSON)
```

Each route:
1. Parses request
2. Locks state (if needed)
3. Performs operation
4. Returns HTTP response

### sqldb.rs

Docker Postgres lifecycle management:

```rust
pub fn create_container(name: &str) -> Result<String, String>
pub fn container_status() -> Result<String, String>
pub fn migrate(sql: &str) -> Result<String, String>
pub fn destroy_container(name: &str) -> Result<String, String>
```

Uses `std::process::Command` to call `docker`:
- `docker run -d --name bridge_pg_<name> ...`
- `docker ps --filter name=bridge_pg_`
- `docker exec bridge_pg_<name> psql ...`
- `docker stop && docker rm`

Gracefully handles missing Docker.

## Configuration

Environment variables:

- `BRIDGE_TCP_ADDR` — TCP address (default: `127.0.0.1:7878`)
- `BRIDGE_HTTP_ADDR` — HTTP address (default: `127.0.0.1:8787`)
- `BRIDGE_REDIS_ADDR` — Miniredis address (default: `127.0.0.1:6399`)

Example:

```bash
export BRIDGE_TCP_ADDR=0.0.0.0:7878
export BRIDGE_HTTP_ADDR=0.0.0.0:8787
cargo run -p daemon
```

## Threading Model

The daemon uses basic threading:

- **Main thread** — TCP server (blocking accept loop)
- **HTTP thread** — HTTP server (blocking accept loop)
- **Miniredis thread** — Started by miniredis crate

State is shared via `Arc<Mutex<State>>`. Each thread locks the mutex when accessing state.

### Why no async?

- **Simplicity** — No tokio, no complex runtimes
- **Clarity** — Easy to understand for contributors
- **Performance** — Good enough for local development
- **Portability** — Works anywhere Rust compiles

For production, consider adding async with tokio or smol.

## Protocol Details

### TCP Protocol

Line-oriented:

```
<COMMAND> <ARGS...>\n
```

Examples:

```
PING
COMPILE service%20hello%0Aendpoint%20ping%20GET%20/ping
DB CREATE mydb
MODE SET full
REDIS STATUS
```

Responses:

```
PONG\n
DATA <url-encoded>\n
OK <message>\n
ERR <error>\n
MODE <mode>\n
```

### HTTP API

All endpoints return plain text or JSON.

#### GET /health

```http
GET /health HTTP/1.1

200 OK
OK
```

#### POST /compile

```http
POST /compile HTTP/1.1
Content-Length: 52

service hello
endpoint ping GET /ping

200 OK
<generated TypeScript code>
```

#### POST /db/create

```http
POST /db/create HTTP/1.1
Content-Length: 5

mydb

200 OK
Container created: bridge_pg_mydb
```

#### GET /redis/status

```http
GET /redis/status HTTP/1.1

200 OK
{"addr":"127.0.0.1:6399","connections":3}
```

## Database Management

The daemon manages PostgreSQL via Docker containers:

### Creating a Database

```rust
sqldb::create_container("mydb")?;
```

Runs:
```bash
docker run -d \
  --name bridge_pg_mydb \
  -e POSTGRES_PASSWORD=bridge \
  -p 5432:5432 \
  postgres:16
```

### Running Migrations

```rust
sqldb::migrate("CREATE TABLE users (id SERIAL, name TEXT);")?;
```

Runs:
```bash
docker exec bridge_pg_mydb psql \
  -U postgres \
  -c "CREATE TABLE users (id SERIAL, name TEXT);"
```

### Checking Status

```rust
let status = sqldb::container_status()?;
```

Parses output of:
```bash
docker ps --filter name=bridge_pg_ --format json
```

### Destroying

```rust
sqldb::destroy_container("mydb")?;
```

Runs:
```bash
docker stop bridge_pg_mydb
docker rm bridge_pg_mydb
```

## Miniredis Integration

On startup, the daemon starts the miniredis server:

```rust
let (redis_info, conn_count) = match miniredis::MiniRedis::start(&redis_addr) {
    Ok((server, _handle)) => {
        eprintln!("miniredis started on {}", server.addr);
        (Some(server.addr.to_string()), Some(Arc::clone(&server.connection_count)))
    }
    Err(e) => {
        eprintln!("miniredis failed to start: {e}");
        (None, None)
    }
};
```

The Redis info is stored in state for status queries.

## Error Handling

The daemon uses `Result<T, String>` for all operations:

```rust
fn handle_compile(source: &str, state: Arc<Mutex<State>>) -> String {
    match compiler::parse(source) {
        Ok(ast) => {
            match codegen::generate_typescript(&ast) {
                Ok(code) => {
                    state.lock().unwrap().store.put("codegen", "latest", &code);
                    format!("DATA {}", protocol::escape(&code))
                }
                Err(e) => format!("ERR codegen failed: {e}")
            }
        }
        Err(e) => format!("ERR parse failed: {e}")
    }
}
```

Errors are returned to clients as:
- TCP: `ERR <message>\n`
- HTTP: Plain text with appropriate status code

## Adding New Features

### Adding a TCP Command

1. Add to `protocol` crate:
```rust
pub enum Command {
    // ...
    NewCommand { arg: String },
}
```

2. Update `protocol::parse_command()`

3. Add handler in `tcp.rs`:
```rust
Command::NewCommand { arg } => handle_new_command(&arg, state),
```

4. Implement handler:
```rust
fn handle_new_command(arg: &str, state: Arc<Mutex<State>>) -> String {
    // Implementation
    protocol::render_response(Response::Ok("done".to_string()))
}
```

### Adding an HTTP Endpoint

In `http.rs`, add to the request dispatcher:

```rust
("GET", "/new-endpoint") => {
    let state = state.lock().unwrap();
    // Implementation
    response("200 OK", "result")
}
```

## Testing

```bash
# Unit tests
cargo test -p daemon

# Integration tests (requires built binary)
cargo build --release
cargo test -p e2e-tests

# Manual testing
cargo run -p daemon &
curl http://localhost:8787/health
echo "PING" | nc localhost 7878
```

## Performance Considerations

### Bottlenecks

- State mutex contention (not an issue for local dev)
- Blocking I/O in HTTP/TCP handlers
- Docker CLI spawning overhead

### Improvements

For production:
- Use async I/O (tokio)
- Connection pooling for Docker API
- Rate limiting
- Request queuing

For local dev, current performance is excellent.

## Security

### Current State

- No authentication
- No rate limiting
- Trusts all input
- Docker runs as host user

**Not suitable for production as-is.**

### Improvements Needed

- API keys for HTTP endpoints
- Input validation and sanitization
- Docker user namespaces
- Resource limits
- TLS support

## Debugging

Enable verbose output:

```bash
RUST_LOG=debug cargo run -p daemon
```

Use the raw CLI command:

```bash
bridge raw "DB STATUS"
```

Inspect state:

```bash
curl http://localhost:8787/mode
curl http://localhost:8787/db/latest
curl http://localhost:8787/redis/status
```

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md).

Common improvements:
- Better error messages
- Structured logging
- Metrics and monitoring
- Health check endpoints with details
- Graceful shutdown

## Dependencies

- Rust `std` library
- `protocol` crate (workspace)
- `compiler` crate (workspace)
- `codegen` crate (workspace)
- `db` crate (workspace)
- `miniredis` crate (workspace)

## License

MIT — see [LICENSE](../LICENSE).
