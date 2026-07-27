export type DocPage = {
  id: string;
  title: string;
  subtitle: string;
  body: string;
};

export const docPages: DocPage[] = [
  {
    id: "index",
    title: "Bridge Framework",
    subtitle: "Local-first backend framework with compile-time codegen",
    body: `
Bridge is a lightweight Encore-inspired framework for defining services, compiling
API contracts, and generating typed frontend clients.

## Quick start

1. Start the daemon: \`cargo run -p daemon\`
2. Install the CLI: \`cargo install --path cli\`
3. Run the dev UI: \`cd frontend && npm install && npm run dev\`

## What you get

- **TCP + HTTP daemon** on \`127.0.0.1:7878\` and \`127.0.0.1:8787\`
- **Bridge language** for \`service\` and \`endpoint\` declarations
- **TypeScript client codegen** via \`bridge compile-file\`
- **Vite + Tailwind** frontend for local development
- **Docker Postgres** management for databases
- **Miniredis** embedded caching server
- **E2E test suite** for full integration testing

See **Installation** and **CLI Reference** for details.
`.trim(),
  },
  {
    id: "install",
    title: "Installation",
    subtitle: "Install Bridge CLI and start local development",
    body: `
## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 20+
- Git Bash or WSL (for \`check.bash\`, \`build.sh\`, \`deploy.sh\`)
- Docker (optional, for Postgres database management)

## Install the Bridge CLI

From the bridge-framework repository:

\`\`\`bash
cargo install --path cli
bridge help
\`\`\`

The CLI talks to the local daemon over TCP (\`127.0.0.1:7878\` by default).

## Start the daemon

\`\`\`bash
cargo run -p daemon
\`\`\`

The HTTP API listens on \`127.0.0.1:8787\` for health checks, compile, and codegen.
The daemon also starts a miniredis instance on \`127.0.0.1:6399\`.

## Create a new app

\`\`\`bash
bridge init my-app
cd my-app/frontend
npm install
npm run generate-client:local
npm run dev
\`\`\`

Or use the npm scaffolder:

\`\`\`bash
npx create-bridge-app my-app
\`\`\`

## Verify your setup

\`\`\`bash
./check.bash
\`\`\`

This runs Rust tests and builds the Vite frontend, matching CI behavior.
`.trim(),
  },
  {
    id: "benefits",
    title: "Bridge Benefits",
    subtitle: "How Bridge helps you ship typed APIs faster",
    body: `
Using Bridge to declare services in a simple DSL unlocks several benefits:

- **Local development with instant codegen**: Compile \`.bridge\` files and get TypeScript clients immediately.
- **Rapid feedback**: Catch invalid endpoints before wiring the frontend.
- **No manual client maintenance**: Generated clients stay aligned with your service definition.
- **Unified toolchain**: One daemon exposes compile, mode, and storage over HTTP and TCP.
- **Frontend-ready defaults**: Vite + Tailwind dev UI ships with the framework repo.
- **Database management**: Spin up Postgres containers with a single CLI command.
- **Built-in caching**: Miniredis provides Redis-compatible caching out of the box.

## Simplicity without giving up flexibility

Bridge keeps the compiler and codegen small and inspectable. You can extend the DSL,
swap the frontend stack, or self-host the daemon wherever Rust binaries run.

## Ponytail modes

Bridge supports lazy-dev modes via \`bridge mode-set\`:

- \`lite\` — minimal output
- \`full\` — default balanced mode
- \`ultra\` — verbose diagnostics
- \`off\` — stop extended behavior
`.trim(),
  },
  {
    id: "cli",
    title: "CLI Reference",
    subtitle: "Commands for local development and codegen",
    body: `
## bridge init

\`\`\`bash
bridge init <project-dir>
\`\`\`

Scaffolds \`bridge.app\`, frontend (Vite + Tailwind), and a generated client stub.

## bridge compile / compile-file

\`\`\`bash
bridge compile "service hello\\nendpoint ping GET /ping"
bridge compile-file ./sample.bridge
\`\`\`

Parses Bridge source and returns generated TypeScript via the daemon.

## bridge ping / help / stop

Daemon connectivity and lifecycle helpers over TCP.

## bridge mode-get / mode-set

\`\`\`bash
bridge mode-get
bridge mode-set full
\`\`\`

## bridge db-put / db-get

Key-value storage in the daemon for generated artifacts and metadata.

## bridge db-create / db-status / db-migrate / db-destroy

\`\`\`bash
bridge db-create mydb
bridge db-status
bridge db-migrate migration.sql
bridge db-destroy mydb
\`\`\`

Docker Postgres container lifecycle management.

## bridge redis-status

\`\`\`bash
bridge redis-status
\`\`\`

Check miniredis server address and connection count.

## HTTP API (daemon)

| Method | Path | Description |
|--------|------|-------------|
| GET | /health | Health check |
| GET | /mode | Current ponytail mode |
| POST | /mode | Set ponytail mode |
| POST | /compile | Compile Bridge source |
| GET | /db/latest | Latest codegen output |
| POST | /db/create | Create Docker Postgres container |
| GET | /db/status | Check container status |
| POST | /db/migrate | Run SQL migration |
| DELETE | /db/destroy | Stop and remove container |
| GET | /redis/status | Miniredis server status |

## Build and deploy

\`\`\`bash
./scripts/build.sh    # release binaries + frontend dist
./scripts/deploy.sh   # package deploy bundle
\`\`\`
`.trim(),
  },
  {
    id: "architecture",
    title: "Architecture",
    subtitle: "Crate dependency graph and system overview",
    body: `
## Workspace Crates

Bridge is a Cargo workspace with the following crates:

- **protocol** — Shared command/response types and TCP wire format
- **compiler** — Parses Bridge DSL (\`service\`, \`endpoint\`) into a Service AST
- **codegen** — Generates TypeScript clients from the compiler's AST
- **db** — In-memory key-value store with namespace support
- **miniredis** — Embedded Redis-compatible server (RESP protocol, TTL support)
- **daemon** — TCP + HTTP server that orchestrates all crates
- **cli** — Command-line client that talks to the daemon over TCP
- **e2e-tests** — Integration tests that spawn the daemon and exercise all APIs

## Dependency Graph

\`\`\`bash
daemon -> protocol, db, compiler, codegen, miniredis
cli    -> protocol
e2e-tests (uses daemon binary as subprocess)
\`\`\`

## Data Flow

1. User writes \`.bridge\` source (DSL)
2. CLI sends \`COMPILE <source>\` to daemon over TCP
3. Daemon calls \`compiler::compile()\` -> Service AST
4. Daemon calls \`codegen::generate_typescript()\` -> TypeScript client code
5. Result stored in \`db::Store\` and returned to client

## Frontend Architecture

The Vite + Tailwind frontend communicates with the daemon's HTTP API:

- Dev Dashboard: interactive controls for compile, mode, database, redis
- Documentation: rendered markdown doc pages
- Service Explorer: parses Bridge source and displays endpoints
- API Tester: direct HTTP endpoint testing

## No External Dependencies

All Rust crates use only \`std\`. No tokio, no serde, no external crates. This keeps
the project minimal and auditable.
`.trim(),
  },
  {
    id: "database",
    title: "Database Management",
    subtitle: "Docker Postgres lifecycle via CLI and HTTP",
    body: `
## Overview

Bridge can manage PostgreSQL databases via Docker containers. The daemon wraps
\`docker\` CLI commands to create, inspect, migrate, and destroy Postgres containers.

## Prerequisites

- Docker must be installed and running on the host
- The daemon gracefully handles missing Docker with clear error messages

## Creating a Database

\`\`\`bash
bridge db-create myapp
\`\`\`

This runs:
\`\`\`bash
docker run -d --name bridge_pg_myapp -e POSTGRES_PASSWORD=bridge -p 5432:5432 postgres:16
\`\`\`

## Checking Status

\`\`\`bash
bridge db-status
\`\`\`

Lists all Bridge Postgres containers and their status.

## Running Migrations

\`\`\`bash
bridge db-migrate schema.sql
\`\`\`

Reads the SQL file and executes it against the running Postgres container via \`psql\`.

## Destroying a Database

\`\`\`bash
bridge db-destroy myapp
\`\`\`

Stops and removes the container \`bridge_pg_myapp\`.

## HTTP Endpoints

- \`POST /db/create\` — body: container name
- \`GET /db/status\` — returns container status
- \`POST /db/migrate\` — body: SQL statements
- \`DELETE /db/destroy\` — body: container name
`.trim(),
  },
  {
    id: "caching",
    title: "Caching (miniredis)",
    subtitle: "Embedded Redis-compatible server for local caching",
    body: `
## Overview

Bridge includes **miniredis**, a minimal Redis-compatible server written in pure Rust.
It starts automatically when the daemon boots and listens on \`127.0.0.1:6399\` by default.

## Supported Commands

- \`PING\` — connection test
- \`SET key value [EX seconds] [PX milliseconds]\` — store a value with optional TTL
- \`GET key\` — retrieve a value
- \`DEL key [key ...]\` — delete one or more keys
- \`EXISTS key [key ...]\` — check if keys exist
- \`KEYS pattern\` — find keys matching a glob pattern
- \`EXPIRE key seconds\` — set TTL on an existing key
- \`TTL key\` — check remaining TTL
- \`COMMAND\` — compatibility stub

## Connecting

Any Redis client can connect to miniredis:

\`\`\`bash
redis-cli -p 6399
> PING
PONG
> SET hello world
OK
> GET hello
"world"
\`\`\`

## Configuration

Set the listen address via environment variable:

\`\`\`bash
BRIDGE_REDIS_ADDR=127.0.0.1:6400 cargo run -p daemon
\`\`\`

## Architecture

- **RESP protocol** parser/serializer (Simple Strings, Errors, Integers, Bulk Strings, Arrays)
- **Thread-safe HashMap** store with TTL support
- **TCP listener** accepting concurrent Redis clients
- Embeddable via \`MiniRedis::start(addr)\`
`.trim(),
  },
  {
    id: "deployment",
    title: "Deployment Guide",
    subtitle: "Build and deploy Bridge applications",
    body: `
## Building for Production

\`\`\`bash
./scripts/build.sh
\`\`\`

This produces a \`dist/\` directory with:

- \`bin/daemon\` — release daemon binary
- \`bin/bridge\` — release CLI binary
- \`frontend/\` — static Vite build
- \`docs/\` — markdown documentation

## Deploying

\`\`\`bash
./scripts/deploy.sh
\`\`\`

Packages the dist bundle for deployment.

## Running in Production

\`\`\`bash
./bin/daemon
\`\`\`

The daemon binds to:
- TCP: \`127.0.0.1:7878\` (configurable via \`BRIDGE_TCP_ADDR\`)
- HTTP: \`127.0.0.1:8787\` (configurable via \`BRIDGE_HTTP_ADDR\`)
- Redis: \`127.0.0.1:6399\` (configurable via \`BRIDGE_REDIS_ADDR\`)

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| BRIDGE_TCP_ADDR | 127.0.0.1:7878 | TCP protocol listener |
| BRIDGE_HTTP_ADDR | 127.0.0.1:8787 | HTTP API listener |
| BRIDGE_REDIS_ADDR | 127.0.0.1:6399 | Miniredis listener |

## Serving the Frontend

The built frontend in \`dist/frontend/\` is static HTML/JS/CSS. Serve with any static host:

\`\`\`bash
npx serve dist/frontend
\`\`\`

Or configure nginx, Caddy, or any CDN to serve the files.
`.trim(),
  },
  {
    id: "api-reference",
    title: "API Reference",
    subtitle: "Complete HTTP endpoint documentation",
    body: `
## Base URL

\`http://127.0.0.1:8787\` (configurable via \`BRIDGE_HTTP_ADDR\`)

## Endpoints

## GET /health

Returns daemon health status.

Response: \`{"status":"ok"}\`

## GET /mode

Returns current daemon mode.

Response: \`{"mode":"full"}\`

## POST /mode

Set daemon mode. Body: \`lite\`, \`full\`, \`ultra\`, or \`off\`.

Response: \`{"mode":"<value>"}\`

## POST /compile

Compile Bridge DSL source. Body: raw Bridge source text.

Response: generated TypeScript client code (text/plain)

## GET /db/latest

Get the most recent codegen output.

Response: TypeScript source (text/plain)

## POST /db/create

Create a Docker Postgres container. Body: container name (optional, defaults to "default").

Response: \`{"ok":true,"message":"created container bridge_pg_<name>"}\`

## GET /db/status

Check running Bridge Postgres containers.

Response: \`{"status":"<container status>"}\`

## POST /db/migrate

Execute SQL against the running Postgres container. Body: SQL statements.

Response: \`{"ok":true,"result":"<psql output>"}\`

## DELETE /db/destroy

Stop and remove a Postgres container. Body: container name.

Response: \`{"ok":true,"message":"destroyed container bridge_pg_<name>"}\`

## GET /redis/status

Check miniredis server status.

Response: \`{"addr":"127.0.0.1:6399","connections":0}\`

## CORS

All endpoints return CORS headers for local development:
- \`Access-Control-Allow-Origin: *\`
- \`Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\`
- \`Access-Control-Allow-Headers: content-type\`
`.trim(),
  },
  {
    id: "tutorials",
    title: "Tutorials",
    subtitle: "Step-by-step guides for common tasks",
    body: `
## Tutorial 1: Defining a Service

Create a file called \`hello.bridge\`:

\`\`\`bash
service hello
endpoint ping GET /ping
endpoint echo POST /echo
endpoint greet GET /greet/:name
\`\`\`

## Tutorial 2: Generating a TypeScript Client

\`\`\`bash
bridge compile-file hello.bridge > client.ts
\`\`\`

This produces a typed TypeScript module with functions for each endpoint.

## Tutorial 3: Using the Dev Dashboard

1. Start the daemon: \`cargo run -p daemon\`
2. Start the frontend: \`cd frontend && npm run dev\`
3. Open \`http://localhost:5173\`
4. Paste your Bridge source in the Compiler textarea
5. Click "Compile + Codegen" to see generated TypeScript
6. Click "Parse Endpoints" to see a table of your service's routes

## Tutorial 4: Setting Up a Database

1. Ensure Docker is installed and running
2. Click "Create DB" in the Database panel (or: \`bridge db-create myapp\`)
3. Write SQL in the migration textarea
4. Click "Run Migration" to execute DDL
5. When done, click "Destroy" to clean up

## Tutorial 5: Using the Redis Cache

Miniredis starts automatically with the daemon. Connect with any Redis client:

\`\`\`bash
redis-cli -p 6399
SET session:abc123 '{"user":"alice"}' EX 3600
GET session:abc123
\`\`\`

Check status in the dashboard or via: \`bridge redis-status\`

## Tutorial 6: Running the Full Test Suite

\`\`\`bash
cargo test --workspace     # unit + integration tests
cargo build --workspace    # build all binaries
cargo test -p e2e-tests    # end-to-end tests (spawns daemon)
cd frontend && npm run build  # frontend build check
\`\`\`

Or run everything with: \`./check.bash\`
`.trim(),
  },
];

export function renderMarkdown(text: string): string {
  return text
    .split("\n")
    .map((line) => {
      if (line.startsWith("## ")) {
        return `<h2 class="mt-8 mb-3 text-xl font-semibold text-white">${escapeHtml(line.slice(3))}</h2>`;
      }
      if (line.startsWith("- ")) {
        return `<li class="ml-4 list-disc text-slate-300">${formatInline(line.slice(2))}</li>`;
      }
      if (line.startsWith("|")) {
        const cells = line.split("|").filter(Boolean).map((c) => c.trim());
        if (cells.every((c) => /^-+$/.test(c))) return "";
        const tag = line.includes("---") ? null : "td";
        if (!tag) return "";
        const isHeader = !line.includes(" GET ") && cells[0] === "Method" || cells[0] === "Variable";
        const cellTag = isHeader ? "th" : "td";
        const row = cells
          .map(
            (c) =>
              `<${cellTag} class="border border-slate-700 px-3 py-2 text-left text-sm">${escapeHtml(c)}</${cellTag}>`,
          )
          .join("");
        return `<tr>${row}</tr>`;
      }
      if (line.startsWith("```")) {
        return line.endsWith("```") && line.length > 3 ? "" : line.startsWith("```bash") ? "<pre class=\"my-4 overflow-x-auto rounded-lg bg-slate-900 p-4 text-sm text-emerald-300\"><code>" : "</code></pre>";
      }
      if (line.trim() === "") return "";
      return `<p class="mb-3 leading-relaxed text-slate-300">${formatInline(line)}</p>`;
    })
    .join("\n")
    .replace(
      /(<tr>[\s\S]*?<\/tr>)+/g,
      (table) =>
        `<table class="my-4 w-full border-collapse text-slate-200"><tbody>${table}</tbody></table>`,
    );
}

function formatInline(text: string): string {
  return escapeHtml(text).replace(/`([^`]+)`/g, "<code class=\"rounded bg-slate-800 px-1.5 py-0.5 text-emerald-300\">$1</code>");
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
