//! Bridge CLI — `bridge <command> [args]`
//!
//! Talks to the daemon over TCP. Some commands (init, completions) run locally.

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::process;

use protocol::encode;

const DEFAULT_ADDR: &str = "127.0.0.1:7878";
const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── ANSI colors (no external deps) ───────────────────────────────────────────

fn tty() -> bool {
    env::var("NO_COLOR").is_err() && env::var("CI").is_err()
}
fn bold(s: &str) -> String {
    if tty() {
        format!("\x1b[1m{s}\x1b[0m")
    } else {
        s.into()
    }
}
fn green(s: &str) -> String {
    if tty() {
        format!("\x1b[32m{s}\x1b[0m")
    } else {
        s.into()
    }
}
fn red(s: &str) -> String {
    if tty() {
        format!("\x1b[31m{s}\x1b[0m")
    } else {
        s.into()
    }
}
fn cyan(s: &str) -> String {
    if tty() {
        format!("\x1b[36m{s}\x1b[0m")
    } else {
        s.into()
    }
}
fn dim(s: &str) -> String {
    if tty() {
        format!("\x1b[2m{s}\x1b[0m")
    } else {
        s.into()
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    let json_out = raw_args.iter().any(|a| a == "--json");
    let args: Vec<String> = raw_args.into_iter().filter(|a| a != "--json").collect();

    if args.is_empty() {
        print_usage();
        return;
    }

    match args[0].as_str() {
        "--version" | "-V" => {
            println!("{VERSION}");
            return;
        }
        "--help" | "-h" | "help" => {
            print_usage();
            return;
        }
        "version" => {
            println!("{VERSION}");
            return;
        }
        "init" => {
            if args.len() < 2 {
                die("usage: bridge init <project-dir> [--template <default|rest-api-auth>]");
            }
            let dir = &args[1];
            let template = args
                .windows(2)
                .find(|w| w[0] == "--template")
                .map(|w| w[1].as_str())
                .unwrap_or("default");
            match init_project(dir, template) {
                Ok(()) => {
                    println!("{} Bridge project created: {}", green("✓"), bold(dir));
                    println!("  {} cd {} && cargo run -p daemon", dim("→"), dir);
                }
                Err(e) => die(&e),
            }
            return;
        }
        "completions" => {
            let shell = args.get(1).map(|s| s.as_str()).unwrap_or("bash");
            print_completions(shell);
            return;
        }
        _ => {}
    }

    let wire = build_command(args[0].as_str(), &args[1..]);
    match send(&daemon_addr(), &wire) {
        Ok(raw) => print!(
            "{}",
            if json_out {
                fmt_json(&raw)
            } else {
                fmt_output(args[0].as_str(), &raw)
            }
        ),
        Err(e) => {
            eprintln!("{} {e}", red("error:"));
            process::exit(1);
        }
    }
}

// ── Command builder ───────────────────────────────────────────────────────────

fn build_command(cmd: &str, rest: &[String]) -> String {
    match cmd {
        "ping" => "PING".into(),
        "version" => "VERSION".into(),
        "health" => "HEALTH".into(),
        "help" => "HELP".into(),
        "stop" => "STOP".into(),
        "mode-get" => "MODE GET".into(),
        "mode-set" => {
            need(rest, 1, "mode-set <lite|full|ultra|off>");
            format!("MODE SET {}", rest[0])
        }
        "compile" => {
            need_min(rest, 1, "compile <source>");
            format!("COMPILE {}", encode(&rest.join(" ")))
        }
        "compile-file" => {
            need(rest, 1, "compile-file <path>");
            format!("COMPILE {}", encode(&read_file(&rest[0])))
        }
        "services" => "SERVICES LIST".into(),
        "routes" => "ROUTES LIST".into(),
        "auth-status" => "AUTH STATUS".into(),
        "auth-set" => {
            need_min(rest, 1, "auth-set [bearer|api_key] <token>");
            format!("AUTH SET {}", rest.join(" "))
        }
        "auth-clear" => "AUTH CLEAR".into(),
        "db-put" => {
            need_min(rest, 3, "db-put <ns> <key> <value>");
            format!(
                "DB PUT {} {} {}",
                rest[0],
                rest[1],
                encode(&rest[2..].join(" "))
            )
        }
        "db-get" => {
            need(rest, 2, "db-get <ns> <key>");
            format!("DB GET {} {}", rest[0], rest[1])
        }
        "db-del" => {
            need(rest, 2, "db-del <ns> <key>");
            format!("DB DEL {} {}", rest[0], rest[1])
        }
        "db-keys" => {
            need(rest, 1, "db-keys <ns>");
            format!("DB KEYS {}", rest[0])
        }
        "db-flush" => {
            need(rest, 1, "db-flush <ns>");
            format!("DB FLUSH {}", rest[0])
        }
        "pg-create" | "db-create" => {
            need(rest, 1, "pg-create <name>");
            format!("PG CREATE {}", rest[0])
        }
        "pg-status" | "db-status" => "PG STATUS".into(),
        "pg-migrate" | "db-migrate" => {
            need(rest, 1, "pg-migrate <sql-file>");
            format!("PG MIGRATE {}", encode(&read_file(&rest[0])))
        }
        "pg-destroy" | "db-destroy" => {
            need(rest, 1, "pg-destroy <name>");
            format!("PG DESTROY {}", rest[0])
        }
        "redis-status" => "REDIS STATUS".into(),
        "redis-ping" => "REDIS PING".into(),
        "redis-flush" => "REDIS FLUSH".into(),
        "redis-get" => {
            need(rest, 1, "redis-get <key>");
            format!("REDIS GET {}", rest[0])
        }
        "redis-set" => {
            need_min(rest, 2, "redis-set <key> <value>");
            format!("REDIS SET {} {}", rest[0], encode(&rest[1..].join(" ")))
        }
        "redis-del" => {
            need(rest, 1, "redis-del <key>");
            format!("REDIS DEL {}", rest[0])
        }
        "redis-keys" => format!(
            "REDIS KEYS {}",
            rest.first().map(|s| s.as_str()).unwrap_or("*")
        ),
        "trace-list" => {
            let l = rest.first().map(|s| format!(" {s}")).unwrap_or_default();
            format!("TRACE LIST{l}")
        }
        "trace-get" => {
            need(rest, 1, "trace-get <id>");
            format!("TRACE GET {}", rest[0])
        }
        "trace-clear" => "TRACE CLEAR".into(),
        "trace-export" => format!(
            "TRACE EXPORT {}",
            rest.first().map(|s| s.as_str()).unwrap_or("json")
        ),
        "metrics" => "METRICS LIST".into(),
        "metrics-clear" => "METRICS CLEAR".into(),
        "raw" => {
            need_min(rest, 1, "raw <command>");
            rest.join(" ")
        }
        other => {
            eprintln!("{} unknown command: {}", red("error:"), bold(other));
            eprintln!("  {} run {} for help", dim("tip:"), bold("bridge help"));
            process::exit(1);
        }
    }
}

// ── Output formatter ──────────────────────────────────────────────────────────

fn fmt_output(cmd: &str, raw: &str) -> String {
    let t = raw.trim_end();
    if t == "PONG" {
        return format!("{}\n", green("✓ daemon is alive"));
    }
    if let Some(d) = t.strip_prefix("DATA ") {
        let decoded = protocol::decode(d).unwrap_or_else(|e| format!("(decode error: {e})"));
        return match cmd {
            "health" | "redis-status" | "auth-status" | "metrics" => {
                format!("{}\n", pretty_json(&decoded))
            }
            "trace-list" | "routes" | "services" => format!("{}\n", pretty_list(&decoded)),
            _ => format!("{decoded}\n"),
        };
    }
    if let Some(ok) = t.strip_prefix("OK ") {
        return format!("{} {ok}\n", green("✓"));
    }
    if let Some(err) = t.strip_prefix("ERR ") {
        return format!("{} {err}\n", red("✗"));
    }
    if let Some(mode) = t.strip_prefix("MODE ") {
        return format!("mode: {}\n", cyan(mode));
    }
    format!("{t}\n")
}

fn fmt_json(raw: &str) -> String {
    let t = raw.trim_end();
    if let Some(d) = t.strip_prefix("DATA ") {
        return protocol::decode(d).unwrap_or_else(|e| format!("decode error: {e}")) + "\n";
    }
    format!("{t}\n")
}

fn pretty_json(s: &str) -> String {
    let s = s.trim();
    if !s.starts_with('{') && !s.starts_with('[') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for ch in s.chars() {
        if esc {
            out.push(ch);
            esc = false;
            continue;
        }
        if ch == '\\' && in_str {
            out.push(ch);
            esc = true;
            continue;
        }
        if ch == '"' {
            in_str = !in_str;
            out.push(ch);
            continue;
        }
        if in_str {
            out.push(ch);
            continue;
        }
        match ch {
            '{' | '[' => {
                depth += 1;
                out.push(ch);
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push(ch);
            }
            ',' => {
                out.push(ch);
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
            }
            ':' => {
                out.push_str(": ");
            }
            _ => {
                out.push(ch);
            }
        }
    }
    out
}

fn pretty_list(s: &str) -> String {
    let s = s.trim();
    if !s.starts_with('[') {
        return s.to_string();
    }
    let inner = s.trim_start_matches('[').trim_end_matches(']');
    if inner.trim().is_empty() {
        return dim("(empty)").to_string();
    }
    inner
        .split("},{")
        .map(|i| format!("  {} {}\n", cyan("•"), i.trim().trim_matches(['{', '}'])))
        .collect()
}

// ── Shell completions ─────────────────────────────────────────────────────────

fn print_completions(shell: &str) {
    match shell {
        "bash" => print!("{BASH_COMPLETION}"),
        "zsh" => print!("{ZSH_COMPLETION}"),
        "fish" => print!("{FISH_COMPLETION}"),
        "powershell" | "pwsh" => print!("{POWERSHELL_COMPLETION}"),
        other => {
            eprintln!(
                "{} unknown shell: {other} (use: bash zsh fish powershell)",
                red("error:")
            );
            process::exit(1);
        }
    }
}

const ALL_CMDS: &str = "init ping version health help stop mode-get mode-set compile compile-file \
services routes auth-status auth-set auth-clear db-put db-get db-del db-keys db-flush \
pg-create pg-status pg-migrate pg-destroy redis-status redis-ping redis-get redis-set \
redis-del redis-keys redis-flush trace-list trace-get trace-clear trace-export \
metrics metrics-clear completions raw";

const BASH_COMPLETION: &str = concat!(
"# Bridge CLI bash completion — add to ~/.bashrc:\n",
"#   source <(bridge completions bash)\n",
"_bridge() {\n",
"  local cur=\"${COMP_WORDS[COMP_CWORD]}\"\n",
"  COMPREPLY=($(compgen -W '", "init ping version health help stop mode-get mode-set compile compile-file services routes auth-status auth-set auth-clear db-put db-get db-del db-keys db-flush pg-create pg-status pg-migrate pg-destroy redis-status redis-ping redis-get redis-set redis-del redis-keys redis-flush trace-list trace-get trace-clear metrics metrics-clear completions", "' -- \"$cur\"))\n",
"}\n",
"complete -F _bridge bridge\n"
);

const ZSH_COMPLETION: &str = "#compdef bridge
# Add to ~/.zshrc: source <(bridge completions zsh)
_bridge() {
  local -a cmds
  cmds=(
    'init:Scaffold project' 'ping:Check daemon' 'health:Health report'
    'version:Show version' 'stop:Stop daemon' 'mode-get:Get mode' 'mode-set:Set mode'
    'compile:Compile DSL' 'compile-file:Compile file' 'services:List services' 'routes:List routes'
    'auth-set:Set auth token' 'auth-clear:Clear token' 'auth-status:Token status'
    'db-put:KV store' 'db-get:KV get' 'db-del:KV delete' 'db-keys:KV list' 'db-flush:KV flush'
    'pg-create:Create DB' 'pg-status:DB status' 'pg-migrate:Run migration' 'pg-destroy:Destroy DB'
    'redis-status:Redis status' 'redis-ping:Ping Redis' 'redis-get:Get key' 'redis-set:Set key' 'redis-del:Del key'
    'trace-list:List traces' 'trace-clear:Clear traces' 'metrics:Show metrics' 'completions:Shell completions'
  )
  _describe 'bridge command' cmds
}
_bridge \"$@\"
";

const FISH_COMPLETION: &str = "# Bridge CLI fish completion
# Save to: ~/.config/fish/completions/bridge.fish
for cmd in init ping version health stop mode-get mode-set compile compile-file services routes \
  auth-status auth-set auth-clear db-put db-get db-del db-keys db-flush \
  pg-create pg-status pg-migrate pg-destroy redis-status redis-ping redis-get redis-set redis-del \
  redis-keys redis-flush trace-list trace-get trace-clear metrics metrics-clear completions raw
  complete -c bridge -f -a $cmd
end
complete -c bridge -n '__fish_seen_subcommand_from mode-set' -a 'lite full ultra off'
complete -c bridge -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish powershell'
";

const POWERSHELL_COMPLETION: &str = r#"# Bridge CLI PowerShell completion
# Add to your $PROFILE:
#   bridge completions powershell | Out-String | Invoke-Expression
#
# Or save to a file and dot-source it:
#   bridge completions powershell > $HOME\bridge_completion.ps1
#   . $HOME\bridge_completion.ps1

Register-ArgumentCompleter -Native -CommandName bridge -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commands = @(
        [System.Management.Automation.CompletionResult]::new('init',          'init',          'ParameterValue', 'Scaffold a new project')
        [System.Management.Automation.CompletionResult]::new('ping',          'ping',          'ParameterValue', 'Check daemon health')
        [System.Management.Automation.CompletionResult]::new('health',        'health',        'ParameterValue', 'Full health report')
        [System.Management.Automation.CompletionResult]::new('version',       'version',       'ParameterValue', 'Show version')
        [System.Management.Automation.CompletionResult]::new('stop',          'stop',          'ParameterValue', 'Stop the daemon')
        [System.Management.Automation.CompletionResult]::new('mode-get',      'mode-get',      'ParameterValue', 'Get current mode')
        [System.Management.Automation.CompletionResult]::new('mode-set',      'mode-set',      'ParameterValue', 'Set daemon mode')
        [System.Management.Automation.CompletionResult]::new('compile',       'compile',       'ParameterValue', 'Compile Bridge DSL inline')
        [System.Management.Automation.CompletionResult]::new('compile-file',  'compile-file',  'ParameterValue', 'Compile .bridge file')
        [System.Management.Automation.CompletionResult]::new('services',      'services',      'ParameterValue', 'List registered services')
        [System.Management.Automation.CompletionResult]::new('routes',        'routes',        'ParameterValue', 'List all routes')
        [System.Management.Automation.CompletionResult]::new('auth-set',      'auth-set',      'ParameterValue', 'Set auth token')
        [System.Management.Automation.CompletionResult]::new('auth-clear',    'auth-clear',    'ParameterValue', 'Clear auth token')
        [System.Management.Automation.CompletionResult]::new('auth-status',   'auth-status',   'ParameterValue', 'Show auth status')
        [System.Management.Automation.CompletionResult]::new('db-put',        'db-put',        'ParameterValue', 'Store a KV value')
        [System.Management.Automation.CompletionResult]::new('db-get',        'db-get',        'ParameterValue', 'Get a KV value')
        [System.Management.Automation.CompletionResult]::new('db-del',        'db-del',        'ParameterValue', 'Delete a KV value')
        [System.Management.Automation.CompletionResult]::new('db-keys',       'db-keys',       'ParameterValue', 'List KV keys')
        [System.Management.Automation.CompletionResult]::new('db-flush',      'db-flush',      'ParameterValue', 'Flush a namespace')
        [System.Management.Automation.CompletionResult]::new('pg-create',     'pg-create',     'ParameterValue', 'Create Postgres container')
        [System.Management.Automation.CompletionResult]::new('pg-status',     'pg-status',     'ParameterValue', 'Postgres container status')
        [System.Management.Automation.CompletionResult]::new('pg-migrate',    'pg-migrate',    'ParameterValue', 'Run SQL migration')
        [System.Management.Automation.CompletionResult]::new('pg-destroy',    'pg-destroy',    'ParameterValue', 'Remove Postgres container')
        [System.Management.Automation.CompletionResult]::new('redis-status',  'redis-status',  'ParameterValue', 'Miniredis status')
        [System.Management.Automation.CompletionResult]::new('redis-ping',    'redis-ping',    'ParameterValue', 'Ping miniredis')
        [System.Management.Automation.CompletionResult]::new('redis-get',     'redis-get',     'ParameterValue', 'Get a Redis key')
        [System.Management.Automation.CompletionResult]::new('redis-set',     'redis-set',     'ParameterValue', 'Set a Redis key')
        [System.Management.Automation.CompletionResult]::new('redis-del',     'redis-del',     'ParameterValue', 'Delete a Redis key')
        [System.Management.Automation.CompletionResult]::new('redis-keys',    'redis-keys',    'ParameterValue', 'List Redis keys')
        [System.Management.Automation.CompletionResult]::new('redis-flush',   'redis-flush',   'ParameterValue', 'Flush all Redis keys')
        [System.Management.Automation.CompletionResult]::new('trace-list',    'trace-list',    'ParameterValue', 'List recent traces')
        [System.Management.Automation.CompletionResult]::new('trace-get',     'trace-get',     'ParameterValue', 'Get a specific trace')
        [System.Management.Automation.CompletionResult]::new('trace-clear',   'trace-clear',   'ParameterValue', 'Clear all traces')
        [System.Management.Automation.CompletionResult]::new('trace-export',  'trace-export',  'ParameterValue', 'Export traces')
        [System.Management.Automation.CompletionResult]::new('metrics',       'metrics',       'ParameterValue', 'Show metrics summary')
        [System.Management.Automation.CompletionResult]::new('metrics-clear', 'metrics-clear', 'ParameterValue', 'Reset metrics')
        [System.Management.Automation.CompletionResult]::new('completions',   'completions',   'ParameterValue', 'Print shell completion script')
        [System.Management.Automation.CompletionResult]::new('raw',           'raw',           'ParameterValue', 'Send raw TCP command')
    )

    # Sub-completions for specific commands
    $tokens = $commandAst.CommandElements
    if ($tokens.Count -ge 2) {
        $subCmd = $tokens[1].ToString()
        switch ($subCmd) {
            'mode-set' {
                return @('lite','full','ultra','off') |
                    Where-Object { $_ -like "$wordToComplete*" } |
                    ForEach-Object {
                        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', "mode: $_")
                    }
            }
            'completions' {
                return @('bash','zsh','fish','powershell') |
                    Where-Object { $_ -like "$wordToComplete*" } |
                    ForEach-Object {
                        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', "$_ shell completions")
                    }
            }
            'trace-export' {
                return @('json','csv','text') |
                    Where-Object { $_ -like "$wordToComplete*" } |
                    ForEach-Object {
                        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', "export format: $_")
                    }
            }
            'auth-set' {
                if ($tokens.Count -eq 2) {
                    return @('bearer','api_key') |
                        Where-Object { $_ -like "$wordToComplete*" } |
                        ForEach-Object {
                            [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', "auth scheme: $_")
                        }
                }
            }
        }
    }

    # Top-level command completion
    $commands | Where-Object { $_.CompletionText -like "$wordToComplete*" }
}
"#;

// ── TCP helpers ───────────────────────────────────────────────────────────────

fn daemon_addr() -> String {
    env::var("BRIDGE_TCP_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string())
}

fn send(addr: &str, cmd: &str) -> Result<String, String> {
    let mut s = TcpStream::connect(addr).map_err(|e| {
        format!(
            "cannot connect to daemon at {addr}: {e}\n  {} start with: {}",
            dim("→"),
            bold("cargo run -p daemon")
        )
    })?;
    s.write_all(format!("{cmd}\n").as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    s.shutdown(Shutdown::Write)
        .map_err(|e| format!("shutdown: {e}"))?;
    let mut resp = String::new();
    s.read_to_string(&mut resp)
        .map_err(|e| format!("read: {e}"))?;
    Ok(resp)
}

// ── File / error helpers ──────────────────────────────────────────────────────

fn read_file(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| die(&format!("cannot read '{path}': {e}")))
}

fn die(msg: &str) -> ! {
    eprintln!("{} {msg}", red("error:"));
    process::exit(1);
}

fn need(args: &[String], n: usize, hint: &str) {
    if args.len() < n {
        die(&format!("usage: bridge {hint}"));
    }
}
fn need_min(args: &[String], n: usize, hint: &str) {
    if args.len() < n {
        die(&format!("usage: bridge {hint}"));
    }
}

// ── Project scaffold ──────────────────────────────────────────────────────────

fn init_project(dir: &str, template: &str) -> Result<(), String> {
    let p = std::path::Path::new(dir);
    if p.exists() {
        return Err(format!("'{dir}' already exists"));
    }
    fs::create_dir_all(p).map_err(|e| format!("create dir: {e}"))?;
    match template {
        "rest-api-auth" => init_rest_api_auth(p, dir),
        "default" | _ => init_default(p, dir),
    }
}

// ── Template: default (minimal) ──────────────────────────────────────────────

fn init_default(p: &std::path::Path, dir: &str) -> Result<(), String> {
    // app.bridge
    fs::write(p.join("app.bridge"),
        "# My Bridge application\n\nservice hello\n  endpoint ping GET /ping\n  endpoint echo POST /echo\n"
    ).map_err(|e| format!("write app.bridge: {e}"))?;

    // bridge.toml
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or(dir);
    fs::write(p.join("bridge.toml"), default_bridge_toml(name))
        .map_err(|e| format!("write bridge.toml: {e}"))?;

    // README.md
    fs::write(p.join("README.md"),
        format!("# {dir}\n\nBridge Framework application.\n\n```bash\ncargo run -p daemon\nbridge compile-file app.bridge\nbridge ping\n```\n\nSee `bridge.toml` for project configuration.\n")
    ).map_err(|e| format!("write README: {e}"))?;

    Ok(())
}

// ── Template: rest-api-auth (REST API with bearer auth + Postgres) ────────────

fn init_rest_api_auth(p: &std::path::Path, dir: &str) -> Result<(), String> {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or(dir);

    // app.bridge — multi-service with auth and DB-backed routes
    fs::write(p.join("app.bridge"), REST_API_AUTH_BRIDGE)
        .map_err(|e| format!("write app.bridge: {e}"))?;

    // bridge.toml
    fs::write(p.join("bridge.toml"), rest_api_auth_toml(name))
        .map_err(|e| format!("write bridge.toml: {e}"))?;

    // migrations/
    fs::create_dir_all(p.join("migrations")).map_err(|e| format!("mkdir migrations: {e}"))?;
    fs::write(
        p.join("migrations").join("001_init.sql"),
        REST_API_AUTH_MIGRATION,
    )
    .map_err(|e| format!("write migrations/001_init.sql: {e}"))?;

    // .env.example
    fs::write(p.join(".env.example"), REST_API_AUTH_ENV)
        .map_err(|e| format!("write .env.example: {e}"))?;

    // README.md
    fs::write(p.join("README.md"), rest_api_auth_readme(dir))
        .map_err(|e| format!("write README.md: {e}"))?;

    Ok(())
}

const REST_API_AUTH_BRIDGE: &str = r#"# REST API with Bearer auth and PostgreSQL
# Generated by: bridge init <dir> --template rest-api-auth

# ── Public endpoints (no auth required) ──────────────────────────────────────
service public
endpoint health GET /health
endpoint version GET /api/version

# ── Auth service (issues and validates tokens) ────────────────────────────────
service auth
endpoint login  POST /auth/login
endpoint logout POST /auth/logout
endpoint refresh POST /auth/refresh

# ── Users service (requires bearer token) ────────────────────────────────────
service users
auth bearer
middleware log rate_limit
endpoint list   GET    /api/v1/users
endpoint get    GET    /api/v1/users/:id
endpoint create POST   /api/v1/users
endpoint update PUT    /api/v1/users/:id
endpoint delete DELETE /api/v1/users/:id
endpoint me     GET    /api/v1/users/me

# ── Items service (requires bearer token) ────────────────────────────────────
service items
auth bearer
middleware log
endpoint list   GET    /api/v1/items
endpoint get    GET    /api/v1/items/:id
endpoint create POST   /api/v1/items
endpoint update PUT    /api/v1/items/:id
endpoint delete DELETE /api/v1/items/:id
endpoint search GET    /api/v1/items/search
"#;

const REST_API_AUTH_MIGRATION: &str = r#"-- 001_init.sql — Initial schema
-- Run with: bridge pg-migrate migrations/001_init.sql

CREATE SCHEMA IF NOT EXISTS app;

-- Users
CREATE TABLE IF NOT EXISTS app.users (
    id         SERIAL PRIMARY KEY,
    email      TEXT UNIQUE NOT NULL,
    name       TEXT NOT NULL,
    role       TEXT NOT NULL DEFAULT 'user',   -- 'user' | 'admin'
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Auth tokens
CREATE TABLE IF NOT EXISTS app.tokens (
    id         SERIAL PRIMARY KEY,
    user_id    INTEGER REFERENCES app.users(id) ON DELETE CASCADE,
    token      TEXT UNIQUE NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Items
CREATE TABLE IF NOT EXISTS app.items (
    id          SERIAL PRIMARY KEY,
    owner_id    INTEGER REFERENCES app.users(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    metadata    JSONB DEFAULT '{}',
    created_at  TIMESTAMPTZ DEFAULT NOW(),
    updated_at  TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_tokens_token   ON app.tokens(token);
CREATE INDEX IF NOT EXISTS idx_tokens_user    ON app.tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_items_owner    ON app.items(owner_id);

-- Sample data
INSERT INTO app.users (email, name, role) VALUES
    ('admin@example.com', 'Admin User', 'admin'),
    ('alice@example.com', 'Alice', 'user')
ON CONFLICT (email) DO NOTHING;
"#;

const REST_API_AUTH_ENV: &str = r#"# .env.example — copy to .env and customise

BRIDGE_TCP_ADDR=127.0.0.1:7878
BRIDGE_HTTP_ADDR=127.0.0.1:8787
BRIDGE_REDIS_ADDR=127.0.0.1:6399
BRIDGE_MODE=full

# PostgreSQL
POSTGRES_USER=bridge
POSTGRES_PASSWORD=bridge
POSTGRES_DB=bridge_dev
POSTGRES_PORT=5432
DATABASE_URL=postgres://bridge:bridge@localhost:5432/bridge_dev

# Auth
BRIDGE_AUTH_SCHEME=bearer
BRIDGE_AUTH_TOKEN=change-me-before-production
"#;

fn rest_api_auth_toml(name: &str) -> String {
    format!(
        r#"# bridge.toml — REST API with Auth + DB

[project]
name    = "{name}"
version = "0.1.0"

[daemon]
http_addr  = "127.0.0.1:8787"
tcp_addr   = "127.0.0.1:7878"
redis_addr = "127.0.0.1:6399"
mode       = "full"

[watch]
enabled = true
poll_ms = 500
dirs    = ["."]
files   = ["app.bridge"]

[[middleware.rules]]
name   = "log"
scope  = "global"
before = "log"

[[middleware.rules]]
name   = "powered-by"
scope  = "global"
after  = "header:X-Powered-By:bridge"

# Rate limiting — 120 req/min on the public API
[[ratelimit.rules]]
method      = "*"
path        = "/api/*"
capacity    = 120
refill_rate = 2.0

# Tighter limit on auth endpoints
[[ratelimit.rules]]
method      = "POST"
path        = "/auth/*"
capacity    = 10
refill_rate = 0.2
"#
    )
}

fn rest_api_auth_readme(dir: &str) -> String {
    format!(
        r#"# {dir}

REST API with Bearer auth and PostgreSQL — generated by Bridge Framework.

## Quick start

```bash
# 1. Start the daemon
cargo run -p daemon

# 2. Create and migrate the database
bridge pg-create {dir}
bridge pg-migrate migrations/001_init.sql

# 3. Compile the API definition
bridge compile-file app.bridge

# 4. Set a bearer token
bridge auth-set bearer my-secret-token

# 5. Test the API
bridge ping
curl http://localhost:8787/health
curl -H "Authorization: Bearer my-secret-token" http://localhost:8787/api/v1/users
```

## Project structure

```
{dir}/
├── app.bridge          # API definition (services + endpoints)
├── bridge.toml         # Project configuration
├── migrations/
│   └── 001_init.sql    # Initial database schema
└── .env.example        # Environment variable template
```

## Services

| Service | Auth | Endpoints |
|---------|------|-----------|
| `public` | none | `GET /health`, `GET /api/version` |
| `auth` | none | `POST /auth/login`, `/auth/logout`, `/auth/refresh` |
| `users` | bearer | CRUD at `/api/v1/users` + `GET /me` |
| `items` | bearer | CRUD at `/api/v1/items` + `GET /search` |

## Adding a new endpoint

Edit `app.bridge`:

```bridge
service users
auth bearer
endpoint list   GET  /api/v1/users
endpoint get    GET  /api/v1/users/:id
endpoint create POST /api/v1/users
# add new endpoint:
endpoint export GET  /api/v1/users/export tags=admin
```

Then recompile:

```bash
bridge compile-file app.bridge
```

## Database management

```bash
# Create container
bridge pg-create {dir}

# Run migration
bridge pg-migrate migrations/001_init.sql

# Check status
bridge pg-status

# Destroy (removes all data)
bridge pg-destroy {dir}
```

## See also

- [Bridge CLI Reference](https://github.com/bridge-framework)
- `bridge help` for all available commands
"#,
        dir = dir
    )
}

fn default_bridge_toml(name: &str) -> String {
    format!(
        r#"# bridge.toml — Bridge Framework project configuration

[project]
name    = "{name}"
version = "0.1.0"

[daemon]
http_addr  = "127.0.0.1:8787"
tcp_addr   = "127.0.0.1:7878"
redis_addr = "127.0.0.1:6399"
mode       = "full"            # lite | full | ultra | off

[watch]
enabled = true
poll_ms = 500
dirs    = ["."]
files   = ["app.bridge"]

[[middleware.rules]]
name   = "powered-by"
scope  = "global"
after  = "header:X-Powered-By:bridge"

[[ratelimit.rules]]
method      = "POST"
path        = "/api/v1/compile"
capacity    = 60
refill_rate = 1.0
"#
    )
}

// ── Usage ─────────────────────────────────────────────────────────────────────

fn print_usage() {
    println!("{} {}", bold("bridge"), dim(&format!("v{VERSION}")));
    println!("{}", dim("Type-safe backend services framework\n"));
    println!("{}", bold("USAGE"));
    println!("  bridge <command> [args] [--json]\n");
    let g = |cmd: &str, args: &str, desc: &str| {
        let a = if args.is_empty() {
            String::new()
        } else {
            format!(" {}", dim(args))
        };
        println!("  {}{:<30} {}", cyan(cmd), a, dim(desc));
    };
    println!("{}", bold("CORE"));
    g("ping", "", "Check daemon health");
    g("health", "", "Full health report (JSON)");
    g("version", "", "Show version");
    g(
        "init",
        "<dir> [--template T]",
        "Scaffold a new project (T: default|rest-api-auth)",
    );
    g("stop", "", "Stop the daemon");
    println!("\n{}", bold("MODE"));
    g("mode-get", "", "Get current mode");
    g("mode-set", "<lite|full|ultra|off>", "Set daemon mode");
    println!("\n{}", bold("COMPILER"));
    g("compile", "<source>", "Compile Bridge DSL inline");
    g(
        "compile-file",
        "<path>",
        "Compile .bridge file → TypeScript",
    );
    g("services", "", "List registered services");
    g("routes", "", "List all routes");
    println!("\n{}", bold("AUTH"));
    g(
        "auth-set",
        "[scheme] <token>",
        "Set auth token (bearer|api_key)",
    );
    g("auth-clear", "", "Clear auth token");
    g("auth-status", "", "Show auth status");
    println!("\n{}", bold("KV STORE"));
    g("db-put", "<ns> <key> <value>", "Store a value");
    g("db-get", "<ns> <key>", "Get a value");
    g("db-del", "<ns> <key>", "Delete a value");
    g("db-keys", "<ns>", "List keys in namespace");
    g("db-flush", "<ns>", "Flush a namespace");
    println!("\n{}", bold("POSTGRES"));
    g("pg-create", "<name>", "Create Postgres container");
    g("pg-status", "", "Container status");
    g("pg-migrate", "<sql-file>", "Run SQL migration file");
    g("pg-destroy", "<name>", "Remove container");
    println!("\n{}", bold("REDIS"));
    g("redis-status", "", "Miniredis status");
    g("redis-ping", "", "Ping miniredis");
    g("redis-get", "<key>", "Get a key");
    g("redis-set", "<key> <value>", "Set a key");
    g("redis-del", "<key>", "Delete a key");
    g("redis-keys", "[pattern]", "List keys (default: *)");
    g("redis-flush", "", "Flush all Redis keys");
    println!("\n{}", bold("TRACES & METRICS"));
    g("trace-list", "[limit]", "List recent traces");
    g("trace-get", "<id>", "Get a specific trace");
    g("trace-clear", "", "Clear all traces");
    g("trace-export", "[json|csv|text]", "Export traces");
    g("metrics", "", "Show metrics summary");
    g("metrics-clear", "", "Reset metrics");
    println!("\n{}", bold("SHELL COMPLETIONS"));
    g(
        "completions",
        "<bash|zsh|fish|powershell>",
        "Print completion script",
    );
    println!("\n{}", dim("--json    output raw JSON response"));
    println!(
        "{}",
        dim("BRIDGE_TCP_ADDR   daemon address (default: 127.0.0.1:7878)")
    );
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_ping() {
        assert_eq!(build_command("ping", &[]), "PING");
    }
    #[test]
    fn cmd_health() {
        assert_eq!(build_command("health", &[]), "HEALTH");
    }
    #[test]
    fn cmd_version() {
        assert_eq!(build_command("version", &[]), "VERSION");
    }
    #[test]
    fn cmd_stop() {
        assert_eq!(build_command("stop", &[]), "STOP");
    }
    #[test]
    fn cmd_mode_get() {
        assert_eq!(build_command("mode-get", &[]), "MODE GET");
    }

    #[test]
    fn cmd_mode_set() {
        let args = vec!["lite".to_string()];
        assert_eq!(build_command("mode-set", &args), "MODE SET lite");
    }

    #[test]
    fn cmd_db_get() {
        let args = vec!["ns".to_string(), "key".to_string()];
        assert_eq!(build_command("db-get", &args), "DB GET ns key");
    }

    #[test]
    fn cmd_redis_keys_default() {
        assert_eq!(build_command("redis-keys", &[]), "REDIS KEYS *");
    }

    #[test]
    fn cmd_redis_keys_pattern() {
        let args = vec!["user:*".to_string()];
        assert_eq!(build_command("redis-keys", &args), "REDIS KEYS user:*");
    }

    #[test]
    fn cmd_trace_list_no_limit() {
        assert_eq!(build_command("trace-list", &[]), "TRACE LIST");
    }

    #[test]
    fn cmd_trace_list_with_limit() {
        let args = vec!["10".to_string()];
        assert_eq!(build_command("trace-list", &args), "TRACE LIST 10");
    }

    #[test]
    fn fmt_pong() {
        let out = fmt_output("ping", "PONG\n");
        assert!(out.contains("alive") || out.contains("✓"), "got: {out}");
    }

    #[test]
    fn fmt_ok() {
        let out = fmt_output("mode-set", "OK mode=lite\n");
        assert!(out.contains("mode=lite"), "got: {out}");
    }

    #[test]
    fn fmt_err() {
        let out = fmt_output("db-get", "ERR not found\n");
        assert!(out.contains("not found"), "got: {out}");
    }

    #[test]
    fn fmt_mode() {
        let out = fmt_output("mode-get", "MODE full\n");
        assert!(out.contains("full"), "got: {out}");
    }

    #[test]
    fn pretty_json_parses_object() {
        let j = r#"{"a":"b","c":1}"#;
        let out = pretty_json(j);
        assert!(out.contains("\"a\""), "got: {out}");
    }

    #[test]
    fn pretty_list_empty() {
        let out = pretty_list("[]");
        assert!(out.contains("empty"), "got: {out}");
    }

    #[test]
    fn init_creates_files() {
        let dir = "test_init_tmp_bridge";
        let _ = fs::remove_dir_all(dir);
        init_project(dir, "default").expect("init failed");
        assert!(std::path::Path::new(dir).join("app.bridge").exists());
        assert!(std::path::Path::new(dir).join("bridge.toml").exists());
        assert!(std::path::Path::new(dir).join("README.md").exists());
        // bridge.toml should contain the project name
        let toml = fs::read_to_string(std::path::Path::new(dir).join("bridge.toml")).unwrap();
        assert!(
            toml.contains("test_init_tmp_bridge") || toml.contains("[project]"),
            "bridge.toml should contain project section: {toml}"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn bash_completion_contains_commands() {
        assert!(BASH_COMPLETION.contains("ping"));
        assert!(BASH_COMPLETION.contains("completions"));
    }

    #[test]
    fn zsh_completion_contains_commands() {
        assert!(ZSH_COMPLETION.contains("ping"));
        assert!(ZSH_COMPLETION.contains("mode-set"));
    }

    #[test]
    fn fish_completion_contains_commands() {
        assert!(FISH_COMPLETION.contains("ping"));
        assert!(FISH_COMPLETION.contains("bash zsh fish powershell"));
    }

    #[test]
    fn powershell_completion_contains_commands() {
        assert!(POWERSHELL_COMPLETION.contains("Register-ArgumentCompleter"));
        assert!(POWERSHELL_COMPLETION.contains("ping"));
        assert!(POWERSHELL_COMPLETION.contains("mode-set"));
        assert!(POWERSHELL_COMPLETION.contains("lite"));
    }

    #[test]
    fn init_rest_api_auth_creates_files() {
        let dir = "test_init_rest_api_auth_tmp";
        let _ = fs::remove_dir_all(dir);
        init_project(dir, "rest-api-auth").expect("rest-api-auth init failed");
        let p = std::path::Path::new(dir);
        assert!(p.join("app.bridge").exists(), "app.bridge missing");
        assert!(p.join("bridge.toml").exists(), "bridge.toml missing");
        assert!(p.join("README.md").exists(), "README.md missing");
        assert!(p.join(".env.example").exists(), ".env.example missing");
        assert!(
            p.join("migrations").join("001_init.sql").exists(),
            "migration missing"
        );
        // app.bridge should have auth bearer
        let bridge = fs::read_to_string(p.join("app.bridge")).unwrap();
        assert!(bridge.contains("auth bearer"), "should have bearer auth");
        assert!(
            bridge.contains("service users"),
            "should have users service"
        );
        // migration should have CREATE TABLE
        let sql = fs::read_to_string(p.join("migrations").join("001_init.sql")).unwrap();
        assert!(
            sql.contains("CREATE TABLE"),
            "migration should have CREATE TABLE"
        );
        fs::remove_dir_all(dir).ok();
    }
}
