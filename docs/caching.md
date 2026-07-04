# Caching (miniredis)

## Overview

Bridge includes **miniredis**, a minimal Redis-compatible server written in pure Rust. It starts automatically when the daemon boots.

## Default Address

`127.0.0.1:6399` (configurable via `BRIDGE_REDIS_ADDR`)

## Supported Commands

- `PING` — connection test
- `SET key value [EX seconds] [PX milliseconds]` — store with optional TTL
- `GET key` — retrieve a value
- `DEL key [key ...]` — delete keys
- `EXISTS key [key ...]` — check existence
- `KEYS pattern` — glob pattern matching
- `EXPIRE key seconds` — set TTL
- `TTL key` — remaining TTL
- `COMMAND` — compatibility stub

## Connecting

```bash
redis-cli -p 6399
> PING
PONG
> SET hello world
OK
> GET hello
"world"
```

## Architecture

- RESP protocol parser/serializer
- Thread-safe HashMap store with TTL
- TCP listener with concurrent client handling
- Embeddable via `MiniRedis::start(addr)`
