# Architecture

## Workspace Crates

Bridge is a Cargo workspace with the following crates:

- **protocol** — Shared command/response types and TCP wire format
- **compiler** — Parses Bridge DSL (`service`, `endpoint`) into a Service AST
- **codegen** — Generates TypeScript clients from the compiler's AST
- **db** — In-memory key-value store with namespace support
- **miniredis** — Embedded Redis-compatible server (RESP protocol, TTL support)
- **daemon** — TCP + HTTP server that orchestrates all crates
- **cli** — Command-line client that talks to the daemon over TCP
- **e2e-tests** — Integration tests that spawn the daemon and exercise all APIs

## Dependency Graph

```
daemon -> protocol, db, compiler, codegen, miniredis
cli    -> protocol
e2e-tests (uses daemon binary as subprocess)
```

## Data Flow

1. User writes `.bridge` source (DSL)
2. CLI sends `COMPILE <source>` to daemon over TCP
3. Daemon calls `compiler::compile()` → Service AST
4. Daemon calls `codegen::generate_typescript()` → TypeScript client code
5. Result stored in `db::Store` and returned to client

## No External Dependencies

All Rust crates use only `std`. No tokio, no serde, no external crates.
