# Hot Reload (Watcher)

The Bridge watcher monitors `.bridge` files for changes, recompiles them automatically, and pushes Server-Sent Events (SSE) to connected clients.

## How It Works

A background thread polls the modification time (`mtime`) of every watched file every `poll_ms` milliseconds (default 500ms). When a file changes:

1. Reads and recompiles the file with the Bridge compiler
2. On success — updates `service_registry` and `codegen/latest` in shared state
3. Broadcasts an SSE event to all connected clients

```
WatchRegistry (shared state)
  watched files: [(path, last_mtime, last_result), ...]
  sse_clients:   [SseSender, ...]

Background thread (every poll_ms):
  for each file:
    mtime = fs::metadata(path).modified()
    if mtime != last_mtime:
      result = compiler::parse(read(path)) → codegen::generate_typescript()
      update state, broadcast SSE event
```

## SSE Event Format

Clients connect to `GET /api/v1/watch/events`. The connection stays open indefinitely. Events are chunked transfer-encoded.

**Successful recompile:**
```
event: reload
data: {"file":"/app/svc.bridge","status":"ok","ts":1720000000}

```

**Compile error:**
```
event: error
data: {"file":"/app/svc.bridge","status":"error","message":"parse failed at line 3","ts":1720000000}

```

**Keepalive** (sent every 15 seconds when idle):
```
: keepalive

```

## HTTP API

### Watch status

```
GET /api/v1/watch
```

Response:
```json
{
  "watching": true,
  "dirs": 1,
  "files": [
    {"path":"app.bridge","status":"ok","changes":3},
    {"path":"users.bridge","status":"error","changes":1,"error":"missing endpoint method"}
  ],
  "sse_clients": 2,
  "poll_ms": 500,
  "events_total": 12
}
```

### Watch a file

```
POST /api/v1/watch/files
Body: app.bridge
```

Only `.bridge` files are accepted. Returns `400` for other extensions.

### Unwatch a file

```
DELETE /api/v1/watch/files
Content-Type: application/json

{"path":"app.bridge"}
```

Returns `404` if the file was not being watched.

### Watch a directory

```
POST /api/v1/watch/dirs
Body: ./services
```

Scans the directory immediately for `.bridge` files and adds them all. Non-existent directories do not error — they simply add zero files.

### SSE event stream

```
GET /api/v1/watch/events
```

Long-lived connection. Returns `Content-Type: text/event-stream` with chunked transfer encoding. Keep the connection alive; reconnect on disconnect.

## BRIDGE_WATCH_DIR

Set this environment variable to auto-watch a directory at daemon startup:

```bash
BRIDGE_WATCH_DIR=./services cargo run -p daemon
```

## bridge.toml Configuration

```toml
[watch]
enabled = true
poll_ms = 500
dirs    = [".", "services"]
files   = ["app.bridge", "admin.bridge"]
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable the watcher background thread |
| `poll_ms` | integer | `500` | Poll interval in milliseconds (minimum 100) |
| `dirs` | array | `[]` | Directories to scan for `.bridge` files |
| `files` | array | `[]` | Explicit `.bridge` files to watch |

## JavaScript EventSource Client

```javascript
const es = new EventSource("http://localhost:8787/api/v1/watch/events");

es.addEventListener("reload", (event) => {
  const data = JSON.parse(event.data);
  console.log("Reloaded:", data.file);
  // Refresh the TypeScript client in your app
});

es.addEventListener("error", (event) => {
  const data = JSON.parse(event.data);
  console.error("Compile error in", data.file, ":", data.message);
});

es.onerror = () => {
  console.warn("SSE connection lost, browser will reconnect automatically");
};
```

## TypeScript Dashboard Client

```typescript
import { createDaemonClient } from "./daemon-client";

const client = createDaemonClient("http://localhost:8787");

// Status
const status = await client.watchStatus();
console.log(`Watching ${status.files.length} files`);

// Add files / dirs
await client.watchAddFile("app.bridge");
await client.watchAddDir("./services");

// Remove
await client.watchRemoveFile("app.bridge");

// Open SSE stream
const es = client.watchEvents();  // returns EventSource
es.addEventListener("reload", (e) => {
  const d = JSON.parse(e.data);
  console.log("reload:", d.file);
});
```

## Poll Interval Tuning

| Use case | Recommended poll_ms |
|----------|---------------------|
| Fast feedback during active development | 200–300 |
| Default / balanced | 500 |
| Low-resource environment | 1000–2000 |

Setting `poll_ms` below 100ms is ignored (clamped to 100ms).
