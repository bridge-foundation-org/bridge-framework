//! HTTP REST API server for the Bridge daemon.
//!
//! All endpoints are prefixed with `/api/v1/` plus legacy `/` paths for
//! backwards-compatibility with the existing frontend.
//!
//! ## Features
//! - CORS headers on every response (configurable via `BRIDGE_CORS_ORIGIN`)
//! - OPTIONS preflight support
//! - Request timing (recorded as traces)
//! - Structured JSON error responses
//! - Health check with full metadata

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use crate::sqldb;
use crate::state::{LogLevel, SharedState};

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

// ── CORS origin ───────────────────────────────────────────────────────────────

fn cors_origin() -> String {
    std::env::var("BRIDGE_CORS_ORIGIN").unwrap_or_else(|_| "*".to_string())
}

// ── Request parsing ───────────────────────────────────────────────────────────

struct Request {
    method:  String,
    path:    String,
    headers: Vec<(String, String)>,
    body:    String,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        let name_lc = name.to_lowercase();
        self.headers.iter()
            .find(|(k, _)| k.to_lowercase() == name_lc)
            .map(|(_, v)| v.as_str())
    }
}

fn parse_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_uppercase();
    let path   = parts.next()?.to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() { break; }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if k.to_lowercase() == "content-length" {
                content_length = v.parse().unwrap_or(0);
            }
            headers.push((k, v));
        }
    }

    let body = if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf).ok()?;
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        String::new()
    };

    Some(Request { method, path, headers, body })
}

// ── Response helpers ──────────────────────────────────────────────────────────

fn json_response(status: u16, body: &str) -> String {
    let reason = match status {
        200 => "OK", 201 => "Created", 204 => "No Content",
        400 => "Bad Request", 401 => "Unauthorized", 404 => "Not Found",
        405 => "Method Not Allowed", 500 => "Internal Server Error",
        _ => "Unknown",
    };
    let origin = cors_origin();
    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         Access-Control-Allow-Origin: {origin}\r\n\
         Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, X-Bridge-Token\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        len = body.len()
    )
}

fn text_response(status: u16, body: &str) -> String {
    let reason = match status { 200 => "OK", _ => "Error" };
    let origin = cors_origin();
    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: text/plain\r\n\
         Content-Length: {len}\r\n\
         Access-Control-Allow-Origin: {origin}\r\n\
         Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, X-Bridge-Token\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        len = body.len()
    )
}

fn cors_preflight() -> String {
    let origin = cors_origin();
    format!(
        "HTTP/1.1 204 No Content\r\n\
         Access-Control-Allow-Origin: {origin}\r\n\
         Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, X-Bridge-Token\r\n\
         Access-Control-Max-Age: 86400\r\n\
         Connection: close\r\n\
         \r\n"
    )
}

fn ok(body: &str)  -> String { json_response(200, body) }
fn err(body: &str) -> String { json_response(500, body) }
fn not_found()     -> String { json_response(404, r#"{"error":"not found"}"#) }
fn bad_request(msg: &str) -> String {
    json_response(400, &format!(r#"{{"error":"{msg}"}}"#))
}

// ── Connection handler ────────────────────────────────────────────────────────

fn handle(mut stream: TcpStream, state: SharedState) -> std::io::Result<()> {
    let start  = Instant::now();
    let req    = match parse_request(&stream) { Some(r) => r, None => return Ok(()) };
    let method = req.method.clone();
    let path   = req.path.clone();

    // CORS preflight
    if method == "OPTIONS" {
        stream.write_all(cors_preflight().as_bytes())?;
        return stream.flush();
    }

    let response = route(&req, &state);
    let status   = response.split_whitespace().nth(1)
        .and_then(|s| s.parse::<u16>().ok()).unwrap_or(200);
    let elapsed  = start.elapsed().as_millis() as u64;

    // Record trace
    {
        if let Ok(mut g) = state.lock() {
            g.push_trace(&method, &path, status, elapsed);
            g.push_log(
                if status >= 500 { LogLevel::Error } else { LogLevel::Info },
                &format!("{method} {path} {status} {elapsed}ms"),
                Default::default(),
            );
        }
    }

    stream.write_all(response.as_bytes())?;
    stream.flush()
}

// ── Router ────────────────────────────────────────────────────────────────────

fn route(req: &Request, state: &SharedState) -> String {
    let path = req.path.split('?').next().unwrap_or(&req.path);

    match (req.method.as_str(), path) {
        // ── Legacy health / mode ──────────────────────────────────────────
        ("GET",  "/health")  => health(state),
        ("GET",  "/mode")    => get_mode(state),
        ("POST", "/mode")    => set_mode(req, state),

        // ── Legacy compile / services ─────────────────────────────────────
        ("POST", "/compile") => compile(req, state),
        ("GET",  "/services")=> services(state),
        ("GET",  "/routes")  => routes(state),

        // ── Legacy codegen ────────────────────────────────────────────────
        ("GET",  "/codegen/latest") => codegen_latest(state),

        // ── Legacy DB / Redis ─────────────────────────────────────────────
        ("GET",  "/db/status")   => db_status(),
        ("POST", "/db/create")   => db_create(req),
        ("POST", "/db/migrate")  => db_migrate(req),
        ("DELETE", "/db/destroy")=> db_destroy(req),
        ("GET",  "/redis/status")=> redis_status(state),

        // ── v1 API ────────────────────────────────────────────────────────
        ("GET",  "/api/v1/health")        => health(state),
        ("GET",  "/api/v1/version")       => version(),
        ("GET",  "/api/v1/mode")          => get_mode(state),
        ("POST", "/api/v1/mode")          => set_mode(req, state),
        ("POST", "/api/v1/compile")       => compile(req, state),
        ("GET",  "/api/v1/services")      => services(state),
        ("GET",  "/api/v1/routes")        => routes(state),
        ("GET",  "/api/v1/codegen/latest")=> codegen_latest(state),
        ("GET",  "/api/v1/pg/status")     => db_status(),
        ("POST", "/api/v1/pg/create")     => db_create(req),
        ("POST", "/api/v1/pg/migrate")    => db_migrate(req),
        ("DELETE", "/api/v1/pg/destroy")  => db_destroy(req),
        ("GET",  "/api/v1/redis/status")  => redis_status(state),
        ("GET",  "/api/v1/auth/status")   => auth_status(state),
        ("POST", "/api/v1/auth/set")      => auth_set(req, state),
        ("DELETE", "/api/v1/auth/clear")  => auth_clear(state),
        ("GET",  "/api/v1/traces")        => traces_list(req, state),
        ("DELETE", "/api/v1/traces")      => traces_clear(state),
        ("GET",  p) if p.starts_with("/api/v1/traces/") => {
            let id = p.trim_start_matches("/api/v1/traces/");
            trace_get(id, state)
        }
        ("GET",  "/api/v1/metrics")       => metrics(state),
        ("DELETE", "/api/v1/metrics")     => metrics_clear(state),
        ("GET",  "/api/v1/openapi")       => openapi(state),
        ("POST", "/api/v1/sampling")      => set_sampling(req, state),

        // ── Catch-all ─────────────────────────────────────────────────────
        _ => not_found(),
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

fn health(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.health_json())
}

fn version() -> String {
    ok(&format!(r#"{{"version":"{}"}}"#, protocol::VERSION))
}

fn get_mode(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&format!(r#"{{"mode":"{}"}}"#, g.mode))
}

fn set_mode(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim().trim_matches('"');
    match protocol::DaemonMode::parse(body) {
        Ok(mode) => {
            state.lock().unwrap().mode = mode;
            ok(&format!(r#"{{"mode":"{}"}}"#, body))
        }
        Err(e) => bad_request(&e),
    }
}

fn compile(req: &Request, state: &SharedState) -> String {
    let source = &req.body;
    match compiler::parse(source) {
        Err(e) => bad_request(&e),
        Ok(file) => {
            let ts = codegen::generate_typescript(&file);
            let mut g = state.lock().unwrap();
            if let Some(first) = file.services.first() {
                g.store.put("codegen", &first.name, ts.clone());
            }
            g.store.put("codegen", "latest", ts.clone());
            g.service_registry = Some(file);
            text_response(200, &ts)
        }
    }
}

fn services(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    match &g.service_registry {
        None => bad_request("no services compiled yet"),
        Some(f) => {
            let items: Vec<String> = f.services.iter().map(|s| {
                format!(r#"{{"name":"{}","auth":"{}","endpoints":{}}}"#,
                    s.name, s.auth.as_str(), s.endpoints.len())
            }).collect();
            ok(&format!("[{}]", items.join(",")))
        }
    }
}

fn routes(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    match &g.service_registry {
        None => bad_request("no services compiled yet"),
        Some(f) => {
            let mut items = Vec::new();
            for svc in &f.services {
                for ep in &svc.endpoints {
                    items.push(format!(
                        r#"{{"service":"{}","name":"{}","method":"{}","path":"{}"}}"#,
                        svc.name, ep.name, ep.method.as_str(), ep.path
                    ));
                }
            }
            ok(&format!("[{}]", items.join(",")))
        }
    }
}

fn codegen_latest(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    match g.store.get("codegen", "latest") {
        Some(ts) => text_response(200, &ts),
        None => not_found(),
    }
}

fn db_status() -> String {
    match sqldb::status() {
        Ok(msg)  => ok(&msg),
        Err(msg) => err(&format!(r#"{{"error":"{msg}"}}"#)),
    }
}

fn db_create(req: &Request) -> String {
    let name = req.body.trim();
    if name.is_empty() { return bad_request("name required"); }
    match sqldb::create(name) {
        Ok(msg)  => ok(&format!(r#"{{"message":"{msg}"}}"#)),
        Err(msg) => err(&format!(r#"{{"error":"{msg}"}}"#)),
    }
}

fn db_migrate(req: &Request) -> String {
    let sql = req.body.trim();
    if sql.is_empty() { return bad_request("sql required"); }
    match sqldb::migrate(sql) {
        Ok(msg)  => ok(&format!(r#"{{"message":"{msg}"}}"#)),
        Err(msg) => err(&format!(r#"{{"error":"{msg}"}}"#)),
    }
}

fn db_destroy(req: &Request) -> String {
    let name = req.body.trim();
    if name.is_empty() { return bad_request("name required"); }
    match sqldb::destroy(name) {
        Ok(msg)  => ok(&format!(r#"{{"message":"{msg}"}}"#)),
        Err(msg) => err(&format!(r#"{{"error":"{msg}"}}"#)),
    }
}

fn redis_status(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    let addr  = g.redis_addr.clone().unwrap_or_else(|| "not running".into());
    let conns = g.redis_connections_count();
    ok(&format!(r#"{{"addr":"{addr}","connections":{conns}}}"#))
}

fn auth_status(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&format!(r#"{{"configured":{}}}"#, g.auth_token.is_some()))
}

fn auth_set(req: &Request, state: &SharedState) -> String {
    let token = req.body.trim().to_string();
    if token.is_empty() { return bad_request("token required"); }
    state.lock().unwrap().auth_token = Some(token);
    ok(r#"{"message":"auth token set"}"#)
}

fn auth_clear(state: &SharedState) -> String {
    state.lock().unwrap().auth_token = None;
    ok(r#"{"message":"auth token cleared"}"#)
}

fn traces_list(req: &Request, state: &SharedState) -> String {
    let limit = req.path.split('?').nth(1)
        .and_then(|q| q.split('&').find(|p| p.starts_with("limit=")))
        .and_then(|p| p.trim_start_matches("limit=").parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let g = state.lock().unwrap();
    let items: Vec<String> = g.traces.iter().take(limit).map(|t| t.to_json()).collect();
    ok(&format!("[{}]", items.join(",")))
}

fn traces_clear(state: &SharedState) -> String {
    state.lock().unwrap().traces.clear();
    ok(r#"{"message":"traces cleared"}"#)
}

fn trace_get(id: &str, state: &SharedState) -> String {
    let g = state.lock().unwrap();
    match g.find_trace(id) {
        Some(t) => ok(&t.to_json()),
        None    => not_found(),
    }
}

fn metrics(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.metrics.to_json())
}

fn metrics_clear(state: &SharedState) -> String {
    state.lock().unwrap().metrics = Default::default();
    ok(r#"{"message":"metrics cleared"}"#)
}

fn openapi(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    match &g.service_registry {
        None => bad_request("no services compiled yet — run POST /compile first"),
        Some(f) => {
            let spec = codegen::generate_openapi(f);
            ok(&spec)
        }
    }
}

fn set_sampling(req: &Request, state: &SharedState) -> String {
    let rate: f64 = match req.body.trim().parse() {
        Ok(r) if (0.0..=1.0).contains(&r) => r,
        _ => return bad_request("rate must be a float between 0.0 and 1.0"),
    };
    state.lock().unwrap().trace_sample_rate = rate;
    ok(&format!(r#"{{"sample_rate":{rate}}}"#))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use std::sync::Mutex;

    fn state() -> SharedState {
        Arc::new(Mutex::new(State::new(None, None)))
    }

    fn fake_req(method: &str, path: &str, body: &str) -> Request {
        Request {
            method:  method.to_string(),
            path:    path.to_string(),
            headers: vec![],
            body:    body.to_string(),
        }
    }

    #[test]
    fn health_returns_ok() {
        let r = route(&fake_req("GET", "/health", ""), &state());
        assert!(r.contains("200"), "got: {r}");
        assert!(r.contains("\"status\":\"ok\""), "got: {r}");
    }

    #[test]
    fn health_includes_version() {
        let r = route(&fake_req("GET", "/health", ""), &state());
        assert!(r.contains("version"), "got: {r}");
    }

    #[test]
    fn mode_get_returns_mode() {
        let r = route(&fake_req("GET", "/mode", ""), &state());
        assert!(r.contains("mode"), "got: {r}");
    }

    #[test]
    fn mode_set_valid() {
        let r = route(&fake_req("POST", "/mode", "lite"), &state());
        assert!(r.contains("200"), "got: {r}");
    }

    #[test]
    fn mode_set_invalid() {
        let r = route(&fake_req("POST", "/mode", "nope"), &state());
        assert!(r.contains("400"), "got: {r}");
    }

    #[test]
    fn compile_returns_ts() {
        let r = route(&fake_req("POST", "/compile", "service hello\nendpoint ping GET /ping"), &state());
        assert!(r.contains("200"), "got: {r}");
        assert!(r.contains("hello") || r.contains("BridgeClient"), "got: {r}");
    }

    #[test]
    fn compile_bad_source() {
        let r = route(&fake_req("POST", "/compile", "bad source###"), &state());
        assert!(r.contains("400"), "got: {r}");
    }

    #[test]
    fn services_before_compile() {
        let r = route(&fake_req("GET", "/services", ""), &state());
        assert!(r.contains("400"), "got: {r}");
    }

    #[test]
    fn cors_preflight_responds_204() {
        let r = cors_preflight();
        assert!(r.contains("204"), "got: {r}");
        assert!(r.contains("Access-Control-Allow-Origin"), "got: {r}");
    }

    #[test]
    fn metrics_endpoint() {
        let s = state();
        s.lock().unwrap().push_trace("GET", "/ping", 200, 5);
        let r = route(&fake_req("GET", "/api/v1/metrics", ""), &s);
        assert!(r.contains("200"), "got: {r}");
        assert!(r.contains("total_requests"), "got: {r}");
    }

    #[test]
    fn sampling_set() {
        let r = route(&fake_req("POST", "/api/v1/sampling", "0.5"), &state());
        assert!(r.contains("200"), "got: {r}");
        assert!(r.contains("0.5"), "got: {r}");
    }

    #[test]
    fn sampling_invalid() {
        let r = route(&fake_req("POST", "/api/v1/sampling", "2.0"), &state());
        assert!(r.contains("400"), "got: {r}");
    }

    #[test]
    fn not_found_returns_404() {
        let r = route(&fake_req("GET", "/nonexistent", ""), &state());
        assert!(r.contains("404"), "got: {r}");
    }

    #[test]
    fn api_v1_health() {
        let r = route(&fake_req("GET", "/api/v1/health", ""), &state());
        assert!(r.contains("200"), "got: {r}");
        assert!(r.contains("\"status\":\"ok\""), "got: {r}");
    }

    #[test]
    fn openapi_before_compile() {
        let r = route(&fake_req("GET", "/api/v1/openapi", ""), &state());
        assert!(r.contains("400"), "got: {r}");
    }

    #[test]
    fn openapi_after_compile() {
        let s = state();
        route(&fake_req("POST", "/compile", "service api\nendpoint list GET /items"), &s);
        let r = route(&fake_req("GET", "/api/v1/openapi", ""), &s);
        assert!(r.contains("200"), "got: {r}");
        assert!(r.contains("openapi"), "got: {r}");
    }
}
