# Bridge Framework — Project Map

Quick reference guide to navigate the codebase.

## 📁 File Structure

```
bridge-framework/
│
├── 📄 Essential Files
│   ├── README.md                    ⭐ Start here! Project overview
│   ├── Cargo.toml                   🦀 Rust workspace config
│   ├── package.json                 📦 NPM scripts
│   ├── implementation_plan.md       🗺️ Feature roadmap
│   ├── CLAUDE.md                    🤖 AI assistant context
│   ├── check.bash                   ✅ Quality checks
│   ├── docker-compose.yml           🐳 Docker services
│   ├── init-db.sql                  🗄️ Database init
│   └── .env.example                 ⚙️ Environment template
│
├── 📚 docs/                         Documentation
│   ├── 📖 User Guides
│   │   ├── index.md                 Documentation home
│   │   ├── install.md               Installation guide
│   │   ├── architecture.md          Architecture overview
│   │   ├── database.md              Database management
│   │   ├── caching.md               Miniredis guide
│   │   ├── deployment.md            Deployment strategies
│   │   ├── api-reference.md         HTTP API documentation
│   │   ├── tutorials.md             Step-by-step tutorials
│   │   ├── benefits.md              Why use Bridge
│   │   └── cli-reference.md         CLI commands
│   │
│   └── 📂 project/                  Project Documentation
│       ├── CONTRIBUTING.md          👥 How to contribute
│       ├── DEVELOPMENT.md           🛠️ Development guide
│       ├── CHANGELOG.md             📝 Version history
│       ├── PROJECT_MAP.md           📍 This file!
│       ├── SUMMARY.md               📊 Modularization summary
│       └── COMPLETED_WORK.md        ✅ What was accomplished
│
├── 🔧 scripts/                      Build & Automation
│   ├── dev/
│   │   ├── start-dev.bat            🪟 Windows startup
│   │   └── start-dev.sh             🐧 Unix/Linux/Mac startup
│   ├── build.sh                     🏗️ Production build
│   └── deploy.sh                    🚀 Deployment
│
├── 🦀 Rust Crates (Backend)
│   │
│   ├── 📦 cli/                      Command-line interface
│   │   ├── src/main.rs              Entry point, command parsing
│   │   ├── Cargo.toml               Dependencies
│   │   └── README.md                📖 CLI architecture and guide
│   │
│   ├── 📦 daemon/                   Backend server
│   │   ├── src/
│   │   │   ├── main.rs              Entry point, threading
│   │   │   ├── state.rs             Shared state management
│   │   │   ├── tcp.rs               TCP protocol server
│   │   │   ├── http.rs              HTTP REST API
│   │   │   └── sqldb.rs             Docker Postgres management
│   │   ├── Cargo.toml               Dependencies
│   │   └── README.md                📖 Daemon architecture and modules
│   │
│   ├── 📦 protocol/                 Shared protocol definitions
│   │   ├── src/lib.rs               Command/Response types, parsing
│   │   ├── Cargo.toml               Dependencies
│   │   └── README.md                📖 Protocol wire format and encoding
│   │
│   ├── 📦 compiler/                 Bridge DSL parser
│   │   ├── src/lib.rs               Lexer, parser, AST
│   │   └── Cargo.toml               Dependencies
│   │
│   ├── 📦 codegen/                  Code generation
│   │   ├── src/lib.rs               AST → TypeScript transformation
│   │   └── Cargo.toml               Dependencies
│   │
│   ├── 📦 db/                       In-memory storage
│   │   ├── src/lib.rs               Thread-safe key-value store
│   │   └── Cargo.toml               Dependencies
│   │
│   ├── 📦 miniredis/                Embedded Redis server
│   │   ├── src/
│   │   │   ├── lib.rs               Public API, TCP listener
│   │   │   ├── resp.rs              RESP protocol parser
│   │   │   ├── store.rs             In-memory storage with TTL
│   │   │   ├── commands.rs          Command handlers
│   │   │   └── dispatch.rs          Command routing
│   │   ├── Cargo.toml               Dependencies
│   │   └── README.md                📖 RESP protocol and commands
│   │
│   ├── 📦 e2e-tests/                Integration tests
│   │   ├── src/lib.rs               End-to-end test suite
│   │   └── Cargo.toml               Dependencies
│   │
│   └── Cargo.toml                   Workspace configuration
│
├── 🎨 Frontend (TypeScript + Vite)
│   └── frontend/
│       ├── src/
│       │   ├── main.ts              App shell, routing, views
│       │   ├── daemon-client.ts     HTTP client for daemon
│       │   ├── docs.ts              Documentation rendering
│       │   └── style.css            Tailwind + custom styles
│       ├── bridge.gen/
│       │   └── client.ts            Generated client code
│       ├── package.json             Frontend dependencies
│       ├── vite.config.ts           Vite configuration
│       ├── tsconfig.json            TypeScript configuration
│       └── index.html               HTML entry point
│
├── 📚 Documentation
│   └── docs/
│       ├── index.md                 Documentation home
│       ├── install.md               Installation guide
│       ├── architecture.md          Architecture overview
│       ├── database.md              Database management
│       ├── caching.md               Miniredis guide
│       ├── deployment.md            Deployment strategies
│       ├── api-reference.md         HTTP API documentation
│       ├── tutorials.md             Step-by-step tutorials
│       ├── benefits.md              Why use Bridge
│       └── cli-reference.md         CLI commands
│
└── 🔧 Build Scripts
    └── scripts/
        ├── build.sh                 Production build
        └── deploy.sh                Deployment script
```

## 🗺️ Navigation Guide

### I Want To...

#### **Understand the Project**
→ Start with [README.md](../../README.md)
→ Then read [architecture.md](../architecture.md)
→ Check [../../implementation_plan.md](../../implementation_plan.md) for roadmap

#### **Get Started Developing**
→ Read [DEVELOPMENT.md](DEVELOPMENT.md)
→ Run `scripts/dev/start-dev.sh` or `scripts\dev\start-dev.bat`
→ Check module READMEs for specific areas

#### **Contribute Code**
→ Read [CONTRIBUTING.md](CONTRIBUTING.md)
→ Pick a module from the structure above
→ Read that module's README
→ Make your changes and submit PR

#### **Use the CLI**
→ See [cli-reference.md](../cli-reference.md)
→ Or `bridge --help` (once installed)

#### **Work on Daemon**
→ Read [../../daemon/README.md](../../daemon/README.md)
→ Understand modules: state, tcp, http, sqldb
→ Make changes and test

#### **Add Redis Commands**
→ Read [../../miniredis/README.md](../../miniredis/README.md)
→ Edit `miniredis/src/commands.rs`
→ Update `miniredis/src/dispatch.rs`
→ Add tests

#### **Improve Frontend**
→ Look at `frontend/src/main.ts`
→ Views are defined as render functions
→ Uses Tailwind for styling
→ Client in `daemon-client.ts`

#### **Understand Protocol**
→ Read [../../protocol/README.md](../../protocol/README.md)
→ See `protocol/src/lib.rs`
→ Text-based, line-delimited
→ URL encoding for special chars

## 🎯 Key Locations

### Configuration Files

| File | Purpose |
|------|---------|
| `Cargo.toml` (root) | Rust workspace config |
| `package.json` (root) | NPM scripts |
| `docker-compose.yml` | Infrastructure services |
| `.env.example` | Environment template |

### Entry Points

| Component | Entry Point |
|-----------|-------------|
| CLI | `cli/src/main.rs` |
| Daemon | `daemon/src/main.rs` |
| Frontend | `frontend/src/main.ts` |
| Tests | `e2e-tests/src/lib.rs` |

### Important Modules

| Module | Location | Purpose |
|--------|----------|---------|
| State Management | `daemon/src/state.rs` | Shared state |
| TCP Server | `daemon/src/tcp.rs` | CLI protocol |
| HTTP Server | `daemon/src/http.rs` | REST API |
| Docker Management | `daemon/src/sqldb.rs` | PostgreSQL |
| RESP Protocol | `miniredis/src/resp.rs` | Redis protocol |
| Command Parsing | `protocol/src/lib.rs` | Protocol types |

## 🔍 Search Tips

### Finding Code

```bash
# Find all references to a function
grep -r "function_name" --include="*.rs"

# Find all HTTP endpoints
grep -r "GET\|POST\|DELETE" daemon/src/http.rs

# Find all CLI commands
grep "match args" cli/src/main.rs

# Find documentation
find docs -name "*.md" | xargs grep "search term"
```

### Understanding Flow

1. **CLI Command Flow**
   ```
   cli/main.rs → protocol/lib.rs → daemon/tcp.rs → handlers
   ```

2. **HTTP Request Flow**
   ```
   frontend → daemon/http.rs → state → response
   ```

3. **Compilation Flow**
   ```
   source → compiler → codegen → db store → frontend
   ```

4. **Database Creation Flow**
   ```
   CLI → protocol → daemon/tcp → daemon/sqldb → Docker
   ```

## 📊 Module Dependencies

```
┌─────────┐     ┌──────────┐     ┌─────────┐
│   CLI   │────▶│ Protocol │◀────│ Daemon  │
└─────────┘     └──────────┘     └────┬────┘
                                       │
                    ┌──────────────────┼──────────────────┐
                    │                  │                  │
              ┌─────▼────┐      ┌─────▼────┐      ┌─────▼────┐
              │ Compiler │      │ Codegen  │      │    DB    │
              └──────────┘      └──────────┘      └──────────┘
                                                         │
                                                   ┌─────▼────┐
                                                   │Miniredis │
                                                   └──────────┘
```

## 🏗️ Build Process

### Development Build

```
1. cargo build --workspace
   ├─▶ cli/
   ├─▶ daemon/
   ├─▶ protocol/
   ├─▶ compiler/
   ├─▶ codegen/
   ├─▶ db/
   ├─▶ miniredis/
   └─▶ e2e-tests/

2. cd frontend && npm install && npm run dev
```

### Production Build

```
1. cargo build --workspace --release
2. cd frontend && npm run build
3. scripts/build.sh (creates distribution package)
```

## 🧪 Testing Map

| Test Type | Location | Command |
|-----------|----------|---------|
| Protocol Tests | `protocol/src/lib.rs` | `cargo test -p protocol` |
| Daemon Tests | `daemon/src/main.rs` | `cargo test -p daemon` |
| Miniredis Tests | `miniredis/src/lib.rs` | `cargo test -p miniredis` |
| Integration Tests | `e2e-tests/src/lib.rs` | `cargo test -p e2e-tests` |
| All Tests | Root | `cargo test --workspace` |

## 📖 Documentation Map

### User Documentation
- Installation → `../install.md`
- Quick Start → `../../README.md`
- Tutorials → `../tutorials.md`
- API Reference → `../api-reference.md`
- CLI Reference → `../cli-reference.md`

### Developer Documentation
- Architecture → `../architecture.md`
- Development Guide → `DEVELOPMENT.md`
- Contributing → `CONTRIBUTING.md`
- Module READMEs → `../../<module>/README.md`

### Project Management
- Roadmap → `../../implementation_plan.md`
- Changelog → `CHANGELOG.md`
- Completed Work → `COMPLETED_WORK.md`
- Project Summary → `SUMMARY.md`

## 🚦 Status Legend

- ⭐ Essential reading
- 📖 Detailed documentation
- 🛠️ Development tools
- 🔧 Configuration
- 📦 Package/Module
- 🎨 Frontend code
- 🦀 Rust code
- 🐳 Docker/Infrastructure
- 📊 Project management

---

**Need help finding something? Check the READMEs or open an issue!**
