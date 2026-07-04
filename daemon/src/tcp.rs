//! TCP protocol server for the Bridge daemon.
//!
//! Accepts incoming TCP connections and processes the line-oriented
//! protocol defined in the `protocol` crate. Each connection reads
//! one command, processes it, and writes the response.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use protocol::{Command, Response, parse_command, render_response};

use crate::sqldb;
use crate::state::State;

/// Start the TCP server loop on the given address.
pub fn run_tcp_server(addr: &str, state: Arc<Mutex<State>>) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("bridge daemon tcp listening on {addr}");
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(err) = handle_tcp_client(stream, state) {
                        eprintln!("tcp connection error: {err}");
                    }
                });
            }
            Err(err) => eprintln!("tcp accept error: {err}"),
        }
    }
    Ok(())
}

/// Handle a single TCP client connection.
fn handle_tcp_client(mut stream: TcpStream, state: Arc<Mutex<State>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response = process_line_command(line.trim(), state);
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Parse and execute a single line command, returning the serialized response.
pub fn process_line_command(input: &str, state: Arc<Mutex<State>>) -> String {
    let command = match parse_command(input) {
        Ok(cmd) => cmd,
        Err(err) => return render_response(Response::Error(err)),
    };
    let response = execute_command(command, state);
    render_response(response)
}

/// Execute a parsed `Command` and return a `Response`.
fn execute_command(command: Command, state: Arc<Mutex<State>>) -> Response {
    let mut guard = state.lock().expect("state lock poisoned");
    match command {
        Command::Ping => Response::Pong,
        Command::Help => Response::Data(
            "commands: ping, help, stop, mode get|set <lite|full|ultra|off>, compile <source>, db put/get, db-create/status/migrate/destroy, redis-status"
                .to_string(),
        ),
        Command::Stop => {
            guard.mode = "off".to_string();
            Response::Ok("MODE off".to_string())
        }
        Command::GetMode => Response::Mode(guard.mode.clone()),
        Command::SetMode(mode) => {
            guard.mode = mode.clone();
            Response::Ok(format!("MODE {mode}"))
        }
        Command::Compile { source } => match compiler::compile(&source) {
            Ok(service) => {
                let output = codegen::generate_typescript(&service);
                guard.store.put("codegen", &service.name, output.clone());
                guard.store.put("codegen", "latest", output.clone());
                Response::Data(output)
            }
            Err(err) => Response::Error(err),
        },
        Command::DbPut {
            namespace,
            key,
            value,
        } => {
            guard.store.put(&namespace, &key, value);
            Response::Ok("stored".to_string())
        }
        Command::DbGet { namespace, key } => match guard.store.get(&namespace, &key) {
            Some(value) => Response::Data(value.to_string()),
            None => Response::Error("not found".to_string()),
        },
        Command::DbCreate { name } => {
            // Release lock before Docker subprocess
            drop(guard);
            match sqldb::create(&name) {
                Ok(msg) => Response::Ok(msg),
                Err(err) => Response::Error(err),
            }
        }
        Command::DbStatus => {
            drop(guard);
            match sqldb::status() {
                Ok(msg) => Response::Data(msg),
                Err(err) => Response::Error(err),
            }
        }
        Command::DbMigrate { sql } => {
            drop(guard);
            match sqldb::migrate(&sql) {
                Ok(msg) => Response::Data(msg),
                Err(err) => Response::Error(err),
            }
        }
        Command::DbDestroy { name } => {
            drop(guard);
            match sqldb::destroy(&name) {
                Ok(msg) => Response::Ok(msg),
                Err(err) => Response::Error(err),
            }
        }
        Command::RedisStatus => {
            let addr = guard
                .redis_addr
                .clone()
                .unwrap_or_else(|| "not running".to_string());
            let connections = guard
                .redis_connections
                .as_ref()
                .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
                .unwrap_or(0);
            Response::Data(format!("addr={addr} connections={connections}"))
        }
    }
}
