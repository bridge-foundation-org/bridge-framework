# Development Guide

Complete guide for developing Bridge Framework.

## Prerequisites

- **Rust** 1.70+ ([rustup.rs](https://rustup.rs/))
- **Node.js** 18+ ([nodejs.org](https://nodejs.org/))
- **Docker** (optional, for database features)
- **Git**

### Recommended Tools

- **VS Code** with rust-analyzer extension
- **redis-cli** for testing miniredis
- **psql** for database management
- **curl** or **Postman** for HTTP testing

## Quick Setup

```bash
# Clone repository
git clone https://github.com/yourusername/bridge-framework.git
cd bridge-framework

# Build all crates
cargo build --workspace

# Install CLI globally
cargo install --path cli

# Install frontend dependencies
cd frontend && npm install && cd ..

# Copy environment template
cp .env.example .env
```

## Development Workflow

### Option 1: Manual (Full Control)

```bash
# Terminal 1: Start daemon
cargo run -p daemon

# Terminal 2: Start frontend
cd frontend && npm run dev

# Terminal 3: Use CLI
bridge ping
bridge compile-file examples/hello.bridge
```

### Option 2: Automated (Using Scripts)

```bash
# Start daemon + frontend together
npm run dev:all

# In another terminal
bridge ping
```

### Option 3: Docker Compose (Full Infrastructure)

```bash
# Start Postgres + Redis + pgAdmin
docker-compose up -d

# Start daemon (connects to external services)
export DATABASE_URL=postgres://bridge:bridge@localhost:5432/bridge_dev
export REDIS_URL=redis://localhost:6379
cargo run -p daemon

# Start frontend
cd frontend && npm run dev
```

## Project Structure

```
bridge-framework/
├── cli/              # Command-line tool
│   ├── src/
│   │   └── main.rs   # Entry point, command parsing
│   ├── Cargo.toml
│   └── README.md
├── daemon/           # Backend server
│   ├── src/
│   │   ├── main.rs   # Entry point, startup orchestration
│   │   ├── state.rs  # Shared state (Arc<Mutex<State>>)
│   │   ├── tcp.rs    # TCP protocol server
│   │   ├── http.rs   # HTTP REST API server
│   │   └── sqldb.rs  # Docker Postgres management
│   ├── Cargo.toml
│   └── README.md
├── protocol/         # Shared protocol definitions
│   ├── src/
│   │   └── lib.rs    # Command/Response types, parsing
│   ├── Cargo.toml
│   └── README.md
├── compiler/         # Bridge DSL parser
│   ├── src/
│   │   └── lib.rs    # Lexer, parser, AST
│   ├── Cargo.toml
│   └── README.md
├── codegen/          # TypeScript client generator
│   ├── src/
│   │   └── lib.rs    # AST → TypeScript transformation
│   ├── Cargo.toml
│   └── README.md
├── db/               # In-memory key-value store
│   ├── src/
│   │   └── lib.rs    # Thread-safe storage abstraction
│   ├── Cargo.toml
│   └── README.md
├── miniredis/        # Embedded Redis server
│   ├── src/
│   │   ├── lib.rs    # Public API, TCP listener
│   │   ├── resp.rs   # RESP protocol parser/serializer
│   │   ├── store.rs  # In-memory storage with TTL
│   │   ├── commands.rs   # Command handlers
│   │   └── dispatch.rs   # Command routing
│   ├── Cargo.toml
│   └── README.md
├── e2e-tests/        # Integration tests
│   ├── src/
│   │   └── lib.rs    # End-to-end test suite
│   ├── Cargo.toml
│   └── README.md
├── frontend/         # Dev dashboard
│   ├── src/
│   │   ├── main.ts   # App shell, routing, views
│   │   ├── daemon-client.ts   # HTTP client
│   │   ├── docs.ts   # Documentation rendering
│   │   └── style.css # Tailwind + custom styles
│   ├── package.json
│   ├── vite.config.ts
│   └── tsconfig.json
└── docs/             # Documentation
    ├── index.md
    ├── install.md
    ├── architecture.md
    ├── database.md
    ├── caching.md
    ├── deployment.md
    ├── api-reference.md
    ├── tutorials.md
    └── benefits.md
```

## Common Tasks

### Building

```bash
# Build all crates
cargo build --workspace

# Build release (optimized)
cargo build --workspace --release

# Build specific crate
cargo build -p daemon
cargo build -p cli

# Build frontend
cd frontend && npm run build
```

### Testing

```bash
# Run all Rust tests
cargo test --workspace

# Run specific crate tests
cargo test -p protocol
cargo test -p compiler

# Run integration tests (requires built daemon)
cargo build --release
cargo test -p e2e-tests

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_compile_command_generates_code
```

### Formatting and Linting

```bash
# Format all Rust code
cargo fmt --all

# Check formatting
cargo fmt --all -- --check

# Run clippy (linter)
cargo clippy --workspace

# Run clippy with warnings as errors
cargo clippy --workspace -- -D warnings
```

### Running

```bash
# Run daemon
cargo run -p daemon

# Run daemon with env vars
BRIDGE_TCP_ADDR=0.0.0.0:7878 cargo run -p daemon

# Run CLI
cargo run -p cli -- ping
cargo run -p cli -- compile "service hello"

# Run frontend
cd frontend && npm run dev
```

### Debugging

```bash
# Run with debug output
RUST_LOG=debug cargo run -p daemon

# Run with backtrace on panic
RUST_BACKTRACE=1 cargo run -p daemon

# Run specific binary with debugger (lldb/gdb)
cargo build
lldb target/debug/daemon
```

## Module-Specific Development

### CLI Development

Located in `cli/src/main.rs`.

**Common changes:**
- Adding new commands
- Improving error messages
- Better output formatting

**Testing:**
```bash
cargo run -p cli -- ping
cargo run -p cli -- compile "service test"
```

### Daemon Development

Located in `daemon/src/`.

**Module breakdown:**
- `main.rs` — Startup, threading
- `state.rs` — Shared state structure
- `tcp.rs` — TCP protocol handler
- `http.rs` — HTTP API handler
- `sqldb.rs` — Docker management

**Testing:**
```bash
# Start daemon
cargo run -p daemon

# In another terminal
curl http://localhost:8787/health
echo "PING" | nc localhost 7878
```

### Protocol Development

Located in `protocol/src/lib.rs`.

**Common changes:**
- Adding new command types
- Improving parsing
- Better error handling

**Testing:**
```bash
cargo test -p protocol
```

### Compiler Development

Located in `compiler/src/lib.rs`.

**Common changes:**
- Extending DSL syntax
- Better error messages
- AST improvements

**Testing:**
```bash
cargo test -p compiler
```

### Codegen Development

Located in `codegen/src/lib.rs`.

**Common changes:**
- Improving generated TypeScript
- Adding new target languages
- Better type mappings

**Testing:**
```bash
cargo test -p codegen

# Manual test
bridge compile-file examples/hello.bridge > output.ts
cat output.ts
```

### Miniredis Development

Located in `miniredis/src/`.

**Common changes:**
- Adding new Redis commands
- Improving RESP parsing
- Performance optimization

**Testing:**
```bash
cargo test -p miniredis

# Manual test with redis-cli
cargo run -p daemon &
redis-cli -p 6399
> PING
> SET key value
> GET key
```

### Frontend Development

Located in `frontend/src/`.

**Common changes:**
- UI improvements
- New dashboard views
- Better error display

**Testing:**
```bash
cd frontend
npm run dev
# Open http://localhost:5173
```

**Hot reload:** Vite automatically reloads on file changes.

## Adding New Features

### Example: Adding a New Command

Let's add a `STATS` command to get daemon statistics.

#### 1. Update Protocol

`protocol/src/lib.rs`:

```rust
pub enum Command {
    // ... existing commands
    Stats,
}

pub fn parse_command(line: &str) -> Result<Command, String> {
    // ... existing parsing
    if trimmed.eq_ignore_ascii_case("STATS") {
        return Ok(Command::Stats);
    }
    // ...
}
```

#### 2. Update Daemon

`daemon/src/tcp.rs`:

```rust
use protocol::{Command, Response};

pub fn process_line_command(line: &str, state: Arc<Mutex<State>>) -> String {
    let cmd = match protocol::parse_command(line) {
        Ok(c) => c,
        Err(e) => return protocol::render_response(Response::Error(e)),
    };
    
    match cmd {
        // ... existing handlers
        Command::Stats => handle_stats(state),
    }
}

fn handle_stats(state: Arc<Mutex<State>>) -> String {
    let state = state.lock().unwrap();
    let stats = format!(
        "mode={}\nredis_connections={}\nstore_keys={}",
        state.mode,
        state.redis_connections.as_ref().map(|c| c.load(Ordering::Relaxed)).unwrap_or(0),
        state.store.count_keys()
    );
    protocol::render_response(Response::Data(stats))
}
```

#### 3. Update CLI

`cli/src/main.rs`:

```rust
"stats" => "STATS".to_string(),
```

Update usage:

```rust
fn print_usage_and_exit(code: i32) {
    eprintln!(
        "usage: cli <command>\n\
         commands:\n\
           // ... existing commands
           stats\n\
           // ..."
    );
    process::exit(code);
}
```

#### 4. Add Tests

`daemon/src/main.rs`:

```rust
#[test]
fn stats_command_returns_data() {
    let state = test_state();
    let response = process_line_command("STATS", Arc::clone(&state));
    assert!(response.starts_with("DATA "));
    assert!(response.contains("mode="));
}
```

#### 5. Test It

```bash
cargo build --workspace
cargo run -p daemon &
bridge stats
```

## Troubleshooting

### Daemon Won't Start

```bash
# Check if port is already in use
netstat -an | grep 7878
netstat -an | grep 8787

# Kill existing process
pkill -f "target/debug/daemon"

# Start with different port
BRIDGE_TCP_ADDR=127.0.0.1:9999 cargo run -p daemon
```

### CLI Can't Connect

```bash
# Verify daemon is running
ps aux | grep daemon

# Check connectivity
nc -zv 127.0.0.1 7878

# Use correct address
export BRIDGE_TCP_ADDR=127.0.0.1:7878
bridge ping
```

### Compilation Errors

```bash
# Clean build artifacts
cargo clean

# Update Rust
rustup update

# Check toolchain
rustc --version
cargo --version
```

### Frontend Won't Start

```bash
# Reinstall dependencies
cd frontend
rm -rf node_modules package-lock.json
npm install

# Check Node version
node --version  # Should be 18+
```

### Docker Issues

```bash
# Check Docker is running
docker ps

# Verify network
docker network ls

# Reset Docker state
docker-compose down -v
docker-compose up -d
```

## Performance Tips

### Rust Build Speed

```bash
# Use cargo-watch for auto-rebuild
cargo install cargo-watch
cargo watch -x "run -p daemon"

# Parallel builds
cargo build --workspace -j8

# Incremental compilation (already default)
export CARGO_INCREMENTAL=1
```

### Frontend Dev Speed

```bash
# Vite is already fast, but you can:
cd frontend
npm run dev -- --host 0.0.0.0  # Network access
```

## Code Style Guidelines

### Rust

- Use `cargo fmt` before committing
- Follow standard Rust naming conventions
- Add doc comments for public APIs
- Keep functions small and focused
- Avoid unwrap() in production code paths

### TypeScript

- Use TypeScript strict mode
- Prefer `const` over `let`
- Use template literals for strings
- Add type annotations for function parameters

### Git Commits

Use conventional commits:

```
feat: add stats command
fix: handle missing Docker gracefully
docs: update CLI reference
refactor: simplify TCP handler
test: add protocol parsing tests
chore: update dependencies
```

## Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust By Example](https://doc.rust-lang.org/rust-by-example/)
- [Redis Protocol](https://redis.io/docs/reference/protocol-spec/)
- [Encore Docs](https://encore.dev/docs) (inspiration)
- [Vite Guide](https://vitejs.dev/guide/)
- [Tailwind CSS](https://tailwindcss.com/docs)

## Getting Help

- Read module README files
- Check [CONTRIBUTING.md](CONTRIBUTING.md)
- Open an issue on GitHub
- Join discussions

## CI/CD

(To be added)

The project will use GitHub Actions for:
- Running tests on PR
- Formatting checks
- Clippy lints
- Building release binaries

## Release Process

(To be documented)

1. Update version in Cargo.toml files
2. Update CHANGELOG.md
3. Tag release
4. Build binaries for multiple platforms
5. Publish to GitHub Releases

---

**Happy coding! 🚀**
