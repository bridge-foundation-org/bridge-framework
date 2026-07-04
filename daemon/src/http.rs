//! HTTP API server for the Bridge daemon.
//!
//! Exposes RESTful endpoints for health, mode, compile, database
//! management, and Redis status. The server is a minimal hand-rolled
//! HTTP/1.1 implementation using only `std::net`.
//!
//! # Endpoints
//!
//! | Method | Path           | Description                      |
//! |--------|----------------|----------------------------------|
//! | GET    | /health        | Daemon health check              |
//! | GET    | /mode          | Current ponytail mode            |
//! | POST   | /mode          | Set ponytail mode                |
//! | POST   | /compile       | Compile Bridge DSL source        |
//! | GET    | /db/latest     | Latest codegen output            |
//! | POST   | /db/create     | Create Docker Postgres container |
//! | GET    | /db/status     | Check container status           |
//! | POST   | /db/migrate    | Run SQL migration                |
//! | DELETE | /db/destroy    | Stop and remove container        |
//! | GET    | /redis/status  | Miniredis server status          |

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::sqldb;
use crate::state::State;

/// Start the HTTP server loop on the given address.
pub fn run_http_server(addr: &str, state: Arc<Mutex<State>>) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("bridge daemon http listening on {addr}");
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(err) = handle_http_client(stream, state) {
                        eprintln!("http connection error: {err}");
                    }
                });
            }
            Err(err) => eprintln!("http accept error: {err}"),
        }
    }
    Ok(())
}

/// Handle a single HTTP request.
fn handle_http_client(mut stream: TcpStream, state: Arc<Mutex<State>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    if request_line.trim().is_empty() {
        return Ok(());
    }

    // Read headers
    let mut content_length = 0usize;
    loop {
        let mut header_line = String::new();
        reader.read_line(&mut header_line)?;
        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse::<usize>().unwrap_or(0);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    let body = String::from_utf8_lossy(&body).to_string();

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    let (status, content_type, payload) = route(method, path, &body, &state);

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: content-type\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Route an HTTP request to the appropriate handler.
fn route(
    method: &str,
    path: &str,
    body: &str,
    state: &Arc<Mutex<State>>,
) -> (&'static str, &'static str, String) {
    match (method, path) {
        // ── CORS preflight ──
        ("OPTIONS", _) => ("204 No Content", "text/plain", String::new()),

        // ── Core endpoints ──
        ("GET", "/health") => (
            "200 OK",
            "application/json",
            "{\"status\":\"ok\"}".to_string(),
        ),
        ("GET", "/mode") => {
            let mode = state.lock().expect("state lock poisoned").mode.clone();
            (
                "200 OK",
                "application/json",
                format!("{{\"mode\":\"{mode}\"}}"),
            )
        }
        ("POST", "/mode") => {
            let desired = body.trim().to_ascii_lowercase();
            if matches!(desired.as_str(), "lite" | "full" | "ultra" | "off") {
                state.lock().expect("state lock poisoned").mode = desired.clone();
                (
                    "200 OK",
                    "application/json",
                    format!("{{\"mode\":\"{desired}\"}}"),
                )
            } else {
                (
                    "400 Bad Request",
                    "application/json",
                    "{\"error\":\"invalid mode\"}".to_string(),
                )
            }
        }

        // ── Compiler ──
        ("POST", "/compile") => match compiler::compile(body) {
            Ok(service) => {
                let output = codegen::generate_typescript(&service);
                let mut guard = state.lock().expect("state lock poisoned");
                guard.store.put("codegen", &service.name, output.clone());
                guard.store.put("codegen", "latest", output.clone());
                ("200 OK", "text/plain", output)
            }
            Err(err) => ("400 Bad Request", "text/plain", err),
        },
        ("GET", "/db/latest") => {
            let value = state
                .lock()
                .expect("state lock poisoned")
                .store
                .get("codegen", "latest")
                .map(str::to_string)
                .unwrap_or_default();
            if value.is_empty() {
                (
                    "404 Not Found",
                    "text/plain",
                    "no generated output yet".to_string(),
                )
            } else {
                ("200 OK", "text/plain", value)
            }
        }

        // ── Database Docker management ──
        ("POST", "/db/create") => {
            let name = body.trim().to_string();
            let name = if name.is_empty() {
                "default".to_string()
            } else {
                name
            };
            match sqldb::create(&name) {
                Ok(msg) => (
                    "200 OK",
                    "application/json",
                    format!("{{\"ok\":true,\"message\":\"{}\"}}", json_escape(&msg)),
                ),
                Err(err) => (
                    "500 Internal Server Error",
                    "application/json",
                    format!("{{\"ok\":false,\"error\":\"{}\"}}", json_escape(&err)),
                ),
            }
        }
        ("GET", "/db/status") => match sqldb::status() {
            Ok(msg) => (
                "200 OK",
                "application/json",
                format!("{{\"status\":\"{}\"}}", json_escape(&msg)),
            ),
            Err(err) => (
                "500 Internal Server Error",
                "application/json",
                format!("{{\"error\":\"{}\"}}", json_escape(&err)),
            ),
        },
        ("POST", "/db/migrate") => match sqldb::migrate(body) {
            Ok(msg) => (
                "200 OK",
                "application/json",
                format!("{{\"ok\":true,\"result\":\"{}\"}}", json_escape(&msg)),
            ),
            Err(err) => (
                "400 Bad Request",
                "application/json",
                format!("{{\"ok\":false,\"error\":\"{}\"}}", json_escape(&err)),
            ),
        },
        ("DELETE", "/db/destroy") => {
            let name = body.trim().to_string();
            let name = if name.is_empty() {
                "default".to_string()
            } else {
                name
            };
            match sqldb::destroy(&name) {
                Ok(msg) => (
                    "200 OK",
                    "application/json",
                    format!("{{\"ok\":true,\"message\":\"{}\"}}", json_escape(&msg)),
                ),
                Err(err) => (
                    "500 Internal Server Error",
                    "application/json",
                    format!("{{\"ok\":false,\"error\":\"{}\"}}", json_escape(&err)),
                ),
            }
        }

        // ── Redis status ──
        ("GET", "/redis/status") => {
            let guard = state.lock().expect("state lock poisoned");
            let addr = guard
                .redis_addr
                .clone()
                .unwrap_or_else(|| "not running".to_string());
            let connections = guard
                .redis_connections
                .as_ref()
                .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
                .unwrap_or(0);
            (
                "200 OK",
                "application/json",
                format!("{{\"addr\":\"{addr}\",\"connections\":{connections}}}"),
            )
        }

        // ── 404 fallback ──
        _ => (
            "404 Not Found",
            "application/json",
            "{\"error\":\"not found\"}".to_string(),
        ),
    }
}

/// Escape special characters for embedding in JSON string values.
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
