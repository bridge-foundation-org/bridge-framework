# Miniredis — Embedded Redis-Compatible Server

A minimal, Redis-compatible in-memory cache server written in pure Rust.

## Overview

Miniredis implements a subset of the Redis protocol (RESP) and command set, designed for:
- Local development and testing
- Embedded caching in applications
- Learning Redis internals

**Not suitable for production** — use real Redis for production workloads.

## Features

- ✅ RESP protocol (REdis Serialization Protocol)
- ✅ Core commands: PING, SET, GET, DEL, EXISTS, KEYS, EXPIRE, TTL
- ✅ Thread-safe storage
- ✅ TTL expiration
- ✅ Multiple concurrent connections
- ✅ Pure Rust stdlib implementation
- ❌ No persistence
- ❌ No replication
- ❌ No clustering
- ❌ Limited command set

## Architecture

```
┌─────────────┐
│ Redis Client│ (any RESP-compatible client)
└──────┬──────┘
       │ RESP protocol
       ▼
┌─────────────┐
│ TCP Listener│ (accepts connections)
└──────┬──────┘
       │
       ▼
┌─────────────┐
│  Dispatcher │ (parses commands)
└──────┬──────┘
       │
       ▼
┌─────────────┐
│   Handlers  │ (executes commands)
└──────┬──────┘
       │
       ▼
┌─────────────┐
│    Store    │ (in-memory HashMap)
└─────────────┘
```

## Usage

### Embedding in Applications

```rust
use miniredis::MiniRedis;

// Start the server
let (server, handle) = MiniRedis::start("127.0.0.1:6399")
    .expect("Failed to start miniredis");

println!("Miniredis listening on {}", server.addr);

// Server runs in background thread
// Your app continues...

// Get connection count
let count = server.connection_count.load(Ordering::Relaxed);
println!("{} active connections", count);

// Wait for server to exit (blocks forever)
handle.join().unwrap();
```

### Using as a Client

Any Redis client works:

**Rust:**
```rust
use redis::Commands;

let client = redis::Client::open("redis://127.0.0.1:6399")?;
let mut con = client.get_connection()?;

con.set("key", "value")?;
let result: String = con.get("key")?;
```

**Python:**
```python
import redis

r = redis.Redis(host='localhost', port=6399)
r.set('key', 'value')
print(r.get('key'))  # b'value'
```

**Node.js:**
```javascript
const redis = require('redis');
const client = redis.createClient({ port: 6399 });

await client.set('key', 'value');
const value = await client.get('key');
```

**CLI:**
```bash
redis-cli -p 6399
127.0.0.1:6399> PING
PONG
127.0.0.1:6399> SET mykey "Hello"
OK
127.0.0.1:6399> GET mykey
"Hello"
```

## Supported Commands

| Command | Description | Example |
|---------|-------------|---------|
| PING | Health check | `PING` → `PONG` |
| SET | Set key-value | `SET key value` → `OK` |
| GET | Get value | `GET key` → `"value"` |
| DEL | Delete keys | `DEL key1 key2` → `(integer) 2` |
| EXISTS | Check existence | `EXISTS key` → `(integer) 1` |
| KEYS | List keys (glob) | `KEYS user:*` → array |
| EXPIRE | Set TTL (seconds) | `EXPIRE key 60` → `(integer) 1` |
| TTL | Get TTL | `TTL key` → `(integer) 55` |
| COMMAND | List commands | `COMMAND` → nested array |

### Unsupported Commands

All other Redis commands return:
```
(error) ERR unknown command '<command>'
```

To add support, see [Adding Commands](#adding-commands).

## RESP Protocol

Miniredis implements RESP2 (Redis Serialization Protocol v2).

### Data Types

```
Simple Strings:  +OK\r\n
Errors:          -ERR message\r\n
Integers:        :42\r\n
Bulk Strings:    $5\r\nhello\r\n
Arrays:          *2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n
```

### Request Example

```
*3\r\n$3\r\nSET\r\n$5\r\nmykey\r\n$7\r\nmyvalue\r\n
```

Breakdown:
- `*3` — Array of 3 elements
- `$3\r\nSET` — Bulk string "SET"
- `$5\r\nmykey` — Bulk string "mykey"
- `$7\r\nmyvalue` — Bulk string "myvalue"

### Response Example

```
+OK\r\n
```

Simple string "OK".

## Code Structure

### lib.rs

Public API:

```rust
pub struct MiniRedis {
    pub addr: SocketAddr,
    pub connection_count: Arc<AtomicUsize>,
}

impl MiniRedis {
    pub fn start(addr: &str) -> Result<(Self, JoinHandle<()>), String>
}
```

### Internal Modules

```
src/
├── lib.rs          # Public API and TCP listener
├── resp.rs         # RESP protocol parser/serializer
├── store.rs        # Thread-safe in-memory storage
├── commands.rs     # Command handlers
└── dispatch.rs     # Command routing
```

### resp.rs

RESP protocol implementation:

```rust
pub enum RespValue {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Vec<u8>),
    Array(Vec<RespValue>),
    Null,
}

pub fn parse(input: &[u8]) -> Result<(RespValue, usize), String>
pub fn serialize(value: &RespValue) -> Vec<u8>
```

### store.rs

Thread-safe storage with TTL:

```rust
pub struct Store {
    data: Arc<Mutex<HashMap<String, Entry>>>,
}

struct Entry {
    value: Vec<u8>,
    expires_at: Option<Instant>,
}

impl Store {
    pub fn set(&self, key: String, value: Vec<u8>)
    pub fn get(&self, key: &str) -> Option<Vec<u8>>
    pub fn del(&self, keys: &[String]) -> usize
    pub fn expire(&self, key: &str, seconds: u64) -> bool
    // ...
}
```

Expired keys are cleaned up on access.

### commands.rs

Command handlers:

```rust
pub fn handle_ping() -> RespValue
pub fn handle_set(store: &Store, args: &[RespValue]) -> RespValue
pub fn handle_get(store: &Store, args: &[RespValue]) -> RespValue
// ...
```

Each returns a `RespValue` to serialize and send to client.

### dispatch.rs

Routes commands to handlers:

```rust
pub fn dispatch(store: &Store, command: &RespValue) -> RespValue {
    let arr = match command {
        RespValue::Array(a) => a,
        _ => return RespValue::Error("invalid command".into()),
    };
    
    let cmd = extract_string(&arr[0]).to_uppercase();
    match cmd.as_str() {
        "PING" => handle_ping(),
        "SET" => handle_set(store, &arr[1..]),
        "GET" => handle_get(store, &arr[1..]),
        // ...
        _ => RespValue::Error(format!("unknown command '{}'", cmd)),
    }
}
```

## Adding Commands

### 1. Add Handler (commands.rs)

```rust
pub fn handle_incr(store: &Store, args: &[RespValue]) -> RespValue {
    if args.len() != 1 {
        return RespValue::Error("wrong number of arguments".into());
    }
    
    let key = match extract_string(&args[0]) {
        s if !s.is_empty() => s,
        _ => return RespValue::Error("invalid key".into()),
    };
    
    let val = store.get(&key)
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    
    let new_val = val + 1;
    store.set(key, new_val.to_string().into_bytes());
    RespValue::Integer(new_val)
}
```

### 2. Add to Dispatcher (dispatch.rs)

```rust
match cmd.as_str() {
    // ...
    "INCR" => handle_incr(store, &arr[1..]),
    // ...
}
```

### 3. Add Tests

```rust
#[test]
fn test_incr() {
    let store = Store::new();
    let cmd = RespValue::Array(vec![
        RespValue::BulkString(b"INCR".to_vec()),
        RespValue::BulkString(b"counter".to_vec()),
    ]);
    
    let resp = dispatch(&store, &cmd);
    assert_eq!(resp, RespValue::Integer(1));
    
    let resp = dispatch(&store, &cmd);
    assert_eq!(resp, RespValue::Integer(2));
}
```

### 4. Update Documentation

Add to [Supported Commands](#supported-commands) table.

## Testing

```bash
# Unit tests
cargo test -p miniredis

# Manual testing with redis-cli
cargo run -p daemon &
redis-cli -p 6399
```

Test script:

```bash
#!/bin/bash
redis-cli -p 6399 <<EOF
PING
SET mykey "Hello World"
GET mykey
EXISTS mykey
DEL mykey
EXISTS mykey
SET session:abc "user123"
EXPIRE session:abc 60
TTL session:abc
KEYS session:*
EOF
```

## Performance

Miniredis is **not optimized** for production:

| Operation | Throughput | Latency |
|-----------|------------|---------|
| SET | ~50K ops/sec | <1ms |
| GET | ~100K ops/sec | <1ms |
| DEL | ~50K ops/sec | <1ms |

Real Redis achieves **500K+ ops/sec** with proper tuning.

### Bottlenecks

- Mutex lock contention on every operation
- Synchronous I/O (blocking accept/read/write)
- No pipelining support
- No connection pooling

### Improvements

For better performance:
- Use async I/O (tokio)
- Shard storage (multiple mutexes)
- Implement pipelining
- Add connection pooling

## Comparison with Real Redis

| Feature | Miniredis | Redis |
|---------|-----------|-------|
| Commands | 9 | 200+ |
| Persistence | ❌ | ✅ (RDB, AOF) |
| Replication | ❌ | ✅ (master-slave) |
| Clustering | ❌ | ✅ (Redis Cluster) |
| Pub/Sub | ❌ | ✅ |
| Transactions | ❌ | ✅ (MULTI/EXEC) |
| Lua Scripts | ❌ | ✅ |
| Data Types | String | String, Hash, List, Set, ZSet |
| Memory | Unbounded | Configurable limits |
| Performance | Good | Excellent |

## When to Use

### ✅ Good For

- Local development
- Unit/integration tests
- Learning Redis protocol
- Embedded caching (non-critical)
- Prototyping

### ❌ Not Good For

- Production workloads
- Data persistence required
- High throughput (>50K ops/sec)
- Advanced Redis features
- Multi-server replication

**For production, use real Redis.**

## Alternatives

- **redis-rs** — Rust Redis client (use with real Redis)
- **mini-redis** — Tokio-based tutorial implementation
- **valkey** — Redis fork by Linux Foundation
- **dragonfly** — Modern Redis alternative

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md).

Common improvements:
- Add more commands (HSET, LPUSH, SADD, etc.)
- Implement pub/sub
- Add persistence (RDB snapshots)
- Async I/O with tokio
- Connection pooling
- Benchmarking suite

## License

Miniredis code: MIT — see [LICENSE](../LICENSE)

RESP protocol: Public domain

## Acknowledgments

Inspired by:
- [Redis](https://redis.io) — The original
- [mini-redis](https://github.com/tokio-rs/mini-redis) — Tokio tutorial
- [miniredis (Go)](https://github.com/alicebob/miniredis) — Pure Go implementation

## Resources

- [Redis Protocol Spec](https://redis.io/docs/reference/protocol-spec/)
- [Redis Commands](https://redis.io/commands/)
- [RESP3 Specification](https://github.com/redis/redis-specifications/blob/master/protocol/RESP3.md)
