# Repository Organization

This document explains the file structure of the Bridge Framework repository.

## 📁 Root Directory (Clean!)

The root contains only essential files:

```
bridge-framework/
├── README.md                    # Project overview
├── QUICK_START.md               # Fast setup guide
├── CLAUDE.md                    # AI context
├── implementation_plan.md       # Roadmap
├── Cargo.toml                   # Rust workspace
├── package.json                 # NPM scripts
├── docker-compose.yml           # Infrastructure
├── .env.example                 # Config template
└── .gitignore                   # Git ignore rules
```

## 📚 Documentation Structure

All documentation is organized in `docs/`:

```
docs/
├── index.md                     # Documentation home
├── *.md                         # User-facing guides
│
└── project/                     # Developer docs
    ├── README.md                # Index of project docs
    ├── CONTRIBUTING.md          # Contribution guide
    ├── DEVELOPMENT.md           # Dev workflows
    ├── PROJECT_MAP.md           # Code navigation
    ├── CHANGELOG.md             # Version history
    ├── SUMMARY.md               # Modularization info
    └── COMPLETED_WORK.md        # Work summary
```

### User Documentation (docs/)
- `install.md` — Installation instructions
- `architecture.md` — System design
- `database.md` — PostgreSQL management
- `caching.md` — Miniredis guide
- `deployment.md` — Production deployment
- `api-reference.md` — HTTP API docs
- `cli-reference.md` — CLI commands
- `tutorials.md` — Step-by-step guides
- `benefits.md` — Why use Bridge

### Project Documentation (docs/project/)
- `CONTRIBUTING.md` — How to contribute
- `DEVELOPMENT.md` — Development setup
- `PROJECT_MAP.md` — Codebase navigation
- `CHANGELOG.md` — Version history
- `SUMMARY.md` — Project overview
- `COMPLETED_WORK.md` — Accomplishments

## 🔧 Scripts Organization

All scripts are in `scripts/`:

```
scripts/
├── check.bash                   # Quality checks (fmt, clippy, test)
├── dev/
│   ├── start-dev.bat            # Windows dev startup
│   └── start-dev.sh             # Unix dev startup
├── build.sh                     # Production build
└── deploy.sh                    # Deployment
```

## 🐳 Docker Configuration

Docker-related files are in `docker/`:

```
docker/
├── README.md                    # Docker setup guide
└── init-db.sql                  # PostgreSQL initialization
```

The main `docker-compose.yml` is in the root for convenience.

## 🦀 Code Organization

Source code is in focused modules:

```
├── cli/                         # Command-line tool
├── daemon/                      # Backend server
├── protocol/                    # Shared protocol
├── compiler/                    # DSL parser
├── codegen/                     # Code generator
├── db/                          # Storage
├── miniredis/                   # Cache server
├── e2e-tests/                   # Integration tests
└── frontend/                    # Dev dashboard
```

Each module has its own `README.md` explaining architecture and usage.

## 🎯 Design Principles

1. **Clean root** — Only essential files at top level
2. **Organized docs** — User vs developer documentation separated
3. **Module focus** — Each module is self-contained
4. **Easy navigation** — Clear structure, good signposting

## 📖 Finding Things

| I Want To... | Go To... |
|--------------|----------|
| Get started quickly | `QUICK_START.md` or `README.md` |
| Learn about Bridge | `docs/` directory |
| Contribute code | `docs/project/CONTRIBUTING.md` |
| Set up development | `docs/project/DEVELOPMENT.md` |
| Navigate codebase | `docs/project/PROJECT_MAP.md` |
| See version history | `docs/project/CHANGELOG.md` |
| Understand a module | `<module>/README.md` |
| Run dev environment | `scripts/dev/start-dev.*` |

## 🔄 Changes from Previous Structure

**Moved to `docs/project/`:**
- ✅ CONTRIBUTING.md
- ✅ DEVELOPMENT.md
- ✅ CHANGELOG.md
- ✅ SUMMARY.md
- ✅ COMPLETED_WORK.md
- ✅ PROJECT_MAP.md

**Moved to `scripts/`:**
- ✅ check.bash (from root)

**Moved to `docker/`:**
- ✅ init-db.sql (from root)

**Kept in root:**
- ✅ README.md (essential)
- ✅ QUICK_START.md (helpful)
- ✅ implementation_plan.md (roadmap)
- ✅ CLAUDE.md (AI context)
- ✅ docker-compose.yml (convenience)
- ✅ All config files (.toml, .json, .example, .gitignore)

## 🎉 Result

**Before:** 15+ files cluttering the root (markdown, scripts, SQL)
**After:** Only 4 markdown files + essential configs

The root is now clean and navigable!

---

For more information, see [PROJECT_MAP.md](../docs/project/PROJECT_MAP.md)
