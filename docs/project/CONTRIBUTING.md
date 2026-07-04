# Contributing to Bridge Framework

Thank you for your interest in contributing to Bridge! This guide will help you get started.

## Quick Links

- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Architecture Overview](docs/architecture.md)
- [Implementation Plan](implementation_plan.md)

## Getting Started

### Prerequisites

- Rust 1.70+ with `cargo`
- Node.js 18+ with `npm`
- Docker (optional, for database features)
- Git

### Setting Up Your Development Environment

1. **Fork and Clone**

```bash
git clone https://github.com/yourusername/bridge-framework.git
cd bridge-framework
```

2. **Build the Project**

```bash
cargo build --workspace
```

3. **Run Tests**

```bash
cargo test --workspace
```

4. **Start the Daemon**

```bash
cargo run -p daemon
```

5. **Start the Frontend**

```bash
cd frontend
npm install
npm run dev
```

## Project Structure

Bridge is organized as a Rust workspace with multiple crates:

```
bridge-framework/
├── cli/              # Command-line interface
├── daemon/           # Backend server (TCP + HTTP)
├── protocol/         # Command/Response protocol
├── compiler/         # Bridge DSL parser
├── codegen/          # TypeScript client generator
├── db/               # In-memory key-value store
├── miniredis/        # Embedded Redis-compatible server
├── e2e-tests/        # Integration test suite
├── frontend/         # Dev dashboard (Vite + Tailwind)
└── docs/             # Documentation
```

### Module Ownership

Each module has a clear purpose and maintainer:

- **cli** — Simple, reads stdin/files, sends to daemon
- **daemon** — Modular server with state/tcp/http/sqldb modules
- **protocol** — Protocol types shared by CLI and daemon
- **compiler** — Parses `.bridge` DSL into AST
- **codegen** — Generates TypeScript from AST
- **db** — Thread-safe storage abstraction
- **miniredis** — RESP protocol + command handlers
- **frontend** — UI components and API client

## Coding Standards

### Rust Code

- **Style**: Run `cargo fmt` before committing
- **Lints**: Run `cargo clippy -- -D warnings` and fix all warnings
- **Dependencies**: Only `std` library — no external crates
- **Error handling**: Use `Result<T, String>` for recoverable errors
- **Documentation**: Add doc comments for public APIs

#### Example

```rust
/// Creates a new Docker Postgres container.
///
/// # Arguments
///
/// * `name` - Container name (must be unique)
///
/// # Returns
///
/// Container ID on success, error message on failure
pub fn create_container(name: &str) -> Result<String, String> {
    // Implementation
}
```

### TypeScript/Frontend Code

- **Style**: Use Prettier (configured in `.prettierrc`)
- **Type safety**: Enable `strict` mode in tsconfig.json
- **No external libraries**: Use native DOM APIs where possible
- **Consistent naming**: camelCase for functions/variables

### Documentation

- **Markdown**: Use GitHub-flavored markdown
- **Code examples**: Always test code snippets
- **Screenshots**: Include for UI changes
- **API docs**: Update when changing endpoints

## Making Changes

### Workflow

1. **Create a Branch**

```bash
git checkout -b feature/my-feature
```

2. **Make Your Changes**

- Write code
- Add tests
- Update documentation
- Run `cargo test --workspace`
- Run `cargo fmt` and `cargo clippy`

3. **Commit Your Changes**

```bash
git add .
git commit -m "feat: add new feature"
```

Use conventional commit messages:
- `feat:` — New feature
- `fix:` — Bug fix
- `docs:` — Documentation changes
- `refactor:` — Code refactoring
- `test:` — Adding tests
- `chore:` — Build/tooling changes

4. **Push and Create PR**

```bash
git push origin feature/my-feature
```

Then open a Pull Request on GitHub.

### Pull Request Guidelines

- **Title**: Clear, descriptive summary
- **Description**: Explain what, why, and how
- **Tests**: Include test coverage
- **Docs**: Update relevant documentation
- **Breaking changes**: Clearly mark and explain

#### PR Template

```markdown
## What does this PR do?

Brief description of the changes.

## Why?

Context and motivation for the changes.

## How was it tested?

- [ ] Unit tests added
- [ ] Integration tests pass
- [ ] Manual testing performed

## Checklist

- [ ] Code compiles and tests pass
- [ ] Documentation updated
- [ ] No clippy warnings
- [ ] Formatted with cargo fmt
```

## Testing

### Unit Tests

Add tests in the same file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_endpoint() {
        let result = parse("endpoint ping GET /ping");
        assert!(result.is_ok());
    }
}
```

### Integration Tests

Add to `e2e-tests/src/lib.rs`:

```rust
#[test]
fn test_full_compile_flow() {
    // Start daemon, send commands, verify responses
}
```

### Frontend Testing

Manual testing via dev dashboard is currently the primary method. Automated UI tests are planned.

## Common Contribution Areas

### Easy Issues (Good First Issue)

- Documentation improvements
- Error message clarity
- CLI help text
- Frontend UI polish
- Additional examples

### Medium Issues

- New protocol commands
- Additional codegen targets (Python, Go)
- Dashboard features
- Docker management enhancements

### Advanced Issues

- Performance optimization
- Security hardening
- Cross-platform support
- Advanced DSL features

## Code Review Process

1. **Automated Checks** — CI runs tests and linters
2. **Maintainer Review** — Core team reviews code quality
3. **Iteration** — Address feedback, push updates
4. **Approval** — Two approvals required for merge
5. **Merge** — Squash and merge with clean commit message

## Getting Help

- **Questions**: [Discussions](https://github.com/yourusername/bridge-framework/discussions)
- **Bugs**: [Issue Tracker](https://github.com/yourusername/bridge-framework/issues)
- **Design Decisions**: Reach out via issues before major changes

## Architecture Deep Dives

### Protocol Layer

The protocol crate defines commands and responses:

```rust
pub enum Command {
    Ping,
    Compile { source: String },
    DbCreate { name: String },
    // ...
}
```

Both CLI and daemon use this. Keep it simple and well-tested.

### Daemon Modularity

The daemon is split into modules:

- `state.rs` — Shared state (Arc<Mutex<State>>)
- `tcp.rs` — TCP server, line-oriented protocol
- `http.rs` — HTTP server, REST API
- `sqldb.rs` — Docker lifecycle management

Each module is self-contained. Changes to one shouldn't affect others.

### Codegen Pipeline

1. **Compiler** parses `.bridge` → AST
2. **Codegen** walks AST → generates TypeScript
3. **Result** stored in db with namespace "codegen", key "latest"

To add a new target (e.g., Python):
1. Create `codegen_python.rs`
2. Walk the AST
3. Emit Python client code

### Frontend Architecture

- **main.ts** — App shell, view routing, event binding
- **daemon-client.ts** — HTTP client for daemon API
- **docs.ts** — Markdown parser and doc content
- **style.css** — Tailwind + custom Encore-inspired styles

Views are server-side-like: `render() -> HTML string`, then `mount()` binds events.

## Release Process

1. Update version in `Cargo.toml` files
2. Update `CHANGELOG.md`
3. Tag release: `git tag v0.x.0`
4. Push: `git push --tags`
5. CI builds and publishes binaries

## License

By contributing, you agree that your contributions will be licensed under the MIT License.

## Recognition

Contributors are recognized in:
- GitHub contributors page
- Release notes
- Special mentions for significant contributions

---

**Thank you for contributing to Bridge! 🚀**
