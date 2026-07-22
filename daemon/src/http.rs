//! HTTP REST API server for the Bridge daemon.
//!
//! All endpoints are prefixed with `/api/v1/` (plus legacy `/` paths for
//! backwards-compat with the existing frontend).
//!
//! ## Endpoint map
//!
//! | Method | Path                     | Description                      |
//! |--------|--------------------------|----------------------------------|
//! | GET    | /health                  | Health check (legacy)            |
//! | GET    | /api/v1/health           | Health check with full detail    |
//! | GET    | /api/v1/version          | Daemon version                   |
//! | GET    | /api/v1/mode             | Current mode                     |
//! | POST   | /api/v1/mode             | Set mode                         |
//! | POST   | /compile, /api/v1/compile| Compile Bridge DSL source        |
//! | GET    | /api/v1/services         | List registered services         |
//! | GET    | /api/v1/routes           | List all endpoints               |
//! | GET    | /api/v1/codegen/latest   | Latest generated TypeScript      |
//! | POST   | /api/v1/pg/create        | Create Postgres container        |
//! | GET    | /api/v1/pg/status        | Container status                 |
//! | POST   | /api/v1/pg/migrate       | Run SQL migration                |
//! | DELETE | /api/v1/pg/destroy       | Remove container                 |
//! | GET    | /api/v1/redis/status     | Miniredis status                 |
//! | GET    | /api/v1/auth/status      | Auth token status                |
//! | POST   | /api/v1/auth/set         | Set auth token                   |
//! | DELETE | /api/v1/auth/clear       | Clear auth token                 |
//! | GET    | /api/v1/traces           | List traces                      |
//! | DELETE | /api/v1/traces           | Clear all traces                 |
//! | GET    | /api/v1/traces/:id       | Get specific trace               |

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::sqldb;
use crate::state::{SharedState, State};

// ── Server ────────────────────────────────────────────────────────────────────

pub fn run_http_server(addr: &str, state: SharedState) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("[bridge] HTTP listening on {addr}");
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let st = Arc::clone(&state);
                thread::spawn(move || { let _ = handle(stream, st); });
            }
            Err(e) => eprintln!("[bridge] HTTP accept: {e}"),
        }
    }
    Ok(())
}

// ── Request parsing ───────────────────────────────────────────────────────────

struct Request {
    method: String,
    path: String,
    body: String,
    auth_header: Option<String>,
}

fn parse_request(stream: &mut TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let request_line = request_line.trim();
    if request_line.is_empty() { return None; }

    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let full_path = parts.next()?.to_string();
    // Strip query string for routing
    let path = full_path.split('?').next().unwrap_or(&full_path).to_string();

    let mut content_length = 0usize;
    let mut auth_header: Option<String> = None;
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim();
        if h.is_empty() { break; }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("authorization:") {
            auth_header = Some(h[14..].trim().to_string());
        }
    }

    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 { reader.read_exact(&mut body_bytes).ok()?; }
    let body = String::from_utf8_lossy(&body_bytes).to_string();

    Some(Request { method, path, body, auth_header })
}

// ── Response helpers ──────────────────────────────────────────────────────────

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) -> std::io::Result<()> {
    let status_text = match status {
        200 => "200 OK", 201 => "201 Created", 204 => "204 No Content",
        400 => "400 Bad Request", 401 => "401 Unauthorized",
        404 => "404 Not Found", 500 => "500 Internal Server Error",
        _ => "200 OK",
    };
    let response = format!(
        "HTTP/1.1 {status_text}\r\n\
         Content-Type: {content_type}; charset=utf-8\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, DELETE, PUT, OPTIONS\r\n\
         Access-Control-Allow-Headers: content-type, authorization\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        len = body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn json(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    respond(stream, status, "application/json", body)
}
fn ok_json(stream: &mut TcpStream, body: &str) -> std::io::Result<()> { json(stream, 200, body) }
fn err_json(stream: &mut TcpStream, status: u16, msg: &str) -> std::io::Result<()> {
    json(stream, status, &format!(r#"{{"error":{}}}"#, jstr(msg)))
}

// ── Router ────────────────────────────────────────────────────────────────────

fn handle(mut stream: TcpStream, state: SharedState) -> std::io::Result<()> {
    let req = match parse_request(&mut stream) {
        Some(r) => r,
        None    => return Ok(()),
    };
    let t0 = Instant::now();

    let method = req.method.as_str();
    let path   = req.path.as_str();

    // CORS preflight
    if method == "OPTIONS" {
        return respond(&mut stream, 204, "text/plain", "");
    }

    let result = route(method, path, &req.body, &req.auth_header, &state, &mut stream);

    // Record trace
    let elapsed = t0.elapsed().as_millis() as u64;
    if let Ok(mut g) = state.lock() {
        g.push_trace(method, path, 200, elapsed);
    }

    result
}

fn route(
    method: &str,
    path: &str,
    body: &str,
    auth: &Option<String>,
    state: &SharedState,
    stream: &mut TcpStream,
) -> std::io::Result<()> {
    // ── Legacy / backwards-compat paths ───────────────────────────────────
    match (method, path) {
        ("GET",  "/health") => return ok_json(stream, r#"{"status":"ok"}"#),
        ("GET",  "/mode")   => {
            let m = state.lock().unwrap().mode;
            return ok_json(stream, &format!(r#"{{"mode":{}}}"#, jstr(m.as_str())));
        }
        ("POST", "/mode")   => {
            let mode = body.trim().to_ascii_lowercase();
            return set_mode(mode, state, stream);
        }
        ("POST", "/compile") => return handle_compile(body, state, stream),
        ("GET",  "/db/latest") => return handle_codegen_latest(state, stream),
        ("GET",  "/db/status") => return handle_pg_status(stream),
        ("POST", "/db/create") => return handle_pg_create(body, stream),
        ("POST", "/db/migrate")=> return handle_pg_migrate(body, stream),
        ("DELETE", "/db/destroy") => return handle_pg_destroy(body, stream),
        ("GET",  "/redis/status") => return handle_redis_status(state, stream),
        _ => {}
    }

    // ── API v1 ────────────────────────────────────────────────────────────
    match (method, path) {
        ("GET",  "/api/v1/health")  => {
            let body = health_json(&state.lock().unwrap());
            ok_json(stream, &body)
        }
        ("GET",  "/api/v1/version") => ok_json(stream, &format!(r#"{{"version":{}}}"#, jstr(protocol::VERSION))),

        ("GET",  "/api/v1/mode")    => {
            let m = state.lock().unwrap().mode;
            ok_json(stream, &format!(r#"{{"mode":{}}}"#, jstr(m.as_str())))
        }
        ("POST", "/api/v1/mode")    => set_mode(body.trim().to_ascii_lowercase(), state, stream),

        ("POST", "/api/v1/compile") | ("POST", "/compile") => handle_compile(body, state, stream),

        ("GET",  "/api/v1/services") => {
            let g = state.lock().unwrap();
            match &g.service_registry {
                None    => err_json(stream, 404, "no services compiled yet"),
                Some(f) => {
                    let arr: Vec<String> = f.services.iter().map(|s| {
                        format!(r#"{{"name":{},"auth":{},"endpoints":{}}}"#,
                            jstr(&s.name), jstr(s.auth.as_str()), s.endpoints.len())
                    }).collect();
                    ok_json(stream, &format!("[{}]", arr.join(",")))
                }
            }
        }
        ("GET",  "/api/v1/routes") => {
            let g = state.lock().unwrap();
            match &g.service_registry {
                None    => err_json(stream, 404, "no services compiled yet"),
                Some(f) => {
                    let mut routes = Vec::new();
                    for svc in &f.services {
                        for ep in &svc.endpoints {
                            routes.push(format!(
                                r#"{{"service":{},"name":{},"method":{},"path":{}}}"#,
                                jstr(&svc.name), jstr(&ep.name),
                                jstr(ep.method.as_str()), jstr(&ep.path)
                            ));
                        }
                    }
                    ok_json(stream, &format!("[{}]", routes.join(",")))
                }
            }
        }

        ("GET",  "/api/v1/codegen/latest") => handle_codegen_latest(state, stream),

        // Postgres
        ("POST",   "/api/v1/pg/create")  => handle_pg_create(body, stream),
        ("GET",    "/api/v1/pg/status")  => handle_pg_status(stream),
        ("POST",   "/api/v1/pg/migrate") => handle_pg_migrate(body, stream),
        ("DELETE", "/api/v1/pg/destroy") => handle_pg_destroy(body, stream),

        // Redis
        ("GET", "/api/v1/redis/status") => handle_redis_status(state, stream),

        // Auth
        ("GET",    "/api/v1/auth/status") => {
            let set = state.lock().unwrap().auth_token.is_some();
            ok_json(stream, &format!(r#"{{"configured":{set}}}"#))
        }
        ("POST",   "/api/v1/auth/set") => {
            let token = body.trim().to_string();
            if token.is_empty() { return err_json(stream, 400, "token cannot be empty"); }
            state.lock().unwrap().auth_token = Some(token);
            ok_json(stream, r#"{"ok":true}"#)
        }
        ("DELETE", "/api/v1/auth/clear") => {
            state.lock().unwrap().auth_token = None;
            ok_json(stream, r#"{"ok":true}"#)
        }

        // Traces
        ("GET",    "/api/v1/traces") => {
            let g = state.lock().unwrap();
            let arr: Vec<String> = g.traces.iter().map(|t| t.to_json()).collect();
            ok_json(stream, &format!("[{}]", arr.join(",")))
        }
        ("DELETE", "/api/v1/traces") => {
            state.lock().unwrap().traces.clear();
            ok_json(stream, r#"{"ok":true}"#)
        }
        // /api/v1/traces/:id
        (_, p) if method == "GET" && p.starts_with("/api/v1/traces/") => {
            let id = &p["/api/v1/traces/".len()..];
            let g  = state.lock().unwrap();
            match g.find_trace(id) {
                Some(t) => ok_json(stream, &t.to_json()),
                None    => err_json(stream, 404, &format!("trace '{id}' not found")),
            }
        }

        _ => err_json(stream, 404, &format!("route not found: {method} {}", path)),
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

fn set_mode(mode: String, state: &SharedState, stream: &mut TcpStream) -> std::io::Result<()> {
    match protocol::DaemonMode::parse(&mode) {
        Err(_) => return err_json(stream, 400, "invalid mode — use: lite|full|ultra|off"),
        Ok(m)  => {
            state.lock().unwrap().mode = m;
            ok_json(stream, &format!(r#"{{"mode":{}}}"#, jstr(m.as_str())))
        }
    }
}

fn handle_compile(body: &str, state: &SharedState, stream: &mut TcpStream) -> std::io::Result<()> {
    match compiler::parse(body) {
        Err(e) => err_json(stream, 400, &e),
        Ok(file) => {
            let ts = codegen::generate_typescript(&file);
            {
                let mut g = state.lock().unwrap();
                if let Some(first) = file.services.first() {
                    g.store.put("codegen", &first.name, ts.clone());
                }
                g.store.put("codegen", "latest", ts.clone());
                g.service_registry = Some(file);
            }
            respond(stream, 200, "text/plain", &ts)
        }
    }
}

fn handle_codegen_latest(state: &SharedState, stream: &mut TcpStream) -> std::io::Result<()> {
    let ts = state.lock().unwrap().store.get("codegen", "latest");
    match ts {
        Some(v) => respond(stream, 200, "text/plain", &v),
        None    => err_json(stream, 404, "no generated output yet — run POST /compile first"),
    }
}

fn handle_pg_create(body: &str, stream: &mut TcpStream) -> std::io::Result<()> {
    let name = body.trim();
    let name = if name.is_empty() { "default" } else { name };
    match sqldb::create(name) {
        Ok(msg) => ok_json(stream, &format!(r#"{{"ok":true,"message":{}}}"#, jstr(&msg))),
        Err(e)  => err_json(stream, 500, &e),
    }
}

fn handle_pg_status(stream: &mut TcpStream) -> std::io::Result<()> {
    match sqldb::status() {
        Ok(msg) => ok_json(stream, &msg),
        Err(e)  => err_json(stream, 500, &e),
    }
}

fn handle_pg_migrate(body: &str, stream: &mut TcpStream) -> std::io::Result<()> {
    match sqldb::migrate(body) {
        Ok(msg) => ok_json(stream, &format!(r#"{{"ok":true,"result":{}}}"#, jstr(&msg))),
        Err(e)  => err_json(stream, 400, &e),
    }
}

fn handle_pg_destroy(body: &str, stream: &mut TcpStream) -> std::io::Result<()> {
    let name = body.trim();
    let name = if name.is_empty() { "default" } else { name };
    match sqldb::destroy(name) {
        Ok(msg) => ok_json(stream, &format!(r#"{{"ok":true,"message":{}}}"#, jstr(&msg))),
        Err(e)  => err_json(stream, 500, &e),
    }
}

fn handle_redis_status(state: &SharedState, stream: &mut TcpStream) -> std::io::Result<()> {
    let g = state.lock().unwrap();
    let addr  = g.redis_addr.clone().unwrap_or_else(|| "not running".into());
    let conns = g.redis_connections_count();
    ok_json(stream, &format!(r#"{{"addr":{},"connections":{conns}}}"#, jstr(&addr)))
}

// ── JSON helpers ──────────────────────────────────────────────────────────────

/// Wrap a string as a JSON string literal (with escaping).
fn jstr(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")
        .replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"))
}

fn health_json(g: &State) -> String {
    let redis  = g.redis_addr.as_deref().unwrap_or("off");
    let conns  = g.redis_connections_count();
    let svcs   = g.service_registry.as_ref().map(|f| f.services.len()).unwrap_or(0);
    let traces = g.traces.len();
    format!(
        r#"{{"status":"ok","mode":{},"redis":{},"redis_connections":{conns},"services":{svcs},"traces":{traces}}}"#,
        jstr(g.mode.as_str()), jstr(redis)
    )
}
