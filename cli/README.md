# Bridge CLI

Command-line interface for interacting with the Bridge daemon.

## Overview

The CLI is a simple, single-binary tool that sends commands to the daemon over TCP. It handles:
- Project initialization
- Compilation and codegen
- Database management
- Daemon control
- Shell completions

## Architecture

```
┌─────────────┐
│ CLI Process │
└──────┬──────┘
       │ TCP (line protocol)
       ▼
┌─────────────┐
│   Daemon    │
└─────────────┘
```

The CLI:
1. Parses command-line arguments
2. Constructs a protocol command
3. Sends it over TCP to the daemon
4. Receives and formats the response

## Command Categories

### Project Management

```bash
bridge init <project-dir>                         # Create new Bridge project (default template)
bridge init <project-dir> --template rest-api-auth  # REST API with bearer auth, DB, rate limiting
```

Creates:
- `bridge.toml` — Project configuration (see [bridge.toml section](#bridgetoml))
- `app.bridge` — Service definition
- `README.md` — Quick start guide

**Templates:**

| Template | Description | Extra files |
|----------|-------------|-------------|
| `default` | Minimal hello-world service | — |
| `rest-api-auth` | REST API with bearer auth + PostgreSQL | `migrations/001_init.sql`, `.env.example` |

### Compilation

```bash
bridge compile <source>         # Compile inline source
bridge compile-file <path>      # Compile from file
```

Sends source to daemon, which:
1. Parses it with the compiler crate
2. Generates TypeScript with the codegen crate
3. Stores result in db
4. Returns generated code

### Database Management

```bash
bridge db-create <name>         # Create Postgres container
bridge db-status                # Check container status
bridge db-migrate <sql-file>    # Run SQL migration
bridge db-destroy <name>        # Stop and remove container
```

Requires Docker. The daemon manages containers via `docker` CLI.

### Daemon Control

```bash
bridge ping                     # Health check
bridge mode-get                 # Get current mode
bridge mode-set <mode>          # Set mode (lite|full|ultra|off)
bridge redis-status             # Check miniredis
```

### Shell Completions

```bash
bridge completions bash          # Print bash completion script
bridge completions zsh           # Print zsh completion script
bridge completions fish          # Print fish completion script
bridge completions powershell    # Print PowerShell completion script
```

See the [Shell Completions section](#shell-completions) for sourcing instructions.

### Low-Level

```bash
bridge raw <command>            # Send raw protocol command
```

For debugging and testing.

## bridge.toml

`bridge init` now creates a `bridge.toml` alongside `app.bridge`. This file configures the daemon's behaviour for the project and is read by the daemon on startup.

**Generated `bridge.toml`:**

```toml
# Bridge project configuration
# Full reference: https://github.com/yourusername/bridge-framework/docs/config.md

tcp_addr   = "127.0.0.1:7878"
http_addr  = "127.0.0.1:8787"
redis_addr = "127.0.0.1:6399"
log_level  = "info"

[watch]
# Enable hot-reload: recompile .bridge files on change and push SSE events
enabled       = true
poll_interval = 500     # milliseconds between file-system polls
dirs          = ["src"] # directories to watch recursively

[ratelimit]
# Token-bucket rate limiter applied to all HTTP routes
enabled  = true
capacity = 100.0        # maximum burst tokens
refill   = 10.0         # tokens refilled per second

[auth]
# Set enabled = true and export BRIDGE_API_KEY=<key> to protect all endpoints
enabled      = false
api_key_env  = "BRIDGE_API_KEY"
```

All values are optional — the daemon uses sensible defaults if `bridge.toml` is missing or a key is omitted. Environment variables (e.g. `BRIDGE_HTTP_ADDR`) override file values.

**Project directory after `bridge init myapp`:**

```
myapp/
├── bridge.toml                 # Daemon configuration  ← new
├── app.bridge                  # Service definition
├── README.md                   # Getting started guide
└── frontend/
    ├── package.json
    ├── tsconfig.json
    ├── vite.config.ts
    ├── index.html
    ├── src/
    │   ├── main.ts
    │   └── style.css
    └── bridge.gen/
        └── client.ts           # Placeholder client
```

## Shell Completions

The `bridge completions` command prints a completion script for your shell. Source it once (or add to your shell profile) to get tab-completion for all Bridge commands and flags.

### Bash

```bash
# Source in the current session
source <(bridge completions bash)

# Install permanently (add to ~/.bashrc)
bridge completions bash >> ~/.bashrc
source ~/.bashrc
```

Or for a system-wide install:

```bash
bridge completions bash | sudo tee /etc/bash_completion.d/bridge
```

### Zsh

```bash
# Source in the current session
source <(bridge completions zsh)

# Install permanently (add to ~/.zshrc)
bridge completions zsh >> ~/.zshrc
source ~/.zshrc
```

If you use `oh-my-zsh`, drop the script into the completions directory:

```bash
bridge completions zsh > "${fpath[1]}/_bridge"
```

Ensure `compinit` runs after this (it typically does via `oh-my-zsh`).

### Fish

```bash
# Source in the current session
bridge completions fish | source

# Install permanently
bridge completions fish > ~/.config/fish/completions/bridge.fish
```

Fish picks up files in `~/.config/fish/completions/` automatically; no extra step needed.

## Protocol

Commands are line-oriented text:

```
PING\n
COMPILE <url-encoded-source>\n
DB CREATE <name>\n
MODE SET full\n
```

Responses:

```
PONG\n
DATA <url-encoded-data>\n
OK <message>\n
ERR <error>\n
```

See [protocol/](../protocol/) for details.

## Configuration

The CLI reads environment variables:

- `BRIDGE_TCP_ADDR` — Daemon TCP address (default: `127.0.0.1:7878`)

Project-level daemon settings live in `bridge.toml` (read by the daemon, not the CLI directly).

Example:
```bash
export BRIDGE_TCP_ADDR=127.0.0.1:9999
bridge ping
```

## Error Handling

The CLI provides friendly error messages:

```bash
$ bridge compile-file missing.bridge
cannot read file missing.bridge: No such file or directory
```

```bash
$ bridge ping
cannot connect to daemon at 127.0.0.1:7878: Connection refused.
Start it with `cargo run -p daemon`.
```

## Full Command Reference

| Command | Arguments | Description |
|---------|-----------|-------------|
| `init` | `<project-dir>` | Scaffold a new Bridge project (creates `bridge.toml` + `app.bridge`) |
| `ping` | — | Health-check the daemon |
| `compile` | `<source>` | Compile inline DSL source |
| `compile-file` | `<path>` | Compile a `.bridge` file |
| `db-create` | `<name>` | Create a Postgres Docker container |
| `db-status` | — | Show container status |
| `db-migrate` | `<sql-file>` | Run a SQL migration file |
| `db-destroy` | `<name>` | Stop and remove a container |
| `mode-get` | — | Get current daemon mode |
| `mode-set` | `<mode>` | Set daemon mode (`lite`/`full`/`ultra`/`off`) |
| `redis-status` | — | Show miniredis connection info |
| `completions` | `bash\|zsh\|fish` | Print shell completion script |
| `raw` | `<command>` | Send a raw protocol command (debug) |

## Code Structure

### main.rs

Simple structure:

```rust
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    
    // Special cases that don't need the daemon
    match args[0].as_str() {
        "init"        => return init_project(&args[1]),
        "completions" => return print_completions(args.get(1)),
        _ => {}
    }
    
    // Parse args into protocol command
    let command = match args[0].as_str() {
        "ping"         => "PING".to_string(),
        "compile-file" => format!("COMPILE {}", escape(file_contents)),
        // ...
    };
    
    // Send to daemon and print result
    match send_command(&addr, &command) {
        Ok(response) => print!("{}", format_output(&response)),
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    }
}
```

### Key Functions

- `send_command(addr, cmd)` — TCP socket, send command, read response
- `format_cli_output(raw)` — Parse `DATA`/`OK`/`ERR` prefixes, pretty-print
- `init_project(dir)` — Create directory structure, write `bridge.toml`, and template files
- `print_completions(shell)` — Emit a completion script for the requested shell
- `escape(s)` / `unescape(s)` — URL encoding (protocol crate)

## Adding New Commands

1. Add to usage message in `print_usage_and_exit()`
2. Add match arm in `main()`
3. Construct protocol command string
4. Add the command to each completion script in `print_completions()`
5. (Optional) Update protocol crate if adding a new command type

Example: Adding `bridge test <file>`

```rust
"test" => {
    if args.len() != 2 {
        eprintln!("test requires a file path");
        process::exit(1);
    }
    let contents = fs::read_to_string(&args[1])?;
    format!("TEST {}", escape(&contents))
}
```

Then add `TEST` command to protocol crate and daemon handler.

## Testing

```bash
# Run unit tests
cargo test -p cli

# Manual testing (requires daemon)
cargo run -p daemon &
cargo run -p cli -- ping
cargo run -p cli -- compile "service hello\nendpoint ping GET /ping"

# Test completions output
cargo run -p cli -- completions bash
cargo run -p cli -- completions zsh
cargo run -p cli -- completions fish
```

## Dependencies

- Only Rust `std` library
- No external crates
- Uses `protocol` crate for escape/unescape

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

Common improvements:
- Progress indicators for long operations
- Colored output
- Interactive mode
- Watch mode (`bridge watch <file>`) that streams SSE events from the daemon

## License

MIT — see [LICENSE](../LICENSE).
