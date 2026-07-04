//! CLI command parsing and dispatch.
//!
//! Maps user-facing subcommands (e.g. `bridge compile-file`) to the
//! protocol's wire format and sends them to the daemon via TCP.

use std::env;
use std::fs;
use std::process;

use protocol::escape;

/// Parse CLI arguments into a protocol command string.
///
/// Returns `None` for commands handled locally (e.g. `init`),
/// or `Some(command_string)` for commands that need to be sent to the daemon.
pub fn parse_args(args: &[String]) -> Option<String> {
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
        // ── Database Docker management ──
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
    Some(command)
}

/// Format raw daemon response for CLI output.
pub fn format_output(raw: &str) -> String {
    let trimmed = raw.trim_end();
    if let Some(data) = trimmed.strip_prefix("DATA ") {
        return protocol::unescape(data);
    }
    if let Some(err) = trimmed.strip_prefix("ERR ") {
        return format!("error: {err}\n");
    }
    format!("{trimmed}\n")
}

/// Print usage information and exit.
pub fn print_usage_and_exit(code: i32) {
    eprintln!(
        "usage: bridge <command>\n\
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
