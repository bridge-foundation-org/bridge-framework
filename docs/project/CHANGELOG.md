# Changelog

All notable changes to Bridge Framework will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Comprehensive documentation system with module-specific READMEs
- Docker Compose infrastructure setup (PostgreSQL, Redis, pgAdmin)
- Windows and Unix startup scripts (`start-dev.bat`, `start-dev.sh`)
- `.env.example` for configuration template
- CONTRIBUTING.md with detailed guidelines
- DEVELOPMENT.md with development workflows
- SUMMARY.md documenting modularization effort
- Enhanced npm scripts for development and Docker management
- Initial database setup script (`init-db.sql`)

### Changed
- Restructured implementation_plan.md to show completed vs planned features
- Enhanced package.json with comprehensive script collection
- Improved root README with architecture diagrams and feature comparison
- Updated project structure to be more contributor-friendly

### Documentation
- Added cli/README.md with command reference and architecture
- Added daemon/README.md with module breakdown and protocol details
- Added protocol/README.md with wire format and encoding details
- Added miniredis/README.md with RESP protocol and command guide

## [0.1.0] - Initial Release

### Added
- **Core Framework**
  - Protocol crate with Command/Response types
  - Daemon with TCP and HTTP servers
  - CLI tool with full command suite
  - Compiler for Bridge DSL parsing
  - Codegen for TypeScript client generation
  - In-memory key-value store (db crate)

- **Infrastructure**
  - Embedded miniredis server (Redis-compatible)
  - Docker PostgreSQL lifecycle management
  - Connection tracking and health monitoring

- **Frontend**
  - Dev dashboard with Encore-inspired UI
  - Overview view with stats and compiler
  - API Explorer for testing endpoints
  - Infrastructure view for DB and Redis management
  - Documentation viewer with markdown rendering
  - Tailwind CSS v4 styling

- **CLI Commands**
  - `init` — Create new project
  - `ping` — Health check
  - `compile` / `compile-file` — Compile Bridge DSL
  - `db-create` / `db-status` / `db-migrate` / `db-destroy` — Database management
  - `redis-status` — Redis monitoring
  - `mode-get` / `mode-set` — Daemon mode control

- **Documentation**
  - Installation guide
  - Architecture overview
  - Database management guide
  - Caching guide
  - Deployment guide
  - API reference
  - Tutorials
  - Benefits documentation

### Technical Details
- Pure Rust implementation (stdlib only, no tokio/serde)
- Thread-safe state management with Arc<Mutex<>>
- URL-encoded text protocol for CLI communication
- HTTP REST API for frontend
- RESP protocol implementation for Redis compatibility
- Docker container management via subprocess

### Security Notice
⚠️ **Not production-ready** — No authentication, no rate limiting, trusts all input. Designed for local development only.

---

## Version History Summary

- **[0.1.0]** — Initial release with core features
- **[Unreleased]** — Enhanced documentation and contributor experience

## Migration Guide

### From Pre-Documentation Version

If you cloned Bridge before the modularization:

1. **Pull latest changes:**
   ```bash
   git pull origin main
   ```

2. **Review new documentation:**
   - Read updated [README.md](README.md)
   - Check [DEVELOPMENT.md](DEVELOPMENT.md) for new workflows
   - Review module READMEs for architecture details

3. **Use new startup scripts:**
   ```bash
   # Windows
   start-dev.bat
   
   # Unix/Linux/Mac
   ./start-dev.sh
   ```

4. **Try Docker Compose (optional):**
   ```bash
   docker-compose up -d
   ```

5. **Update npm scripts:**
   ```bash
   npm install  # Install concurrently for dev:all script
   npm run dev:all
   ```

## Breaking Changes

### None Yet

All changes in v0.1.x are backwards compatible. Breaking changes will be clearly marked and will trigger a major version bump (v1.0.0).

## Deprecation Notices

### None

No features are currently deprecated.

## Future Plans

See [implementation_plan.md](implementation_plan.md) for detailed roadmap.

**Next Release (v0.2.0):**
- CI/CD with GitHub Actions
- Unit test coverage improvements
- Enhanced error handling
- Shell completion scripts

**Future (v0.3.0+):**
- Authentication system
- WebSocket support
- Pub/Sub messaging
- Hot reload
- Additional language codegen targets

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to contribute to this project.

## Support

- **Documentation**: [docs/](docs/)
- **Issues**: [GitHub Issues](https://github.com/yourusername/bridge-framework/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/bridge-framework/discussions)

---

**Note**: This changelog is maintained manually. Each release will document all notable changes, additions, and fixes.
