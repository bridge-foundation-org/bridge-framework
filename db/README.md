# db — Bridge In-Memory Store

Thread-safe, namespace-partitioned key-value store used by the Bridge daemon.

## Overview

The `db` crate provides a lightweight in-memory data layer that supports:

- **Namespaces** — isolate data by service, session, or component
- **TTL / expiry** — keys auto-expire after a configurable duration
- **Transactions** — batch multiple writes; commit or rollback atomically
- **Pattern search** — wildcard lookups with `*` and `?` glob syntax
- **Statistics** — per-namespace key counts and memory estimates

It is the backbone of the daemon's service registry, session storage, and cached responses. The entire API is synchronous and uses a single `Arc<Mutex<...>>` for sharing across threads with no async overhead.

## Quick Start

```rust
use db::Db;
use std::time::Duration;

let db = Db::new();

// Basic put/get/del
db.put("sessions", "user:123", "jwt-token");
assert_eq!(db.get("sessions", "user:123"), Some("jwt-token".to_string()));
db.del("sessions", "user:123");

// TTL — key auto-expires after 60 s
db.put_with_ttl("cache", "hot-key", "value", Duration::from_secs(60));
assert!(db.ttl("cache", "hot-key").is_some());

// Pattern search
db.put("routes", "GET /users", "users::list");
db.put("routes", "POST /users", "users::create");
let all = db.keys_matching("routes", "GET *");
assert_eq!(all, vec!["GET /users"]);
```

## API Reference

### `Db::new() → Db`

Creates a new, empty database instance. Cheap to clone — all clones share the same backing store.

### Core Operations

| Method | Description |
|--------|-------------|
| `put(ns, key, value)` | Insert or overwrite a key |
| `put_with_ttl(ns, key, value, duration)` | Insert with auto-expiry |
| `get(ns, key) → Option<String>` | Retrieve a key (returns `None` if missing or expired) |
| `del(ns, key) → bool` | Delete a key; `true` if it existed |
| `exists(ns, key) → bool` | Check key existence (respects expiry) |

### TTL Operations

| Method | Description |
|--------|-------------|
| `expire(ns, key, duration) → bool` | Set/update TTL on an existing key |
| `persist(ns, key) → bool` | Remove TTL, making key permanent |
| `ttl(ns, key) → Option<Duration>` | Remaining time-to-live |
| `purge_expired()` | Sweep all namespaces and remove expired keys |

### Namespace Operations

| Method | Description |
|--------|-------------|
| `keys(ns) → Vec<String>` | All live keys in a namespace |
| `keys_matching(ns, pattern) → Vec<String>` | Keys matching a glob pattern |
| `values(ns) → Vec<String>` | All live values in a namespace |
| `flush_ns(ns)` | Delete all keys in a namespace |
| `flush()` | Delete all data in all namespaces |

### Statistics

```rust
let stats = db.stats();
// stats.total_keys    — total key count across all namespaces
// stats.namespaces    — per-namespace { keys, size_bytes }
```

### Transactions

```rust
let mut tx = db.begin();
tx.put("orders", "order:1", "pending");
tx.put("orders", "order:2", "paid");
tx.del("orders", "order:0");

db.commit(tx);  // apply atomically, or
db.rollback(tx); // discard (no-op)
```

Transactions buffer operations in memory. `commit` applies them all under the single mutex lock. `rollback` drops the buffer with no side effects.

## Pattern Syntax

`keys_matching` uses simple glob matching:

| Pattern | Matches |
|---------|---------|
| `*` | any sequence of characters |
| `?` | exactly one character |
| `GET *` | all keys starting with `GET ` |
| `user:?` | `user:a`, `user:1`, etc. |
| `*:active` | anything ending in `:active` |

## Used By

- **daemon/state** — service registry, session tokens, watched files
- **daemon/tcp** — responses cached between requests
- **daemon/http** — API rate-limit counters
- **e2e-tests** — unit tests for db behaviour

## Design Notes

- Zero external dependencies — pure `std`.
- All public methods take `&self`, so `Db` can be freely cloned and shared between threads.
- Expiry is lazy: expired keys are hidden from reads but not deleted until the next `purge_expired()` call (or a namespace flush).
- Transactions are optimistic — no per-key locking, just a buffered list of ops applied atomically.
