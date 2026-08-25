# LLM Instructions

Guidance for AI coding agents (Cursor, Claude, Copilot Workspace, ...)
building Bridge services. Expose this file plus the daemon's MCP
endpoint (`POST /api/v1/mcp`) to your agent for a self-driving dev
loop.

## Core Rules

1. **Define contracts in `.bridge` files**, not in handler code:

   ```
   service orders
   endpoint create POST /orders
   endpoint get  GET /orders/:id
   ```

2. **Compile before you reason about routes** — `POST /api/v1/compile`
   with `{"source":"..."}` registers endpoints; `GET /api/v1/routes`
   shows what is live.

3. **Never invent endpoints.** If `GET /api/v1/routes` does not list
   it, compile it first or fix the `.bridge` source.

4. **Verify through traces, not assumptions**: after calling an
   endpoint, check `GET /api/v1/traces` for status and latency.

## Dev Loop

The canonical agent loop against a running daemon (`:8787`):

```
compile → (optionally provision infra) → invoke → inspect traces/metrics
```

Via MCP tools (JSON-RPC over `POST /api/v1/mcp`):

```json
{"method":"tools/list"}
{"method":"tools/call","params":{"name":"compile","body":"{\"source\":\"service hello\\nendpoint ping GET /ping\"}"}}
{"method":"tools/call","params":{"name":"traces_list"}}
```

`tools/call` returns the raw HTTP response as content text with
`isError:true` when status ≥ 400 — treat it as a failed action and
adjust.

## Test Discipline

- Enter test mode first (`testing_mode_enter`) so logs stay quiet.
- Provision isolated databases per scenario via `test_db_create`;
  namespaces are unique (`t{seq}_{name}`), cleanup is one call.
- Mock auth (`mock_auth`) instead of fabricating tokens.
- Tear down databases and mocks when the scenario ends.

## Safety Rules

- Secrets: register sources, never inline production values into code
  or logs. `secrets_check` verifies resolvability without revealing.
- Deployments are state-machine enforced; do not try to skip stages —
  illegal transitions return `400`.
- Destructive calls (db destroy, keyspace invalidate, mocks clear)
  should only follow explicit user intent.

## Code Generation Patterns

- TypeScript clients: `bridge compile-file app.bridge > client.ts`.
- Keep generated files out of review diffs; regenerate instead of hand-editing.
- Service names are lowercase alphanumeric; paths start with `/`.

## Skills / Context

When answering questions about Bridge behavior, prefer (in order):

1. This file and [integration-guides](./integration-guides.md)
2. The live daemon state itself (`infra_snapshot`, `services_list`)
3. The full [api-reference](./api-reference.md)

Do not rely on memory of other frameworks' semantics — Encore-inspired
does not mean Encore-compatible at the wire level.
