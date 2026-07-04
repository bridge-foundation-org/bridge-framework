# Bridge Protocol

Shared protocol definitions used by CLI and daemon for communication.

## Overview

The protocol crate defines:
- **Commands** — Operations the daemon can perform
- **Responses** — Results returned to clients
- **Parsing** — Command string → structured type
- **Rendering** — Structured type → response string
- **Encoding** — URL-style encoding for newlines/spaces

## Architecture

```
┌─────────┐                  ┌─────────┐
│   CLI   │ ─── Command ───► │ Daemon  │
└─────────┘                  └─────────┘
            ◄── Response ───
```

Both sides use the same protocol crate for consistency.

## Command Types

```rust
pub enum Command {
    // Basic
    Ping,
    Help,
    Stop,
    
    // Mode
    GetMode,
    SetMode(String),
    
    // Compilation
    Compile { source: String },
    
    // Key-value storage
    DbPut { namespace: String, key: String, value: String },
    DbGet { namespace: String, key: String },
    
    // Docker Postgres
    DbCreate { name: String },
    DbStatus,
    DbMigrate { sql: String },
    DbDestroy { name: String },
    
    // Redis
    RedisStatus,
}
```

## Response Types

```rust
pub enum Response {
    Pong,
    Mode(String),
    Ok(String),
    Data(String),
    Error(String),
}
```

## Wire Format

Commands are line-delimited text:

```
PING\n
COMPILE <escaped-source>\n
DB CREATE mydb\n
MODE SET full\n
```

Responses:

```
PONG\n
DATA <escaped-data>\n
OK <message>\n
ERR <error>\n
MODE <mode>\n
```

## Encoding

Special characters are URL-encoded:

- Space → `%20`
- Newline → `%0A`
- Percent → `%25`

Example:

```
"service hello\nendpoint ping GET /ping"
→
"service%20hello%0Aendpoint%20ping%20GET%20/ping"
```

Functions:

```rust
pub fn escape(value: &str) -> String
pub fn unescape(value: &str) -> String
```

## Parsing

Convert wire format to structured type:

```rust
pub fn parse_command(line: &str) -> Result<Command, String>
```

Examples:

```rust
parse_command("PING")
// → Ok(Command::Ping)

parse_command("COMPILE service%20hello")
// → Ok(Command::Compile { source: "service hello".to_string() })

parse_command("DB CREATE mydb")
// → Ok(Command::DbCreate { name: "mydb".to_string() })

parse_command("INVALID")
// → Err("unknown command".to_string())
```

## Rendering

Convert structured type to wire format:

```rust
pub fn render_response(response: Response) -> String
```

Examples:

```rust
render_response(Response::Pong)
// → "PONG\n"

render_response(Response::Data("hello world".to_string()))
// → "DATA hello%20world\n"

render_response(Response::Error("not found".to_string()))
// → "ERR not found\n"
```

## Adding New Commands

1. **Add to enum:**

```rust
pub enum Command {
    // ...
    NewCommand { arg: String },
}
```

2. **Add parsing logic:**

```rust
pub fn parse_command(line: &str) -> Result<Command, String> {
    // ...
    if let Some(arg) = trimmed.strip_prefix("NEW ") {
        return Ok(Command::NewCommand {
            arg: arg.to_string(),
        });
    }
    // ...
}
```

3. **Add tests:**

```rust
#[test]
fn parse_new_command() {
    let parsed = parse_command("NEW foo").unwrap();
    assert_eq!(parsed, Command::NewCommand { arg: "foo".to_string() });
}
```

4. **Update daemon handler** (in daemon crate)

## Design Decisions

### Why Text-Based?

- **Simple** — Easy to debug with `nc` or `telnet`
- **Human-readable** — No binary parsing
- **Universal** — Works on any platform
- **Debuggable** — Can inspect with Wireshark, tcpdump

### Why URL Encoding?

- **Familiar** — Well-understood format
- **Simple** — Only 3 substitutions
- **Sufficient** — Handles all common cases

Alternatives considered:
- JSON — Too verbose, requires parser
- MessagePack — Binary, not human-readable
- Protocol Buffers — Overkill for local dev

### Why Line-Delimited?

- **Simple** — Just `BufReader::read_line()`
- **Stateless** — No connection tracking needed
- **Lightweight** — No framing overhead

## Testing

```bash
cargo test -p protocol
```

Tests cover:
- Parsing all command types
- Rendering all response types
- Escape/unescape round trips
- Error cases (invalid commands)

## Usage Example

### Client Side (CLI)

```rust
use protocol::{Command, parse_command, render_response, escape};

let source = "service hello\nendpoint ping GET /ping";
let cmd = format!("COMPILE {}", escape(source));

// Send over TCP
stream.write_all(format!("{cmd}\n").as_bytes())?;
```

### Server Side (Daemon)

```rust
use protocol::{Command, Response, parse_command, render_response};

let line = read_line_from_socket()?;
match parse_command(&line) {
    Ok(Command::Compile { source }) => {
        let result = compile_and_generate(&source)?;
        let response = Response::Data(result);
        write_response(render_response(response));
    }
    Err(e) => {
        write_response(render_response(Response::Error(e)));
    }
}
```

## Future Enhancements

- **Binary protocol option** — For high-performance use cases
- **Streaming** — For large responses (e.g., logs)
- **Compression** — For bandwidth-constrained environments
- **Authentication** — API keys or tokens
- **Versioning** — Protocol version negotiation

## Dependencies

- Only Rust `std` library
- No external crates

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md).

Common improvements:
- Additional command types
- Better error messages
- Protocol documentation generator
- Wire format validator

## License

MIT — see [LICENSE](../LICENSE).
