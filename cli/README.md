# Bridge CLI

Command-line interface for interacting with the Bridge daemon.

## Overview

The CLI is a simple, single-binary tool that sends commands to the daemon over TCP. It handles:
- Project initialization
- Compilation and codegen
- Database management
- Daemon control

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
bridge init <project-dir>   # Create new Bridge project
```

Creates:
- `bridge.app` — Service definition
- `frontend/` — Vite + Tailwind frontend
- `README.md` — Quick start guide

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

### Low-Level

```bash
bridge raw <command>            # Send raw protocol command
```

For debugging and testing.

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

## Project Init Templates

When you run `bridge init myapp`, it creates:

```
myapp/
├── bridge.app                  # Service definition
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

The frontend is pre-configured with:
- Vite for dev server and building
- Tailwind CSS v4
- TypeScript with strict mode
- Alias `~bridge` pointing to `bridge.gen/`

## Code Structure

### main.rs

Simple structure:

```rust
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    
    // Special case: init (doesn't need daemon)
    if args[0] == "init" {
        return init_project(&args[1]);
    }
    
    // Parse args into protocol command
    let command = match args[0].as_str() {
        "ping" => "PING".to_string(),
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
- `init_project(dir)` — Create directory structure and template files
- `escape(s)` / `unescape(s)` — URL encoding (protocol crate)

## Adding New Commands

1. Add to usage message in `print_usage_and_exit()`
2. Add match arm in `main()`
3. Construct protocol command string
4. (Optional) Update protocol crate if adding new command type

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
```

## Dependencies

- Only Rust `std` library
- No external crates
- Uses `protocol` crate for escape/unescape

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

Common improvements:
- Better error messages
- Shell completion scripts (bash, zsh, fish)
- Progress indicators for long operations
- Colored output
- Interactive mode

## License

MIT — see [LICENSE](../LICENSE).
