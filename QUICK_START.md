# Bridge Framework — Quick Start

Get up and running in 5 minutes!

## 🚀 One-Command Setup

### Windows
```bash
git clone https://github.com/yourusername/bridge-framework.git
cd bridge-framework
scripts\dev\start-dev.bat
```

### Unix/Linux/Mac
```bash
git clone https://github.com/yourusername/bridge-framework.git
cd bridge-framework
chmod +x scripts/dev/start-dev.sh
scripts/dev/start-dev.sh
```

### Alternative: NPM
```bash
git clone https://github.com/yourusername/bridge-framework.git
cd bridge-framework
npm install
npm run start       # Unix/Mac
npm run start:windows  # Windows
```

## 📚 Documentation

- **[README.md](README.md)** — Project overview and features
- **[docs/](docs/)** — User guides and tutorials
- **[docs/project/](docs/project/)** — Developer documentation

## 🛠️ Common Commands

```bash
# Start development
npm run dev:all         # Start daemon + frontend
npm run dev:daemon      # Daemon only
npm run dev:frontend    # Frontend only

# Docker infrastructure
npm run docker:up       # Start PostgreSQL + Redis
npm run docker:down     # Stop services

# Build & test
npm run build           # Build all Rust crates
npm run test            # Run all tests
npm run lint            # Check code quality
```

## 📖 Next Steps

1. **[Installation Guide](docs/install.md)** — Detailed setup
2. **[Architecture](docs/architecture.md)** — How it works
3. **[Tutorials](docs/tutorials.md)** — Step-by-step guides
4. **[Contributing](docs/project/CONTRIBUTING.md)** — Help improve Bridge

## 🆘 Need Help?

- **Development issues?** → [docs/project/DEVELOPMENT.md](docs/project/DEVELOPMENT.md)
- **Want to contribute?** → [docs/project/CONTRIBUTING.md](docs/project/CONTRIBUTING.md)
- **Lost in the code?** → [docs/project/PROJECT_MAP.md](docs/project/PROJECT_MAP.md)

---

**Made with ❤️ by the Bridge community**
