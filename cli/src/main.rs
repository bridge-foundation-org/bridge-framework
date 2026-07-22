//! Bridge CLI — `bridge <command> [args]`
//!
//! Talks to the daemon over TCP. The `init` command runs locally.
//!
//! # Usage
//!
//! ```text
//! bridge init <project-dir>              Scaffold a new Bridge project
//! bridge ping                            Check daemon health
//! bridge version                         Show daemon version
//! bridge health                          Full health report (JSON)
//! bridge mode-get                        Current mode
//! bridge mode-set <lite|full|ultra|off>  Change mode
//! bridge compile <source>                Compile Bridge DSL from string
//! bridge compile-file <path>             Compile .bridge file → TypeScript
//! bridge services                        List registered services
//! bridge routes                          List all routes
//! bridge auth-set <token>                Set auth token
//! bridge auth-clear                      Clear auth token
//! bridge auth-status                     Auth token status
//! bridge db-put <ns> <key> <value>       Store a value
//! bridge db-get <ns> <key>               Retrieve a value
//! bridge db-del <ns> <key>               Delete a value
//! bridge db-keys <ns>                    List keys in namespace
//! bridge db-flush <ns>                   Flush a namespace
//! bridge pg-create <name>                Create Postgres container
//! bridge pg-status                       Postgres container status
//! bridge pg-migrate <sql-file>           Run a SQL migration
//! bridge pg-destroy <name>               Remove Postgres container
//! bridge redis-status                    Miniredis status
//! bridge redis-ping                      Ping miniredis
//! bridge redis-get <key>                 Get a Redis key
//! bridge redis-set <key> <value>         Set a Redis key
//! bridge redis-del <key>                 Delete a Redis key
//! bridge redis-keys <pattern>            List Redis keys
//! bridge redis-flush                     Flush Redis DB
//! bridge trace-list                      List recent traces
//! bridge trace-clear                     Clear all traces
//! bridge stop                            Stop the daemon
//! ```

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::process;

use protocol::encode;

const DEFAULT_ADDR: &str = "127.0.0.1:7878";

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() { usage(); }

    let cmd = args[0].as_str();

    // Local-only commands
    if cmd == "init" {
        if args.len() != 2 { die("init requires exactly one argument: <project-dir>"); }
        match init_project(&args[1]) {
            Ok(()) => { println!("✓ Bridge project created at '{}'", args[1]); println!("  cd {} && cargo run -p daemon", args[1]); }
            Err(e) => die(&e),
        }
        return;
    }

    // Daemon commands
    let wire = build_command(cmd, &args[1..]);
    match send(&daemon_addr(), &wire) {
        Ok(raw) => print!("{}", format_output(&raw)),
        Err(e)  => { eprintln!("error: {e}"); process::exit(1); }
    }
}

// ── Command builder ───────────────────────────────────────────────────────────

fn build_command(cmd: &str, rest: &[String]) -> String {
    match cmd {
        "ping"         => "PING".into(),
        "version"      => "VERSION".into(),
        "health"       => "HEALTH".into(),
        "help"         => "HELP".into(),
        "stop"         => "STOP".into(),
        "mode-get"     => "MODE GET".into(),
        "mode-set"     => { need(rest, 1, "mode-set <lite|full|ultra|off>"); format!("MODE SET {}", rest[0]) }
        "compile"      => { need_min(rest, 1, "compile <source>"); format!("COMPILE {}", encode(&rest.join(" "))) }
        "compile-file" => { need(rest, 1, "compile-file <path>"); format!("COMPILE {}", encode(&read_file(&rest[0]))) }
        "services"     => "SERVICES LIST".into(),
        "routes"       => "ROUTES LIST".into(),
        "auth-status"  => "AUTH STATUS".into(),
        "auth-set"     => { need(rest, 1, "auth-set <token>"); format!("AUTH SET {}", encode(&rest[0])) }
        "auth-clear"   => "AUTH CLEAR".into(),
        "db-put"       => { need_min(rest, 3, "db-put <ns> <key> <value>"); format!("DB PUT {} {} {}", rest[0], rest[1], encode(&rest[2..]                .join(" "))) }
        "db-get"       => { need(rest, 2, "db-get <ns> <key>"); format!("DB GET {} {}", rest[0], rest[1]) }
        "db-del"       => { need(rest, 2, "db-del <ns> <key>"); format!("DB DEL {} {}", rest[0], rest[1]) }
        "db-keys"      => { need(rest, 1, "db-keys <ns>"); format!("DB KEYS {}", rest[0]) }
        "db-flush"     => { need(rest, 1, "db-flush <ns>"); format!("DB FLUSH {}", rest[0]) }
        "pg-create"    => { need(rest, 1, "pg-create <name>"); format!("PG CREATE {}", rest[0]) }
        "pg-status"    => "PG STATUS".into(),
        "pg-migrate"   => { need(rest, 1, "pg-migrate <sql-file>"); format!("PG MIGRATE {}", encode(&read_file(&rest[0]))) }
        "pg-destroy"   => { need(rest, 1, "pg-destroy <name>"); format!("PG DESTROY {}", rest[0]) }
        // legacy aliases
        "db-create"    => { need(rest, 1, "db-create <name>"); format!("PG CREATE {}", rest[0]) }
        "db-status"    => "PG STATUS".into(),
        "db-migrate"   => { need(rest, 1, "db-migrate <sql-file>"); format!("PG MIGRATE {}", encode(&read_file(&rest[0]))) }
        "db-destroy"   => { need(rest, 1, "db-destroy <name>"); format!("PG DESTROY {}", rest[0]) }
        "redis-status" => "REDIS STATUS".into(),
        "redis-ping"   => "REDIS PING".into(),
        "redis-flush"  => "REDIS FLUSH".into(),
        "redis-get"    => { need(rest, 1, "redis-get <key>"); format!("REDIS GET {}", rest[0]) }
        "redis-set"    => { need_min(rest, 2, "redis-set <key> <value>"); format!("REDIS SET {} {}", rest[0], encode(&rest[1..].join(" "))) }
        "redis-del"    => { need(rest, 1, "redis-del <key>"); format!("REDIS DEL {}", rest[0]) }
        "redis-keys"   => { need(rest, 1, "redis-keys <pattern>"); format!("REDIS KEYS {}", rest[0]) }
        "trace-list"   => "TRACE LIST".into(),
        "trace-clear"  => "TRACE CLEAR".into(),
        "raw"          => { need_min(rest, 1, "raw <command>"); rest.join(" ") }
        _ => { eprintln!("unknown command: {cmd}"); usage(); }
    }
}

// ── Output formatter ──────────────────────────────────────────────────────────

fn format_output(raw: &str) -> String {
    let t = raw.trim_end();
    if let Some(data) = t.strip_prefix("DATA ") {
        let decoded = protocol::decode(data).unwrap_or_else(|e| format!("decode error: {e}"));
        return format!("{decoded}\n");
    }
    if let Some(err) = t.strip_prefix("ERR ") {
        return format!("error: {err}\n");
    }
    format!("{t}\n")
}

// ── TCP helpers ───────────────────────────────────────────────────────────────

fn daemon_addr() -> String {
    env::var("BRIDGE_TCP_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string())
}

fn send(addr: &str, command: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| {
        format!("cannot connect to daemon at {addr}: {e}\n  → Start it with: cargo run -p daemon")
    })?;
    stream.write_all(format!("{command}\n").as_bytes()).map_err(|e| format!("write error: {e}"))?;
    stream.shutdown(Shutdown::Write).map_err(|e| format!("shutdown error: {e}"))?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp).map_err(|e| format!("read error: {e}"))?;
    Ok(resp)
}

// ── File helpers ──────────────────────────────────────────────────────────────

fn read_file(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| die(&format!("cannot read '{}': {e}", path)))
}

fn die(msg: &str) -> ! { eprintln!("error: {msg}"); process::exit(1); }

fn need(args: &[String], n: usize, usage_hint: &str) {
    if args.len() < n { die(&format!("usage: bridge {usage_hint}")); }
}
fn need_min(args: &[String], n: usize, usage_hint: &str) {
    if args.len() < n { die(&format!("usage: bridge {usage_hint}")); }
}

fn usage() -> ! {
    eprintln!("{USAGE}");
    process::exit(1);
}

const USAGE: &str = r#"bridge — type-safe backend services framework

USAGE
  bridge <command> [args]

COMMANDS
  init <dir>                  Scaffold a new Bridge project
  ping                        Check daemon is running
  version                     Show daemon version
  health                      Full health report (JSON)
  mode-get / mode-set <mode>  Get or set mode (lite|full|ultra|off)
  compile <src>               Compile Bridge DSL source
  compile-file <path>         Compile a .bridge file → TypeScript client
  services                    List registered services
  routes                      List all API routes

  auth-status / auth-set <token> / auth-clear

  db-put <ns> <key> <value>   Store a KV entry
  db-get <ns> <key>           Read a KV entry
  db-del <ns> <key>           Delete a KV entry
  db-keys <ns>                List all keys in a namespace
  db-flush <ns>               Remove all keys in a namespace

  pg-create <name>            Create a Postgres Docker container
  pg-status                   List Postgres containers
  pg-migrate <sql-file>       Run a SQL file
  pg-destroy <name>           Remove a Postgres container

  redis-status / redis-ping / redis-flush
  redis-get <key> / redis-set <key> <val> / redis-del <key>
  redis-keys <pattern>

  trace-list                  Show recent request traces
  trace-clear                 Clear all traces
  stop                        Stop the daemon

ENVIRONMENT
  BRIDGE_TCP_ADDR   TCP address of daemon (default: 127.0.0.1:7878)

DOCS
  https://github.com/yourusername/bridge-framework
"#;

// ── `bridge init` scaffolding ─────────────────────────────────────────────────

fn init_project(dir: &str) -> Result<(), String> {
    let root = PathBuf::from(dir);
    if root.exists() {
        return Err(format!("'{}' already exists", root.display()));
    }
    let frontend = root.join("frontend");
    fs::create_dir_all(frontend.join("src"))
        .and_then(|_| fs::create_dir_all(frontend.join("bridge.gen")))
        .map_err(|e| format!("mkdir failed: {e}"))?;

    wf(&root.join("app.bridge"), SAMPLE_BRIDGE)?;
    wf(&root.join("README.md"), README)?;
    wf(&frontend.join("package.json"), PKG_JSON)?;
    wf(&frontend.join("vite.config.ts"), VITE_CONFIG)?;
    wf(&frontend.join("tsconfig.json"), TSCONFIG)?;
    wf(&frontend.join("index.html"), INDEX_HTML)?;
    wf(&frontend.join("src").join("main.ts"), MAIN_TS)?;
    wf(&frontend.join("src").join("style.css"), STYLE_CSS)?;
    wf(&frontend.join("bridge.gen").join("client.ts"), GEN_CLIENT)?;
    Ok(())
}

fn wf(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("write '{}': {e}", path.display()))
}

// ── Scaffold templates ────────────────────────────────────────────────────────

const SAMPLE_BRIDGE: &str = "\
# Bridge DSL — define your services here
service hello
  auth none

endpoint ping   GET  /ping
endpoint echo   POST /echo
";

const README: &str = "\
# Bridge App

## Getting started

1. Start the daemon (from the bridge-framework repo):
   ```
   cargo run -p daemon
   ```

2. Generate the TypeScript client:
   ```
   bridge compile-file app.bridge > frontend/bridge.gen/client.ts
   ```

3. Run the frontend:
   ```
   cd frontend && npm install && npm run dev
   ```
";

const PKG_JSON: &str = r#"{
  "name": "bridge-app",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "gen": "bridge compile-file ../app.bridge > ./bridge.gen/client.ts"
  },
  "devDependencies": {
    "typescript": "^5.6.3",
    "vite": "^5.4.10"
  }
}
"#;

const VITE_CONFIG: &str = r#"import { defineConfig } from "vite";
import path from "path";

export default defineConfig({
  resolve: {
    alias: { "~bridge": path.resolve(__dirname, "./bridge.gen") },
  },
});
"#;

const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true
  }
}
"#;

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Bridge App</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
"#;

const MAIN_TS: &str = r##"import "./style.css";
import { createClient } from "~bridge/client";

const client = createClient("http://127.0.0.1:8787");
const app = document.querySelector<HTMLDivElement>("#app")!;

client.hello.ping()
  .then((r) => { app.innerHTML = `<pre>${r}</pre>`; })
  .catch((e) => { app.textContent = String(e); });
"##;

const STYLE_CSS: &str = r#"body { font-family: monospace; background: #0f172a; color: #e2e8f0; padding: 2rem; }
pre { background: #1e293b; border-radius: 8px; padding: 1rem; color: #34d399; }
"#;

const GEN_CLIENT: &str = r#"// Generated by bridge — re-run: bridge compile-file ../app.bridge > bridge.gen/client.ts
export function createClient(baseUrl: string) {
  return {
    hello: {
      async ping() {
        const r = await fetch(`${baseUrl}/ping`);
        if (!r.ok) throw new Error(`ping failed: ${r.status}`);
        return r.text();
      },
    },
  };
}
"#;
