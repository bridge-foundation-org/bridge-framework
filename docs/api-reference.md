# API Reference

## Base URL

`http://127.0.0.1:8787` (configurable via `BRIDGE_HTTP_ADDR`)

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | /health | Health check → `{"status":"ok"}` |
| GET | /mode | Current mode → `{"mode":"full"}` |
| POST | /mode | Set mode (body: lite/full/ultra/off) |
| POST | /compile | Compile Bridge source → TypeScript |
| GET | /db/latest | Latest codegen output |
| POST | /db/create | Create Docker Postgres container |
| GET | /db/status | Check container status |
| POST | /db/migrate | Execute SQL migration |
| DELETE | /db/destroy | Remove Postgres container |
| GET | /redis/status | Miniredis status → `{"addr":"...","connections":0}` |

## CORS

All endpoints return:
- `Access-Control-Allow-Origin: *`
- `Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS`
- `Access-Control-Allow-Headers: content-type`

## TCP Protocol

Connect to `127.0.0.1:7878` and send text commands:

```
PING → PONG
HELP → DATA <help text>
MODE GET → MODE <mode>
MODE SET <mode> → OK MODE <mode>
COMPILE <escaped-source> → DATA <escaped-typescript>
DB PUT <ns> <key> <value> → OK stored
DB GET <ns> <key> → DATA <value>
DB CREATE <name> → OK <message>
DB STATUS → DATA <status>
DB MIGRATE <sql> → DATA <result>
DB DESTROY <name> → OK <message>
REDIS STATUS → DATA addr=<addr> connections=<n>
```
