# miniredis — Embedded Redis-Compatible Server

A lightweight, pure-Rust Redis server that speaks the Redis Serialization Protocol (RESP). No external crates required.

## Overview

`miniredis` launches a background TCP thread that handles Redis clients using the standard RESP wire format. It's used by the Bridge daemon to provide caching without requiring a separate Redis installation.

**Default port:** `6399` (configurable via `BRIDGE_REDIS_ADDR`)

## Supported Commands

### String Commands

| Command | Description |
|---------|-------------|
| `GET key` | Get value of a key |
| `SET key value [EX seconds]` | Set a key with optional TTL |
| `SETNX key value` | Set only if key doesn't exist |
| `SETEX key seconds value` | Set with expiry |
| `MGET key [key ...]` | Get multiple keys |
| `MSET key value [key value ...]` | Set multiple keys |
| `INCR key` | Increment integer value by 1 |
| `DECR key` | Decrement integer value by 1 |
| `INCRBY key n` | Increment by n |
| `DECRBY key n` | Decrement by n |

### List Commands

| Command | Description |
|---------|-------------|
| `LPUSH key value [value ...]` | Prepend values to a list |
| `RPUSH key value [value ...]` | Append values to a list |
| `LRANGE key start stop` | Get a range of list elements |
| `LLEN key` | Get list length |
| `LINDEX key index` | Get element by index |

### Hash Commands

| Command | Description |
|---------|-------------|
| `HSET key field value [field value ...]` | Set hash fields |
| `HSETNX key field value` | Set field only if it doesn't exist |
| `HGET key field` | Get a hash field |
| `HMGET key field [field ...]` | Get multiple hash fields |
| `HGETALL key` | Get all fields and values |
| `HDEL key field [field ...]` | Delete hash fields |
| `HLEN key` | Get number of fields |
| `HEXISTS key field` | Check if field exists |
| `HKEYS key` | Get all field names |
| `HVALS key` | Get all values |
| `HINCRBY key field n` | Increment integer field by n |

### Key Management

| Command | Description |
|---------|-------------|
| `DEL key [key ...]` | Delete keys |
| `EXISTS key [key ...]` | Check if keys exist |
| `EXPIRE key seconds` | Set key TTL in seconds |
| `TTL key` | Get remaining TTL (-1 = no expiry, -2 = missing) |
| `KEYS pattern` | List matching keys (`*` wildcard) |
| `TYPE key` | Get value type (`string`, `list`, `hash`) |
| `FLUSHDB` | Delete all keys |

### Server Commands

| Command | Description |
|---------|-------------|
| `PING [message]` | Health check |
| `COMMAND` | List supported commands |

## Usage

### Starting the Server

```rust
use miniredis::MiniRedis;

// Start on default port (6399)
let server = MiniRedis::start("127.0.0.1:6399").unwrap();

// Check stats
println!("commands served: {}", server.commands_served());
println!("active clients: {}", server.active_clients());

// Stop the server
server.stop();
```

### Connecting with redis-cli

```bash
redis-cli -p 6399
127.0.0.1:6399> SET foo bar
OK
127.0.0.1:6399> GET foo
"bar"
127.0.0.1:6399> EXPIRE foo 60
(integer) 1
127.0.0.1:6399> TTL foo
(integer) 59
```

### Connecting from the Bridge CLI

```bash
bridge redis-status
```

## RESP Protocol

miniredis speaks RESP2 (Redis Serialization Protocol version 2), the same wire format used by Redis 6 and earlier clients.

Data types:

| Symbol | Type | Example |
|--------|------|---------|
| `+` | Simple string | `+OK\r\n` |
| `-` | Error | `-ERR unknown command\r\n` |
| `:` | Integer | `:42\r\n` |
| `$` | Bulk string | `$5\r\nhello\r\n` |
| `*` | Array | `*2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n` |

Inline commands (space-separated, no RESP encoding) are also supported for easy testing with `telnet` or `nc`.

## Architecture

```
TCP listener (background thread)
  │
  ├── per-client goroutine (thread)
  │     ├── parse_resp()  ← RESP2 parser
  │     ├── dispatch()    ← command router
  │     └── serialize()   ← RESP2 serializer
  │
  └── shared store: Arc<Mutex<HashMap<String, Entry>>>
        ├── StoreVal::String
        ├── StoreVal::List
        └── StoreVal::Hash
```

Each client gets its own OS thread. The shared store is protected by a single mutex. TTL expiry is lazy — expired keys are hidden on reads, not deleted on a timer.

## Limitations vs Real Redis

- No persistence (data is lost on restart)
- No AUTH command
- No SELECT (only database 0)
- No Pub/Sub
- No scripting (EVAL)
- No cluster support
- No streams (XADD etc.)
- Limited KEYS pattern syntax (`*` only, no `[`, `?`)

For production use, replace miniredis with a real Redis instance by pointing `BRIDGE_REDIS_ADDR` at it. The Bridge daemon is transparent to the backend.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BRIDGE_REDIS_ADDR` | `127.0.0.1:6399` | Listen address for miniredis |

## Used By

- **daemon** — caching layer, session storage
- **bridge redis-status** — connectivity check
- **e2e-tests** — miniredis RESP protocol tests
