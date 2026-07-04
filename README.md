# Bridge Framework

> A lightweight, Encore-inspired framework for building type-safe backend services with generated frontend clients.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Overview

Bridge is a developer-friendly framework that lets you define your APIs in a simple DSL, automatically generate type-safe TypeScript clients, and manage infrastructure through an elegant dev dashboard. Think of it as Encore's minimal cousin — same philosophy, zero dependencies.

```bridge
service hello
endpoint ping GET /ping
endpoint echo POST /echo
```

**Features:**
- 🎯 **Zero config** — Start building in seconds
- 🔄 **Hot codegen** — TypeScript clients generated on the fly
- 🐘 **Docker integration** — PostgreSQL containers managed for you
- ⚡ **Built-in caching** — Embedded Redis-compatible server (miniredis)
- 🎨 **Dev dashboard** — Beautiful Encore-inspired UI
- 📦 **Pure Rust** — Only stdlib, no external crates
- 🚀 **Production ready** — Deploy anywhere Docker runs

## Quick Start

### Prerequisites

- Rust 1.70+ ([install](https://rustup.rs/))
- Node.js 18+ ([install](https://nodejs.org/))
- Docker (optional, for PostgreSQL)

### Quick Start (Automated)

**Windows:**
```bash
git clone https://github.com/yourusername/bridge-framework.git
cd bridge-framework
scripts\dev\start-dev.bat
```

**Unix/Linux/Mac:**
```bash
git clone https://github.com/yourusername/bridge-framework.git
cd bridge-framework
chmod +x scripts/dev/start-dev.sh
scripts/dev/start-dev.sh
```

Open [http://localhost:5173](http://localhost:5173) to see the dev dashboard! 🎉

### Manual Setup

If you prefer step-by-step control:

```bash
# 1. Build everything
cargo build --workspace

# 2. Install CLI
cargo install --path cli

# 3. Start daemon (Terminal 1)
cargo run -p daemon

# 4. Start frontend (Terminal 2)
cd frontend && npm install && npm run dev

# 5. Use CLI (Terminal 3)
bridge ping
```

## Architecture

```
┌─────────────┐
│  .bridge    │  Define services in simple DSL
│   Source    │
└──────┬──────┘
       │
       ▼
┌─────────────┐
│  Compiler   │  Parse and validate
└──────┬──────┘
       │
       ▼
┌─────────────┐
│   Codegen   │  Generate TypeScript client
└──────┬──────┘
       │
       ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ PostgreSQL  │◄────┤   Daemon    ├────►│  Miniredis  │
│  (Docker)   │     │  TCP + HTTP │     │   (Cache)   │
└─────────────┘     └─────────────┘     └─────────────┘
                           │
                           ▼
                    ┌─────────────┐
                    │  Frontend   │  Dev dashboard
                    │  Dashboard  │
                    └─────────────┘
```

## Project Structure

```
bridge-framework/
├── cli/              # Command-line interface
├── daemon/           # Backend server (TCP + HTTP)
│   ├── src/
│   │   ├── main.rs   # Entry point
│   │   ├── state.rs  # Shared state
│   │   ├── tcp.rs    # TCP protocol server
│   │   ├── http.rs   # HTTP REST API
│   │   └── sqldb.rs  # Docker Postgres management
├── protocol/         # Command/Response protocol
├── compiler/         # Bridge DSL parser
├── codegen/          # TypeScript client generator
├── db/               # In-memory key-value store
├── miniredis/        # Embedded Redis-compatible server
├── e2e-tests/        # Integration test suite
├── frontend/         # Dev dashboard (Vite + Tailwind)
└── docs/             # Comprehensive documentation
```

Each module has its own README with detailed documentation.

## CLI Reference

```bash
# Core commands
bridge init <project-dir>       # Create new project
bridge ping                     # Check daemon health
bridge compile <source>         # Compile Bridge DSL
bridge compile-file <path>      # Compile from file

# Database management (requires Docker)
bridge db-create <name>         # Create Postgres container
bridge db-status                # Check container status
bridge db-migrate <sql-file>    # Run SQL migration
bridge db-destroy <name>        # Stop and remove container

# Redis
bridge redis-status             # Check miniredis status

# Daemon control
bridge mode-get                 # Get current mode
bridge mode-set <mode>          # Set mode (lite|full|ultra|off)
```

## Docker Compose Setup

For easy infrastructure management:

```bash
docker-compose up -d
```

This starts PostgreSQL and provides a Redis-compatible endpoint. See [docker-compose.yml](docker-compose.yml) for configuration.

## Development Workflow

### 1. Write Your Service

Create `myapp.bridge`:
```bridge
service users
endpoint list GET /users
endpoint get GET /users/:id
endpoint create POST /users
```

### 2. Generate Client

```bash
bridge compile-file myapp.bridge > frontend/bridge.gen/client.ts
```

### 3. Use in Frontend

```typescript
import { createClient } from "~bridge/client";

const client = createClient("http://localhost:8787");
const users = await client.users.list();
```

### 4. Manage Infrastructure

Use the dev dashboard at [http://localhost:5173](http://localhost:5173):
- **Overview** — See system health, parse endpoints
- **API Explorer** — Test HTTP endpoints interactively
- **Infrastructure** — Create databases, run migrations, monitor Redis
- **Docs** — Comprehensive documentation built-in

## Testing

```bash
# Run all tests
cargo test --workspace

# Run integration tests (requires daemon build)
cargo build --release
cargo test -p e2e-tests

# Run frontend dev server
cd frontend && npm run dev
```

## Documentation

- **Getting Started**
  - [Installation Guide](docs/install.md)
  - [Quick Start](#quick-start-automated) (see above)
  
- **Core Concepts**
  - [Architecture Overview](docs/architecture.md)
  - [Database Management](docs/database.md)
  - [Caching with Miniredis](docs/caching.md)
  - [API Reference](docs/api-reference.md)
  
- **Development**
  - [Development Guide](docs/project/DEVELOPMENT.md)
  - [Contributing Guide](docs/project/CONTRIBUTING.md)
  - [Project Map](docs/project/PROJECT_MAP.md)
  
- **Project**
  - [Changelog](docs/project/CHANGELOG.md)
  - [Implementation Plan](implementation_plan.md)
  - [Deployment Guide](docs/deployment.md)
  - [Tutorials](docs/tutorials.md)

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Module Overview for Contributors

- **cli** — Simple command-line parser, talks to daemon via TCP
- **daemon** — Heart of the system, modular design (state, tcp, http, sqldb)
- **protocol** — Command/Response types, shared by CLI and daemon
- **compiler** — Parses `.bridge` files into AST
- **codegen** — Transforms AST into TypeScript client code
- **db** — Thread-safe in-memory store with namespaces
- **miniredis** — Redis protocol implementation (RESP parser + command handlers)
- **frontend** — Vite + TypeScript + Tailwind dashboard

See individual README files in each module for detailed architecture and contribution guidelines.

## Design Philosophy

Following Encore's principles:

1. **Simple by default** — Minimal configuration, sensible defaults
2. **Type-safe** — Generated clients match your backend contracts
3. **Developer joy** — Great errors, beautiful UI, fast feedback loops
4. **Modular** — Each component has a single responsibility
5. **Zero dependencies** — Pure Rust stdlib, no tokio/serde/async complexity

## Comparison with Encore

| Feature | Bridge | Encore |
|---------|--------|--------|
| Language | Rust | Go + TypeScript |
| Runtime | Stdlib only | Complex async runtime |
| Setup | Clone + cargo build | Install CLI |
| Database | Docker Postgres | Cloud-managed |
| Cache | Embedded miniredis | External Redis |
| Dashboard | Included | Cloud-hosted |
| Cost | Free | Paid tiers |

Bridge is ideal for:
- Local development
- Self-hosted deployments
- Learning how frameworks work
- Projects that need full control
- Rust shops

## Roadmap

See [implementation_plan.md](implementation_plan.md) for detailed status.

**Completed:**
- ✅ Core protocol and daemon
- ✅ Docker PostgreSQL management
- ✅ Embedded miniredis
- ✅ TypeScript codegen
- ✅ Dev dashboard with Encore-inspired UI

**In Progress:**
- 🚧 Enhanced modularity and contributor guides
- 🚧 Docker Compose infrastructure setup

**Planned:**
- 📋 Authentication and middleware system
- 📋 WebSocket support
- 📋 Pub/Sub messaging
- 📋 Hot reload for daemon
- 📋 CLI autocomplete

## License

MIT — see [LICENSE](LICENSE) file.

## Acknowledgments

Inspired by [Encore](https://encore.dev) and their brilliant approach to backend development. Bridge is an educational project exploring these ideas with different tradeoffs.

## Support

- 📖 [Documentation](docs/)
- 💬 [Discussions](https://github.com/yourusername/bridge-framework/discussions)
- 🐛 [Issue Tracker](https://github.com/yourusername/bridge-framework/issues)

---

**Made with ❤️ by developers, for developers.**
