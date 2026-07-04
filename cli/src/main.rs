use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::process;

use protocol::escape;

const DEFAULT_ADDR: &str = "127.0.0.1:7878";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage_and_exit(1);
    }

    if args[0] == "init" {
        if args.len() != 2 {
            eprintln!("init requires a project directory name");
            process::exit(1);
        }
        match init_project(&args[1]) {
            Ok(()) => {
                println!("initialized bridge project at {}", args[1]);
                return;
            }
            Err(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
        }
    }

    let command = match args[0].as_str() {
        "ping" => "PING".to_string(),
        "help" => "HELP".to_string(),
        "stop" => "STOP".to_string(),
        "mode-get" => "MODE GET".to_string(),
        "mode-set" => {
            if args.len() != 2 {
                eprintln!("mode-set requires one value: lite|full|ultra|off");
                process::exit(1);
            }
            format!("MODE SET {}", args[1])
        }
        "compile" => {
            if args.len() < 2 {
                eprintln!("compile requires source text");
                process::exit(1);
            }
            let source = args[1..].join(" ");
            format!("COMPILE {}", escape(&source))
        }
        "compile-file" => {
            if args.len() != 2 {
                eprintln!("compile-file requires a path");
                process::exit(1);
            }
            let source = match fs::read_to_string(&args[1]) {
                Ok(text) => text,
                Err(err) => {
                    eprintln!("cannot read file {}: {err}", args[1]);
                    process::exit(1);
                }
            };
            format!("COMPILE {}", escape(&source))
        }
        "db-put" => {
            if args.len() < 4 {
                eprintln!("db-put requires namespace key value");
                process::exit(1);
            }
            let value = args[3..].join(" ");
            format!("DB PUT {} {} {}", args[1], args[2], escape(&value))
        }
        "db-get" => {
            if args.len() != 3 {
                eprintln!("db-get requires namespace key");
                process::exit(1);
            }
            format!("DB GET {} {}", args[1], args[2])
        }
        // ── New database commands ──
        "db-create" => {
            if args.len() != 2 {
                eprintln!("db-create requires a database name");
                process::exit(1);
            }
            format!("DB CREATE {}", args[1])
        }
        "db-status" => "DB STATUS".to_string(),
        "db-migrate" => {
            if args.len() != 2 {
                eprintln!("db-migrate requires a path to a SQL file");
                process::exit(1);
            }
            let sql = match fs::read_to_string(&args[1]) {
                Ok(text) => text,
                Err(err) => {
                    eprintln!("cannot read SQL file {}: {err}", args[1]);
                    process::exit(1);
                }
            };
            format!("DB MIGRATE {}", escape(&sql))
        }
        "db-destroy" => {
            if args.len() != 2 {
                eprintln!("db-destroy requires a database name");
                process::exit(1);
            }
            format!("DB DESTROY {}", args[1])
        }
        "redis-status" => "REDIS STATUS".to_string(),
        "raw" => {
            if args.len() < 2 {
                eprintln!("raw requires a command string");
                process::exit(1);
            }
            args[1..].join(" ")
        }
        _ => {
            print_usage_and_exit(1);
            String::new()
        }
    };

    match send_command(&resolve_addr(), &command) {
        Ok(output) => print!("{}", format_cli_output(&output)),
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    }
}

fn format_cli_output(raw: &str) -> String {
    let trimmed = raw.trim_end();
    if let Some(data) = trimmed.strip_prefix("DATA ") {
        return protocol::unescape(data);
    }
    if let Some(err) = trimmed.strip_prefix("ERR ") {
        return format!("error: {err}\n");
    }
    format!("{trimmed}\n")
}

fn resolve_addr() -> String {
    env::var("BRIDGE_TCP_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string())
}

fn send_command(addr: &str, command: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| {
        format!("cannot connect to daemon at {addr}: {e}. Start it with `cargo run -p daemon`.")
    })?;

    stream
        .write_all(format!("{command}\n").as_bytes())
        .map_err(|e| format!("failed to send command: {e}"))?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|e| format!("failed to finalize request: {e}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| format!("failed to read response: {e}"))?;
    Ok(response)
}

fn init_project(dir_name: &str) -> Result<(), String> {
    let root = PathBuf::from(dir_name);
    if root.exists() {
        return Err(format!("target path already exists: {}", root.display()));
    }

    let frontend_src = root.join("frontend").join("src");
    let frontend_gen = root.join("frontend").join("bridge.gen");
    fs::create_dir_all(&frontend_src).map_err(|e| format!("failed to create folders: {e}"))?;
    fs::create_dir_all(&frontend_gen).map_err(|e| format!("failed to create folders: {e}"))?;

    write_file(&root.join("bridge.app"), SAMPLE_BRIDGE_APP)?;
    write_file(&root.join("README.md"), INIT_README)?;
    write_file(&root.join("frontend").join("package.json"), FRONTEND_PACKAGE_JSON)?;
    write_file(&root.join("frontend").join("vite.config.ts"), FRONTEND_VITE_CONFIG)?;
    write_file(&root.join("frontend").join("tsconfig.json"), FRONTEND_TSCONFIG)?;
    write_file(&root.join("frontend").join("index.html"), FRONTEND_INDEX_HTML)?;
    write_file(&root.join("frontend").join("src").join("main.ts"), FRONTEND_MAIN_TS)?;
    write_file(&root.join("frontend").join("src").join("style.css"), FRONTEND_STYLE_CSS)?;
    write_file(
        &root.join("frontend").join("bridge.gen").join("client.ts"),
        FRONTEND_CLIENT_TS,
    )?;
    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn print_usage_and_exit(code: i32) {
    eprintln!(
        "usage: cli <command>\n\
         commands:\n\
           init <project-dir>\n\
           ping\n\
           help\n\
           stop\n\
           mode-get\n\
           mode-set <lite|full|ultra|off>\n\
           compile <source>\n\
           compile-file <path>\n\
           db-put <namespace> <key> <value>\n\
           db-get <namespace> <key>\n\
           db-create <name>\n\
           db-status\n\
           db-migrate <sql-file>\n\
           db-destroy <name>\n\
           redis-status\n\
           raw <command>"
    );
    process::exit(code);
}

const SAMPLE_BRIDGE_APP: &str = "service hello\nendpoint ping GET /ping\nendpoint echo POST /echo\n";

const INIT_README: &str = "# Bridge App\n\n1. Start daemon from bridge-framework repo: `cargo run -p daemon`\n2. Install CLI binary: `cargo install --path ./cli`\n3. Generate client in this app: `cd frontend && npm run generate-client:local`\n4. Run frontend: `npm install && npm run dev`\n";

const FRONTEND_PACKAGE_JSON: &str = r#"{
  "name": "bridge-frontend",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "generate-client:local": "bridge compile-file ../bridge.app > ./bridge.gen/client.ts"
  },
  "devDependencies": {
    "@tailwindcss/vite": "^4.1.11",
    "tailwindcss": "^4.1.11",
    "typescript": "^5.6.3",
    "vite": "^5.4.10"
  }
}
"#;

const FRONTEND_VITE_CONFIG: &str = r#"import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import path from "path";

export default defineConfig({
  plugins: [tailwindcss()],
  resolve: {
    alias: {
      "~bridge": path.resolve(__dirname, "./bridge.gen"),
    },
  },
});
"#;

const FRONTEND_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true
  }
}
"#;

const FRONTEND_INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Bridge Frontend</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
"#;

const FRONTEND_MAIN_TS: &str = r#"import "./style.css";
import { createClient } from "~bridge/client";

const app = document.querySelector<HTMLDivElement>('#app');
if (!app) throw new Error('missing app root');

const client = createClient("http://127.0.0.1:8787");

async function run() {
  const result = await client.ping();
  app.innerHTML = '<main class="mx-auto max-w-2xl p-8"><h1 class="text-3xl font-bold text-white">Bridge Frontend</h1><pre class="mt-4 rounded-lg bg-slate-900 p-4 text-emerald-300">' + result + '</pre></main>';
}

run().catch((err) => {
  app.textContent = String(err);
});
"#;

const FRONTEND_STYLE_CSS: &str = r#"@import "tailwindcss";

@layer base {
  body {
    @apply bg-slate-950 text-slate-100 antialiased;
  }
}
"#;

const FRONTEND_CLIENT_TS: &str = r##"// Generated by bridge codegen
export function createClient(baseUrl: string) {
  return {
    async ping() {
      const response = await fetch(`${baseUrl}/health`);
      if (!response.ok) throw new Error(`request failed: ${response.status}`);
      return response.text();
    },
  };
}
"##;
