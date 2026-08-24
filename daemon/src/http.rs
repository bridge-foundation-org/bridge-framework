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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use crate::middleware::{MiddlewareBuilder, MiddlewareContext, Scope};
use crate::ratelimit::BucketKey;
use crate::sqldb;
use crate::state::{LogLevel, SharedState};
use crate::staticfiles::{StaticMount, StaticResult};
use crate::streaming;
use crate::validation::{self, violations_json, Rule};

// ── Request ID counter ────────────────────────────────────────────────────────

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_request_id() -> String {
    let n = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req-{n:08x}")
}

// ── Server ────────────────────────────────────────────────────────────────────

pub fn run_http_server(addr: &str, state: SharedState) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("[bridge] HTTP listening on {addr}");
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let st = Arc::clone(&state);
                thread::spawn(move || {
                    let _ = handle(stream, st);
                });
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

pub(crate) struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        let name_lc = name.to_lowercase();
        self.headers
            .iter()
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
    let path = parts.next()?.to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
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

    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

// ── Response helpers ──────────────────────────────────────────────────────────

fn json_response(status: u16, body: &str) -> String {
    json_response_with_id(status, body, "")
}

fn json_response_with_id(status: u16, body: &str, req_id: &str) -> String {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let origin = cors_origin();
    let id_hdr = if req_id.is_empty() {
        String::new()
    } else {
        format!("X-Bridge-Request-Id: {req_id}\r\n")
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         Access-Control-Allow-Origin: {origin}\r\n\
         Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, X-Bridge-Token, X-Api-Key\r\n\
         {id_hdr}\
         Connection: close\r\n\
         \r\n\
         {body}",
        len = body.len()
    )
}

fn text_response(status: u16, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        _ => "Error",
    };
    let origin = cors_origin();
    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: text/plain\r\n\
         Content-Length: {len}\r\n\
         Access-Control-Allow-Origin: {origin}\r\n\
         Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, X-Bridge-Token, X-Api-Key\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        len = body.len()
    )
}

fn prometheus_response(body: &str) -> String {
    let origin = cors_origin();
    format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/plain; version=0.0.4\r\n\
         Content-Length: {len}\r\n\
         Access-Control-Allow-Origin: {origin}\r\n\
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

fn ok(body: &str) -> String {
    json_response(200, body)
}
fn err(body: &str) -> String {
    json_response(500, body)
}
fn not_found() -> String {
    json_response(404, r#"{"error":"not found"}"#)
}
fn bad_request(msg: &str) -> String {
    json_response(400, &format!(r#"{{"error":"{msg}"}}"#))
}
#[allow(dead_code)]
fn unauthorized(msg: &str) -> String {
    json_response(
        401,
        &format!(r#"{{"error":"unauthenticated","message":"{}"}}"#, msg),
    )
}

// ── Connection handler ────────────────────────────────────────────────────────

fn handle(mut stream: TcpStream, state: SharedState) -> std::io::Result<()> {
    let start = Instant::now();
    let req_id = next_request_id();
    let req = match parse_request(&stream) {
        Some(r) => r,
        None => return Ok(()),
    };
    let method = req.method.clone();
    let path = req.path.clone();

    // CORS preflight — always allowed, no auth check
    if method == "OPTIONS" {
        stream.write_all(cors_preflight().as_bytes())?;
        return stream.flush();
    }

    // SSE hot-reload stream — handled inline, never returns normal response
    let clean_path = path.split('?').next().unwrap_or(&path);
    if method == "GET" && clean_path == "/api/v1/watch/events" {
        return handle_sse(stream, state);
    }

    // State-streaming SSE endpoints (traces / metrics / services)
    if method == "GET"
        && matches!(
            clean_path,
            "/api/v1/stream/traces" | "/api/v1/stream/metrics" | "/api/v1/stream/services"
        )
    {
        return handle_state_stream(stream, state, clean_path);
    }

    // Static file mounts — byte-accurate responses, only for non-API paths.
    // Registered API routes always win; a "/" mount serves everything else
    // (SPA-style) and falls through to normal 404 when nothing matches.
    let is_api_route = clean_path == "/"
        || clean_path.starts_with("/api/")
        || matches!(
            clean_path,
            "/health" | "/mode" | "/compile" | "/services" | "/routes"
        )
        || clean_path.starts_with("/codegen/")
        || clean_path.starts_with("/db/")
        || clean_path.starts_with("/redis/");
    if !is_api_route && (method == "GET" || method == "HEAD") {
        let mount_hit = {
            let g = state.lock().unwrap();
            g.static_files.matches(clean_path)
        };
        if !mount_hit {
            return serve_request(stream, state, &req, &req_id, start);
        }
        if let Some(bytes) = static_byte_response(&req, &state, clean_path) {
            return stream.write_all(&bytes).and_then(|_| stream.flush());
        }
    }

    serve_request(stream, state, &req, &req_id, start)
}

/// Parse-complete request → route → trace/log → write response.
fn serve_request(
    mut stream: TcpStream,
    state: SharedState,
    req: &Request,
    req_id: &str,
    start: Instant,
) -> std::io::Result<()> {
    let method = req.method.clone();
    let path = req.path.clone();

    // route() handles auth enforcement + request-ID injection
    let response = route(req, &state, req_id);

    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(200);
    let elapsed = start.elapsed().as_millis() as u64;

    if let Ok(mut g) = state.lock() {
        g.push_trace(&method, &path, status, elapsed);
        g.push_log(
            if status >= 500 {
                LogLevel::Error
            } else {
                LogLevel::Info
            },
            &format!("{method} {path} {status} {elapsed}ms req_id={req_id}"),
            Default::default(),
        );
    }

    stream.write_all(response.as_bytes())?;
    stream.flush()
}

/// Handle a long-lived SSE connection for hot-reload events.
fn handle_sse(mut stream: TcpStream, state: SharedState) -> std::io::Result<()> {
    let origin = cors_origin();
    // Write SSE headers — keep connection open
    let headers = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/event-stream\r\n\
         Cache-Control: no-cache\r\n\
         Access-Control-Allow-Origin: {origin}\r\n\
         Connection: keep-alive\r\n\
         Transfer-Encoding: chunked\r\n\
         \r\n"
    );
    stream.write_all(headers.as_bytes())?;
    stream.flush()?;

    // Register this client
    let (client_id, rx) = {
        let mut g = state.lock().unwrap();
        g.watcher.add_sse_client()
    };

    // Send a connection-established ping
    let ping = ": connected\n\n";
    write_chunk(&mut stream, ping)?;

    // Drain events from channel and forward to client
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(15)) {
            Ok(msg) => {
                if write_chunk(&mut stream, &msg).is_err() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Send SSE keepalive comment
                if write_chunk(&mut stream, ": keepalive\n\n").is_err() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Cleanup client registration
    if let Ok(mut g) = state.lock() {
        g.watcher.remove_sse_client(client_id);
    }
    Ok(())
}

/// Write a single chunk in HTTP chunked transfer encoding.
fn write_chunk(stream: &mut TcpStream, data: &str) -> std::io::Result<()> {
    use std::io::Write;
    stream.write_all(format!("{:x}\r\n{}\r\n", data.len(), data).as_bytes())?;
    stream.flush()
}

/// Long-lived SSE stream of daemon state (traces / metrics / services).
///
/// Registers a session in `StreamRegistry` for the endpoint's lifetime,
/// polls the matching renderer every 2s (15s keepalive between quiet
/// periods), and always deregisters on disconnect so `active_count()`
/// reflects live clients.
fn handle_state_stream(
    mut stream: TcpStream,
    state: SharedState,
    endpoint: &str,
) -> std::io::Result<()> {
    let origin = cors_origin();
    let headers = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/event-stream\r\n\
         Cache-Control: no-cache\r\n\
         Access-Control-Allow-Origin: {origin}\r\n\
         Connection: keep-alive\r\n\
         Transfer-Encoding: chunked\r\n\
         \r\n"
    );
    stream.write_all(headers.as_bytes())?;
    stream.flush()?;

    // Normalize the path and register the session.
    let session = {
        let mut g = state.lock().unwrap();
        match g.streams.open(endpoint) {
            Some(ep) => g.streams.set_open(&ep),
            None => return Ok(()), // unknown endpoint: close quietly
        }
    };

    // Initial burst: current state immediately.
    let initial = render_stream_snapshot(&state, endpoint);
    if write_chunk(&mut stream, &initial).is_err() {
        finish_state_stream(&state, &session);
        return Ok(());
    }

    // Poll loop — render fresh state each tick.
    let tick = std::time::Duration::from_secs(2);
    loop {
        std::thread::sleep(tick);
        let frame = render_stream_snapshot(&state, endpoint);
        if write_chunk(&mut stream, &frame).is_err() {
            break;
        }
    }

    finish_state_stream(&state, &session);
    Ok(())
}

/// Deregister a stream session (best-effort; state may be poisoned).
fn finish_state_stream(state: &SharedState, session: &str) {
    if let Ok(mut g) = state.lock() {
        g.streams.close(session);
    }
}

/// Render one poll of the requested stream as SSE text.
fn render_stream_snapshot(state: &SharedState, endpoint: &str) -> String {
    let g = state.lock().unwrap();
    match endpoint {
        "/api/v1/stream/traces" => crate::streaming::render_traces(&g, 50),
        "/api/v1/stream/metrics" => crate::streaming::render_metrics(&g),
        "/api/v1/stream/services" => crate::streaming::render_services(&g),
        _ => String::new(),
    }
}

// ── Router ────────────────────────────────────────────────────────────────────

fn route(req: &Request, state: &SharedState, req_id: &str) -> String {
    let path = req.path.split('?').next().unwrap_or(&req.path);

    // Auth middleware — enforce token on non-public endpoints
    if should_enforce_auth(path) {
        let configured_token = state.lock().unwrap().auth_token.clone();
        if let Some(expected) = configured_token {
            if let Err(msg) = check_auth(req, &expected) {
                let body = format!(r#"{{"error":"unauthenticated","message":"{}"}}"#, msg);
                return inject_request_id(json_response(401, &body), req_id);
            }
        }
    }

    // Rate limiting — checked before middleware chain
    {
        let mut g = state.lock().unwrap();
        match g.rate_limiter.check(&req.method, path) {
            Some(Err(retry)) => {
                let body = format!(r#"{{"error":"rate limit exceeded","retry_after":{retry}}}"#);
                let mut resp = json_response(429, &body);
                resp = inject_header(resp, "Retry-After", &retry.to_string());
                resp = inject_header(resp, "X-RateLimit-Remaining", "0");
                return inject_request_id(resp, req_id);
            }
            Some(Ok((cap, rem, reset))) => {
                // Store RL info to inject headers below
                drop(g);
                let inner = route_after_rl(req, state, path, req_id);
                let mut resp = inject_header(inner, "X-RateLimit-Limit", &cap.to_string());
                resp = inject_header(resp, "X-RateLimit-Remaining", &rem.to_string());
                resp = inject_header(resp, "X-RateLimit-Reset", &reset.to_string());
                return resp;
            }
            None => {} // no rule — fall through
        }
    }

    route_after_rl(req, state, path, req_id)
}

/// Common path after rate-limit check: run middleware chain then handler.
fn route_after_rl(req: &Request, state: &SharedState, path: &str, req_id: &str) -> String {
    // Middleware chain — before hooks
    let mut mw_ctx = MiddlewareContext::new(&req.method, path, req_id);
    {
        let g = state.lock().unwrap();
        g.middleware.run_before(&mut mw_ctx);
    }

    // If a middleware rejected the request, return early
    if let Some((status, body)) = mw_ctx.rejection {
        let resp = json_response(status, &body);
        return inject_request_id(inject_extra_headers(resp, &mw_ctx.extra_headers), req_id);
    }

    // Body validation — registered schemas short-circuit with 400 + violations
    if req.method == "POST" || req.method == "PUT" || req.method == "PATCH" {
        let violations = {
            let g = state.lock().unwrap();
            g.validation.validate_body(&req.method, path, &req.body)
        };
        if !violations.is_empty() {
            let resp = json_response(400, &violations_json(&violations));
            return inject_request_id(inject_extra_headers(resp, &mw_ctx.extra_headers), req_id);
        }
    }

    // Run the actual handler
    let mut inner = route_inner(req, state, path);

    // Middleware chain — after hooks
    {
        let g = state.lock().unwrap();
        g.middleware.run_after(&mut mw_ctx);
    }

    // Inject any headers added by after hooks
    inner = inject_extra_headers(inner, &mw_ctx.extra_headers);
    inject_request_id(inner, req_id)
}

/// Inject X-Bridge-Request-Id into any HTTP response string.
fn inject_request_id(mut response: String, req_id: &str) -> String {
    if req_id.is_empty() {
        return response;
    }
    let header = format!("X-Bridge-Request-Id: {req_id}\r\n");
    if let Some(pos) = response.find("Connection: close") {
        response.insert_str(pos, &header);
    }
    response
}

/// Inject extra headers (from middleware context) into a response string.
fn inject_extra_headers(
    mut response: String,
    headers: &std::collections::HashMap<String, String>,
) -> String {
    if headers.is_empty() {
        return response;
    }
    let mut to_inject = String::new();
    for (k, v) in headers {
        to_inject.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(pos) = response.find("Connection: close") {
        response.insert_str(pos, &to_inject);
    }
    response
}

/// Inject a single named header into a response string.
fn inject_header(mut response: String, key: &str, value: &str) -> String {
    let header = format!("{key}: {value}\r\n");
    if let Some(pos) = response.find("Connection: close") {
        response.insert_str(pos, &header);
    }
    response
}

fn route_inner(req: &Request, state: &SharedState, path: &str) -> String {
    match (req.method.as_str(), path) {
        // ── Legacy health / mode ──────────────────────────────────────────
        ("GET", "/health") => health(state),
        ("GET", "/mode") => get_mode(state),
        ("POST", "/mode") => set_mode(req, state),

        // ── Legacy compile / services ─────────────────────────────────────
        ("POST", "/compile") => compile(req, state),
        ("GET", "/services") => services(state),
        ("GET", "/routes") => routes(state),

        // ── Legacy codegen ────────────────────────────────────────────────
        ("GET", "/codegen/latest") => codegen_latest(state),

        // ── Legacy DB / Redis ─────────────────────────────────────────────
        ("GET", "/db/status") => db_status(),
        ("POST", "/db/create") => db_create(req),
        ("POST", "/db/migrate") => db_migrate(req),
        ("DELETE", "/db/destroy") => db_destroy(req),
        ("GET", "/redis/status") => redis_status(state),

        // ── v1 API ────────────────────────────────────────────────────────
        ("GET", "/api/v1/health") => health(state),
        ("GET", "/api/v1/version") => version(),
        ("GET", "/api/v1/mode") => get_mode(state),
        ("POST", "/api/v1/mode") => set_mode(req, state),
        ("POST", "/api/v1/compile") => compile(req, state),
        ("GET", "/api/v1/services") => services(state),
        ("GET", "/api/v1/routes") => routes(state),
        ("GET", "/api/v1/codegen/latest") => codegen_latest(state),
        ("GET", "/api/v1/pg/status") => db_status(),
        ("POST", "/api/v1/pg/create") => db_create(req),
        ("POST", "/api/v1/pg/migrate") => db_migrate(req),
        ("DELETE", "/api/v1/pg/destroy") => db_destroy(req),
        ("GET", "/api/v1/redis/status") => redis_status(state),
        ("GET", "/api/v1/auth/status") => auth_status(state),
        ("POST", "/api/v1/auth/set") => auth_set(req, state),
        ("DELETE", "/api/v1/auth/clear") => auth_clear(state),
        ("GET", "/api/v1/traces") => traces_list(req, state),
        ("DELETE", "/api/v1/traces") => traces_clear(state),
        ("GET", p) if p.starts_with("/api/v1/traces/") => {
            let id = p.trim_start_matches("/api/v1/traces/");
            trace_get(id, state)
        }
        ("GET", "/api/v1/metrics") => metrics(state),
        ("GET", "/api/v1/metrics/prometheus") => metrics_prometheus(state),
        ("DELETE", "/api/v1/metrics") => metrics_clear(state),
        ("GET", "/api/v1/openapi") => openapi(state),
        ("POST", "/api/v1/sampling") => set_sampling(req, state),

        // ── Streaming endpoints ───────────────────────────────────────────
        ("GET", "/api/v1/stream/traces") => stream_traces(state, &req),
        ("GET", "/api/v1/stream/metrics") => stream_metrics(state, &req),
        ("GET", "/api/v1/stream/services") => stream_services(state, &req),

        // ── Middleware ────────────────────────────────────────────────────
        ("GET", "/api/v1/middleware") => middleware_list(state),
        ("POST", "/api/v1/middleware") => middleware_register(req, state),
        ("DELETE", "/api/v1/middleware") => middleware_remove(req, state),

        // ── Hot reload / watcher ──────────────────────────────────────────
        ("GET", "/api/v1/watch") => watch_status(state),
        ("POST", "/api/v1/watch/files") => watch_add_file(req, state),
        ("DELETE", "/api/v1/watch/files") => watch_remove_file(req, state),
        ("POST", "/api/v1/watch/dirs") => watch_add_dir(req, state),

        // ── Rate limiting ─────────────────────────────────────────────────
        ("GET", "/api/v1/ratelimit") => ratelimit_list(state),
        ("POST", "/api/v1/ratelimit") => ratelimit_add(req, state),
        ("DELETE", "/api/v1/ratelimit") => ratelimit_remove(req, state),

        // ── Config ────────────────────────────────────────────────────────
        ("GET", "/api/v1/config") => config_show(state),

        // ── Validation ────────────────────────────────────────────────────
        ("GET", "/api/v1/validate") => validation_list(state),
        ("POST", "/api/v1/validate") => validation_register(req, state),
        ("DELETE", "/api/v1/validate") => validation_remove(req, state),

        // ── Static files ──────────────────────────────────────────────────
        ("GET", "/api/v1/static") => static_list(state),
        ("POST", "/api/v1/static") => static_register(req, state),
        ("DELETE", "/api/v1/static") => static_remove(req, state),

        // ── Catch-all ─────────────────────────────────────────────────────
        _ => not_found(),
    }
}

// ── Auth helpers ──────────────────────────────────────────────────────────────

/// Returns true for endpoints that require auth when a token is configured.
/// Health, version, and auth-management paths are always public.
pub fn should_enforce_auth(path: &str) -> bool {
    !matches!(
        path,
        "/health"
            | "/api/v1/health"
            | "/api/v1/version"
            | "/api/v1/auth/status"
            | "/api/v1/auth/set"
            | "/api/v1/auth/clear"
    )
}

/// Validate the request's auth headers against the configured token.
/// Accepts: `Authorization: Bearer <token>`, `X-Api-Key: <key>`, `X-Bridge-Token: <tok>`.
pub(crate) fn check_auth(req: &Request, expected: &str) -> Result<(), String> {
    if let Some(hdr) = req.header("authorization") {
        let tok = hdr.strip_prefix("Bearer ").unwrap_or(hdr).trim();
        return if tok == expected {
            Ok(())
        } else {
            Err("invalid bearer token".into())
        };
    }
    if let Some(key) = req.header("x-api-key") {
        return if key.trim() == expected {
            Ok(())
        } else {
            Err("invalid API key".into())
        };
    }
    if let Some(tok) = req.header("x-bridge-token") {
        return if tok.trim() == expected {
            Ok(())
        } else {
            Err("invalid bridge token".into())
        };
    }
    Err("authentication required".into())
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
            let items: Vec<String> = f
                .services
                .iter()
                .map(|s| {
                    format!(
                        r#"{{"name":"{}","auth":"{}","endpoints":{}}}"#,
                        s.name,
                        s.auth.as_str(),
                        s.endpoints.len()
                    )
                })
                .collect();
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
                        svc.name,
                        ep.name,
                        ep.method.as_str(),
                        ep.path
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
        Ok(msg) => ok(&msg),
        Err(msg) => err(&format!(r#"{{"error":"{msg}"}}"#)),
    }
}

fn db_create(req: &Request) -> String {
    let name = req.body.trim();
    if name.is_empty() {
        return bad_request("name required");
    }
    match sqldb::create(name) {
        Ok(msg) => ok(&format!(r#"{{"message":"{msg}"}}"#)),
        Err(msg) => err(&format!(r#"{{"error":"{msg}"}}"#)),
    }
}

fn db_migrate(req: &Request) -> String {
    let sql = req.body.trim();
    if sql.is_empty() {
        return bad_request("sql required");
    }
    match sqldb::migrate(sql) {
        Ok(msg) => ok(&format!(r#"{{"message":"{msg}"}}"#)),
        Err(msg) => err(&format!(r#"{{"error":"{msg}"}}"#)),
    }
}

fn db_destroy(req: &Request) -> String {
    let name = req.body.trim();
    if name.is_empty() {
        return bad_request("name required");
    }
    match sqldb::destroy(name) {
        Ok(msg) => ok(&format!(r#"{{"message":"{msg}"}}"#)),
        Err(msg) => err(&format!(r#"{{"error":"{msg}"}}"#)),
    }
}

fn redis_status(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    let addr = g.redis_addr.clone().unwrap_or_else(|| "not running".into());
    let conns = g.redis_connections_count();
    ok(&format!(r#"{{"addr":"{addr}","connections":{conns}}}"#))
}

fn auth_status(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    let scheme = g.auth_token.as_ref().map(|_| "bearer").unwrap_or("none");
    ok(&format!(
        r#"{{"configured":{},"scheme":"{}"}}"#,
        g.auth_token.is_some(),
        scheme
    ))
}

/// Accept either plain text token or JSON body:
/// `{"scheme":"bearer","token":"my-secret"}` or just `my-secret`
fn auth_set(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    if body.is_empty() {
        return bad_request("token required");
    }

    let token = if body.starts_with('{') {
        // Parse JSON: look for "token" field
        extract_json_field(body, "token").unwrap_or_else(|| body.to_string())
    } else {
        // Plain string token (strip surrounding quotes if present)
        body.trim_matches('"').to_string()
    };

    if token.is_empty() {
        return bad_request("token value is empty");
    }
    let scheme = if body.starts_with('{') {
        extract_json_field(body, "scheme").unwrap_or_else(|| "bearer".to_string())
    } else {
        "bearer".to_string()
    };

    state.lock().unwrap().auth_token = Some(token);
    ok(&format!(
        r#"{{"message":"auth token set","scheme":"{}"}}"#,
        scheme
    ))
}

/// Minimal JSON field extractor for simple flat objects (no serde dependency).
fn extract_json_field(json: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\"", field);
    let pos = json.find(&needle)?;
    let rest = json[pos + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if rest.starts_with('"') {
        let inner = &rest[1..];
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else {
        // non-string value — take until , or }
        let end = rest.find(|c| c == ',' || c == '}').unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

fn auth_clear(state: &SharedState) -> String {
    state.lock().unwrap().auth_token = None;
    ok(r#"{"message":"auth token cleared"}"#)
}

fn traces_list(req: &Request, state: &SharedState) -> String {
    let limit = req
        .path
        .split('?')
        .nth(1)
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
        None => not_found(),
    }
}

fn metrics(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.metrics.to_json())
}

/// Prometheus text format at /api/v1/metrics/prometheus
fn metrics_prometheus(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    let m = &g.metrics;
    let mut lines = Vec::new();

    lines.push("# HELP bridge_requests_total Total HTTP requests processed".to_string());
    lines.push("# TYPE bridge_requests_total counter".to_string());
    lines.push(format!("bridge_requests_total {}", m.total_requests));

    lines.push("# HELP bridge_errors_total Total HTTP errors (status >= 400)".to_string());
    lines.push("# TYPE bridge_errors_total counter".to_string());
    lines.push(format!("bridge_errors_total {}", m.total_errors));

    for (key, count) in &m.request_counts {
        let label = key.replace(' ', "_").replace('/', "_").replace('-', "_");
        lines.push(format!(
            "bridge_endpoint_requests_total{{endpoint=\"{}\"}} {}",
            label, count
        ));
    }
    for (key, errs) in &m.error_counts {
        let label = key.replace(' ', "_").replace('/', "_").replace('-', "_");
        lines.push(format!(
            "bridge_endpoint_errors_total{{endpoint=\"{}\"}} {}",
            label, errs
        ));
    }

    lines.push(String::new()); // trailing newline
    prometheus_response(&lines.join("\n"))
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

// ── Middleware handlers ───────────────────────────────────────────────────────

fn middleware_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.middleware.to_json())
}

/// Register a named middleware from JSON body.
///
/// Body format:
/// ```json
/// {
///   "name": "my-logger",
///   "scope": "global" | "service:users" | "GET:/ping",
///   "before": "log" | "reject:403:forbidden" | null,
///   "after":  "header:X-Timing:done"        | null
/// }
/// ```
///
/// Supported built-in hook specs:
/// - `"log"`                    — tag the context with the middleware name
/// - `"reject:<status>:<msg>"` — reject with given status and JSON error body
/// - `"header:<key>:<value>"`  — inject a response header (after hook)
fn middleware_register(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    if body.is_empty() {
        return bad_request("body required");
    }

    let name = match extract_json_field(body, "name") {
        Some(n) if !n.is_empty() => n,
        _ => return bad_request("name is required"),
    };
    let scope_str = extract_json_field(body, "scope").unwrap_or_else(|| "global".to_string());
    let scope = match parse_scope(&scope_str) {
        Ok(s) => s,
        Err(e) => return bad_request(&e),
    };
    let before_spec = extract_json_field(body, "before");
    let after_spec = extract_json_field(body, "after");

    let mut builder = MiddlewareBuilder::new(&name).scope(scope);

    if let Some(spec) = before_spec {
        if spec != "null" {
            match build_hook_before(&spec) {
                Ok(hook) => builder = builder.before(hook),
                Err(e) => return bad_request(&e),
            }
        }
    }
    if let Some(spec) = after_spec {
        if spec != "null" {
            match build_hook_after(&spec) {
                Ok(hook) => builder = builder.after(hook),
                Err(e) => return bad_request(&e),
            }
        }
    }

    let mut g = state.lock().unwrap();
    // Replace if name already exists
    g.middleware.remove(&name);
    let idx = g.middleware.register(builder.build());
    ok(&format!(
        r#"{{"message":"middleware registered","name":"{name}","index":{idx}}}"#
    ))
}

fn middleware_remove(req: &Request, state: &SharedState) -> String {
    let name = if req.body.trim().starts_with('{') {
        extract_json_field(req.body.trim(), "name")
    } else {
        Some(req.body.trim().trim_matches('"').to_string())
    };
    match name {
        Some(n) if !n.is_empty() => {
            let removed = state.lock().unwrap().middleware.remove(&n);
            if removed {
                ok(&format!(
                    r#"{{"message":"middleware removed","name":"{n}"}}"#
                ))
            } else {
                json_response(
                    404,
                    &format!(r#"{{"error":"middleware not found","name":"{n}"}}"#),
                )
            }
        }
        _ => bad_request("name required"),
    }
}

/// Parse a scope string: "global" | "service:NAME" | "METHOD:/path"
pub fn parse_scope(s: &str) -> Result<Scope, String> {
    if s == "global" {
        return Ok(Scope::Global);
    }
    if let Some(name) = s.strip_prefix("service:") {
        return Ok(Scope::Service(name.to_string()));
    }
    // Endpoint: "GET:/ping"
    if let Some(colon) = s.find(':') {
        let method = s[..colon].to_uppercase();
        let path = s[colon + 1..].to_string();
        if !method.is_empty() && !path.is_empty() {
            return Ok(Scope::Endpoint { method, path });
        }
    }
    Err(format!(
        "invalid scope: {s:?} — use \"global\", \"service:NAME\", or \"METHOD:/path\""
    ))
}

/// Build a before-hook from a spec string.
pub fn build_hook_before(spec: &str) -> Result<crate::middleware::Hook, String> {
    if spec == "log" {
        return Ok(Box::new(|ctx| ctx.tag("logged")));
    }
    if let Some(rest) = spec.strip_prefix("reject:") {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        let status: u16 = parts
            .first()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("reject spec must be reject:<status>:<msg>, got {spec:?}"))?;
        let msg = parts.get(1).copied().unwrap_or("rejected").to_string();
        return Ok(Box::new(move |ctx| {
            ctx.reject(status, format!(r#"{{"error":"{msg}"}}"#));
        }));
    }
    Err(format!(
        "unknown before spec: {spec:?} — supported: \"log\", \"reject:<status>:<msg>\""
    ))
}

/// Build an after-hook from a spec string.
pub fn build_hook_after(spec: &str) -> Result<crate::middleware::Hook, String> {
    if let Some(rest) = spec.strip_prefix("header:") {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        let key = parts.first().copied().unwrap_or("").to_string();
        let val = parts.get(1).copied().unwrap_or("").to_string();
        if key.is_empty() {
            return Err(format!(
                "header spec must be header:<key>:<value>, got {spec:?}"
            ));
        }
        return Ok(Box::new(move |ctx| {
            ctx.set_header(key.clone(), val.clone())
        }));
    }
    if spec == "log" {
        return Ok(Box::new(|ctx| ctx.tag("logged-after")));
    }
    Err(format!(
        "unknown after spec: {spec:?} — supported: \"log\", \"header:<key>:<value>\""
    ))
}

// ── Validation handlers ───────────────────────────────────────────────────────

fn validation_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.validation.to_json())
}

/// Register validation rules for one field on an endpoint.
///
/// Body format:
/// ```json
/// {
///   "endpoint": "POST:/users",
///   "field":    "email",
///   "rules":    ["required", "isEmail", "maxLen:255"]
/// }
/// ```
///
/// Rule vocabulary mirrors `encore.dev/validate`: `required`, `minLen:n`,
/// `maxLen:n`, `min:x`, `max:x`, `matches:<regexp>`, `startsWith:s`,
/// `endsWith:s`, `isEmail`, `isURL`. `"rules"` also accepts the compact
/// string form `"required,isEmail"`.
fn validation_register(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    if body.is_empty() {
        return bad_request("body required");
    }
    let endpoint = match extract_json_field(body, "endpoint") {
        Some(e) if !e.is_empty() => e,
        _ => return bad_request("endpoint is required (format \"METHOD:/path\")"),
    };
    // Accept both "METHOD:/path" and bare "/path" (defaults to POST).
    let endpoint = if endpoint.contains(':') {
        endpoint
    } else {
        format!("POST:{endpoint}")
    };
    let field = match extract_json_field(body, "field") {
        Some(f) if !f.is_empty() => f,
        _ => return bad_request("field is required"),
    };
    let Some(rule_specs) = validation::parse_rules_field(body) else {
        return bad_request("rules is required (array or comma-separated string)");
    };

    let mut rules = Vec::new();
    for spec in &rule_specs {
        match Rule::parse(spec) {
            Ok(r) => rules.push(r),
            Err(e) => return bad_request(&e),
        }
    }

    let mut g = state.lock().unwrap();
    let count = g.validation.add_field(&endpoint, &field, rules);
    ok(&format!(
        r#"{{"message":"validation registered","endpoint":"{endpoint}","field":"{field}","fields":{count}}}"#
    ))
}

fn validation_remove(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    if body.is_empty() || !body.starts_with('{') {
        return bad_request("JSON body required");
    }
    let endpoint = match extract_json_field(body, "endpoint") {
        Some(e) if !e.is_empty() => e,
        _ => return bad_request("endpoint is required"),
    };
    match extract_json_field(body, "field") {
        Some(field) if !field.is_empty() => {
            let removed = state
                .lock()
                .unwrap()
                .validation
                .remove_field(&endpoint, &field);
            if removed {
                ok(&format!(
                    r#"{{"message":"validation removed","endpoint":"{endpoint}","field":"{field}"}}"#
                ))
            } else {
                json_response(
                    404,
                    &format!(
                        r#"{{"error":"no such field rule","endpoint":"{endpoint}","field":"{field}"}}"#
                    ),
                )
            }
        }
        _ => {
            // No field — drop the whole endpoint schema.
            let removed = state.lock().unwrap().validation.remove_endpoint(&endpoint);
            if removed {
                ok(&format!(
                    r#"{{"message":"validation schema removed","endpoint":"{endpoint}"}}"#
                ))
            } else {
                json_response(
                    404,
                    &format!(r#"{{"error":"schema not found","endpoint":"{endpoint}"}}"#),
                )
            }
        }
    }
}

// ── Static file handlers ──────────────────────────────────────────────────────

fn static_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.static_files.to_json())
}

/// Register a static mount.
///
/// Body format:
/// ```json
/// {
///   "prefix":   "/assets",
///   "dir":      "./public",
///   "fallback": "./public/index.html",         // optional SPA fallback
///   "headers":  {"Cache-Control": "max-age=3600"} // optional
/// }
/// ```
fn static_register(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    if body.is_empty() {
        return bad_request("body required");
    }
    let prefix = match extract_json_field(body, "prefix") {
        Some(p) if !p.is_empty() => p,
        _ => return bad_request("prefix is required"),
    };
    let dir = match extract_json_field(body, "dir") {
        Some(d) if !d.is_empty() => d,
        _ => return bad_request("dir is required"),
    };
    if !std::path::Path::new(&dir).is_dir() {
        return bad_request(&format!("dir is not a directory: {dir}"));
    }
    let mut mount = StaticMount::new(prefix, dir);
    if let Some(fb) = extract_json_field(body, "fallback") {
        if fb != "null" && !fb.is_empty() {
            mount = mount.with_fallback(fb);
        }
    }
    if let Some(headers_obj) = extract_json_object(body, "headers") {
        for (k, v) in parse_flat_json_object(&headers_obj) {
            mount = mount.with_header(k, v);
        }
    }

    let mut g = state.lock().unwrap();
    let count = g.static_files.register(mount);
    ok(&format!(
        r#"{{"message":"static mount registered","mounts":{count}}}"#
    ))
}

fn static_remove(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let prefix = if body.starts_with('{') {
        extract_json_field(body, "prefix")
    } else {
        Some(body.trim_matches('"').to_string())
    };
    match prefix {
        Some(p) if !p.is_empty() => {
            let removed = state.lock().unwrap().static_files.remove(&p);
            if removed {
                ok(&format!(
                    r#"{{"message":"static mount removed","prefix":"{p}"}}"#
                ))
            } else {
                json_response(
                    404,
                    &format!(r#"{{"error":"mount not found","prefix":"{p}"}}"#),
                )
            }
        }
        _ => bad_request("prefix required"),
    }
}

/// Try serving `path` from static mounts as raw bytes. Returns None when no
/// mount matches (caller falls through to normal routing).
///
/// Byte-accurate on purpose: static responses bypass the String pipeline so
/// binary assets (images, fonts, wasm) arrive uncorrupted.
pub(crate) fn static_byte_response(
    req: &Request,
    state: &SharedState,
    path: &str,
) -> Option<Vec<u8>> {
    let result = {
        let g = state.lock().unwrap();
        g.static_files.serve(
            req.method.as_str(),
            path,
            req.header("if-none-match"),
            req.header("if-modified-since"),
        )
    };
    let mut out = Vec::new();
    match result {
        StaticResult::NotFound => None,
        StaticResult::NotModified(headers) => {
            out.extend_from_slice(b"HTTP/1.1 304 Not Modified\r\n");
            for (k, v) in &headers {
                out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
            }
            out.extend_from_slice(
                format!("Access-Control-Allow-Origin: {}\r\n", cors_origin()).as_bytes(),
            );
            out.extend_from_slice(b"Connection: close\r\n\r\n");
            Some(out)
        }
        StaticResult::Found {
            status,
            headers,
            body,
        } => {
            let reason = if status == 200 { "OK" } else { "Unknown" };
            out.extend_from_slice(format!("HTTP/1.1 {status} {reason}\r\n").as_bytes());
            for (k, v) in &headers {
                out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
            }
            out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
            out.extend_from_slice(
                format!("Access-Control-Allow-Origin: {}\r\n", cors_origin()).as_bytes(),
            );
            out.extend_from_slice(b"Connection: close\r\n\r\n");
            out.extend_from_slice(&body);
            Some(out)
        }
    }
}

/// Extract the full object literal following `"key": { ... }` (balanced braces).
fn extract_json_object(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = json.find(&needle)?;
    let rest = json[pos + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let start = json.len() - rest.len();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut prev_esc = false;
    for (i, c) in json[start..].char_indices() {
        match c {
            '"' if !prev_esc => in_str = !in_str,
            '{' if !in_str => depth += 1,
            '}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return Some(json[start..=start + i].to_string());
                }
            }
            _ => {}
        }
        prev_esc = c == '\\' && !prev_esc;
    }
    None
}

/// Parse a flat `{ "k": "v", ... }` object into pairs.
fn parse_flat_json_object(obj: &str) -> Vec<(String, String)> {
    let inner = obj.trim().trim_start_matches('{').trim_end_matches('}');
    inner
        .split(',')
        .filter_map(|pair| {
            let (k, v) = pair.split_once(':')?;
            Some((
                k.trim().trim_matches('"').to_string(),
                v.trim().trim_matches('"').to_string(),
            ))
        })
        .collect()
}

// ── Watch / hot-reload handlers ───────────────────────────────────────────────

fn watch_status(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.watcher.to_json())
}

fn watch_add_file(req: &Request, state: &SharedState) -> String {
    let path = req.body.trim().trim_matches('"');
    if path.is_empty() {
        return bad_request("file path required");
    }
    if !path.ends_with(".bridge") {
        return bad_request("only .bridge files can be watched");
    }
    state.lock().unwrap().watcher.watch_file(path);
    ok(&format!(r#"{{"message":"watching","path":"{path}"}}"#))
}

fn watch_remove_file(req: &Request, state: &SharedState) -> String {
    let path = if req.body.trim().starts_with('{') {
        extract_json_field(req.body.trim(), "path")
    } else {
        Some(req.body.trim().trim_matches('"').to_string())
    };
    match path {
        Some(p) if !p.is_empty() => {
            let removed = state.lock().unwrap().watcher.unwatch(&p);
            if removed {
                ok(&format!(r#"{{"message":"unwatched","path":"{p}"}}"#))
            } else {
                json_response(404, &format!(r#"{{"error":"not watched","path":"{p}"}}"#))
            }
        }
        _ => bad_request("path required"),
    }
}

fn watch_add_dir(req: &Request, state: &SharedState) -> String {
    let dir = req.body.trim().trim_matches('"');
    if dir.is_empty() {
        return bad_request("directory path required");
    }
    let added = {
        let mut g = state.lock().unwrap();
        let before = g.watcher.files.len();
        g.watcher.watch_dir(dir);
        g.watcher.files.len() - before
    };
    ok(&format!(
        r#"{{"message":"watching directory","dir":"{dir}","new_files":{added}}}"#
    ))
}

// ── Rate-limit handlers ───────────────────────────────────────────────────────

fn ratelimit_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.rate_limiter.to_json())
}

/// Add a rate-limit rule from JSON body.
///
/// ```json
/// {"method":"GET","path":"/api/v1/users","capacity":100,"refill_rate":10}
/// ```
///
/// - `method`: HTTP method or `"*"` for any
/// - `path`:   exact path or `"*"` for any
/// - `capacity`:    maximum burst tokens
/// - `refill_rate`: tokens refilled per second
fn ratelimit_add(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    if body.is_empty() {
        return bad_request("body required");
    }

    let method = extract_json_field(body, "method").unwrap_or_else(|| "*".to_string());
    let path = extract_json_field(body, "path").unwrap_or_else(|| "*".to_string());
    let capacity: u64 = match extract_json_field(body, "capacity").and_then(|s| s.parse().ok()) {
        Some(c) if c > 0 => c,
        _ => return bad_request("capacity must be a positive integer"),
    };
    let refill_rate: f64 =
        match extract_json_field(body, "refill_rate").and_then(|s| s.parse().ok()) {
            Some(r) if r > 0.0 => r,
            _ => return bad_request("refill_rate must be a positive number"),
        };

    let key = BucketKey::new(method.to_uppercase(), &path);
    state
        .lock()
        .unwrap()
        .rate_limiter
        .add_rule(key, capacity, refill_rate);
    ok(&format!(
        r#"{{"message":"rate limit added","method":"{method}","path":"{path}","capacity":{capacity},"refill_rate":{refill_rate}}}"#
    ))
}

fn ratelimit_remove(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let method = if body.starts_with('{') {
        extract_json_field(body, "method").unwrap_or_else(|| "*".to_string())
    } else {
        "*".to_string()
    };
    let path = if body.starts_with('{') {
        extract_json_field(body, "path")
    } else {
        Some(body.trim_matches('"').to_string())
    };

    match path {
        Some(p) if !p.is_empty() => {
            let key = BucketKey::new(method.to_uppercase(), &p);
            let removed = state.lock().unwrap().rate_limiter.remove_rule(&key);
            if removed {
                ok(&format!(
                    r#"{{"message":"rate limit removed","method":"{method}","path":"{p}"}}"#
                ))
            } else {
                json_response(
                    404,
                    &format!(r#"{{"error":"rule not found","method":"{method}","path":"{p}"}}"#),
                )
            }
        }
        _ => bad_request("path required"),
    }
}

// ── Config handler ────────────────────────────────────────────────────────────

/// Return a JSON summary of the current effective runtime configuration.
fn config_show(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    let middleware_names: Vec<String> = g
        .middleware
        .names()
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect();
    let rl_count = g.rate_limiter.to_json();
    let watch_files: Vec<String> = g
        .watcher
        .files
        .iter()
        .map(|f| format!("\"{}\"", f.path))
        .collect();
    let body = format!(
        r#"{{"app":"{app}","version":"{ver}","mode":"{mode}","middleware":[{mw}],"ratelimit":{rl},"watch":{{"enabled":{we},"poll_ms":{wms},"files":[{wf}]}}}}"#,
        app = g.app_name,
        ver = g.app_version,
        mode = g.mode,
        mw = middleware_names.join(","),
        rl = rl_count,
        we = g.watcher.running,
        wms = g.watcher.poll_ms,
        wf = watch_files.join(","),
    );
    ok(&body)
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
            method: method.to_string(),
            path: path.to_string(),
            headers: vec![],
            body: body.to_string(),
        }
    }

    fn fake_req_with_header(
        method: &str,
        path: &str,
        body: &str,
        hdr_name: &str,
        hdr_val: &str,
    ) -> Request {
        Request {
            method: method.to_string(),
            path: path.to_string(),
            headers: vec![(hdr_name.to_string(), hdr_val.to_string())],
            body: body.to_string(),
        }
    }

    fn r(method: &str, path: &str, body: &str) -> String {
        route(&fake_req(method, path, body), &state(), "test-id")
    }

    fn rs(method: &str, path: &str, body: &str, s: &SharedState) -> String {
        route(&fake_req(method, path, body), s, "test-id")
    }

    // ── Health & version ──────────────────────────────────────────────────

    #[test]
    fn health_returns_ok() {
        let resp = r("GET", "/health", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("\"status\":\"ok\""), "got: {resp}");
    }

    #[test]
    fn health_includes_version() {
        assert!(r("GET", "/health", "").contains("version"));
    }

    #[test]
    fn api_v1_health() {
        let resp = r("GET", "/api/v1/health", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("\"status\":\"ok\""), "got: {resp}");
    }

    #[test]
    fn version_endpoint() {
        let resp = r("GET", "/api/v1/version", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("version"), "got: {resp}");
    }

    // ── Mode ──────────────────────────────────────────────────────────────

    #[test]
    fn mode_get_returns_mode() {
        assert!(r("GET", "/mode", "").contains("mode"));
    }

    #[test]
    fn mode_set_valid() {
        assert!(r("POST", "/mode", "lite").contains("200"));
    }

    #[test]
    fn mode_set_invalid() {
        assert!(r("POST", "/mode", "nope").contains("400"));
    }

    // ── Compile ───────────────────────────────────────────────────────────

    #[test]
    fn compile_returns_ts() {
        let resp = r("POST", "/compile", "service hello\nendpoint ping GET /ping");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("hello"), "got: {resp}");
    }

    #[test]
    fn compile_bad_source() {
        assert!(r("POST", "/compile", "bad source###").contains("400"));
    }

    #[test]
    fn services_before_compile() {
        assert!(r("GET", "/services", "").contains("400"));
    }

    // ── CORS ──────────────────────────────────────────────────────────────

    #[test]
    fn cors_preflight_responds_204() {
        let resp = cors_preflight();
        assert!(resp.contains("204"), "got: {resp}");
        assert!(resp.contains("Access-Control-Allow-Origin"), "got: {resp}");
    }

    // ── Metrics ───────────────────────────────────────────────────────────

    #[test]
    fn metrics_endpoint() {
        let s = state();
        s.lock().unwrap().push_trace("GET", "/ping", 200, 5);
        let resp = rs("GET", "/api/v1/metrics", "", &s);
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("total_requests"), "got: {resp}");
    }

    #[test]
    fn metrics_prometheus_endpoint() {
        let s = state();
        s.lock().unwrap().push_trace("GET", "/ping", 200, 5);
        let resp = rs("GET", "/api/v1/metrics/prometheus", "", &s);
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("bridge_requests_total"), "got: {resp}");
        assert!(resp.contains("text/plain"), "got: {resp}");
    }

    // ── Sampling ──────────────────────────────────────────────────────────

    #[test]
    fn sampling_set() {
        let resp = r("POST", "/api/v1/sampling", "0.5");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("0.5"), "got: {resp}");
    }

    #[test]
    fn sampling_invalid() {
        assert!(r("POST", "/api/v1/sampling", "2.0").contains("400"));
    }

    // ── Not found ─────────────────────────────────────────────────────────

    #[test]
    fn not_found_returns_404() {
        assert!(r("GET", "/nonexistent", "").contains("404"));
    }

    // ── OpenAPI ───────────────────────────────────────────────────────────

    #[test]
    fn openapi_before_compile() {
        assert!(r("GET", "/api/v1/openapi", "").contains("400"));
    }

    #[test]
    fn openapi_after_compile() {
        let s = state();
        rs(
            "POST",
            "/compile",
            "service api\nendpoint list GET /items",
            &s,
        );
        let resp = rs("GET", "/api/v1/openapi", "", &s);
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("openapi"), "got: {resp}");
    }

    // ── Request ID ────────────────────────────────────────────────────────

    #[test]
    fn response_contains_request_id() {
        let resp = route(&fake_req("GET", "/health", ""), &state(), "req-abc123");
        assert!(resp.contains("req-abc123"), "got: {resp}");
        assert!(resp.contains("X-Bridge-Request-Id"), "got: {resp}");
    }

    #[test]
    fn request_id_counter_increments() {
        let id1 = next_request_id();
        let id2 = next_request_id();
        assert_ne!(id1, id2, "each request should get a unique ID");
    }

    // ── Auth middleware ───────────────────────────────────────────────────

    #[test]
    fn auth_health_always_public() {
        // /health is never gated even when a token is configured
        assert!(!should_enforce_auth("/health"));
        assert!(!should_enforce_auth("/api/v1/health"));
    }

    #[test]
    fn auth_other_paths_enforced() {
        assert!(should_enforce_auth("/api/v1/metrics"));
        assert!(should_enforce_auth("/compile"));
        assert!(should_enforce_auth("/api/v1/traces"));
    }

    #[test]
    fn auth_valid_bearer_passes() {
        let req = fake_req_with_header(
            "GET",
            "/api/v1/metrics",
            "",
            "Authorization",
            "Bearer secret-token",
        );
        assert!(check_auth(&req, "secret-token").is_ok());
    }

    #[test]
    fn auth_invalid_bearer_fails() {
        let req = fake_req_with_header(
            "GET",
            "/api/v1/metrics",
            "",
            "Authorization",
            "Bearer wrong-token",
        );
        assert!(check_auth(&req, "secret-token").is_err());
    }

    #[test]
    fn auth_x_api_key_passes() {
        let req = fake_req_with_header("GET", "/api/v1/metrics", "", "x-api-key", "my-key");
        assert!(check_auth(&req, "my-key").is_ok());
    }

    #[test]
    fn auth_no_header_fails() {
        let req = fake_req("GET", "/api/v1/metrics", "");
        assert!(check_auth(&req, "secret-token").is_err());
    }

    // ── Auth set (JSON body) ──────────────────────────────────────────────

    #[test]
    fn auth_set_plain_token() {
        let s = state();
        let resp = rs("POST", "/api/v1/auth/set", "my-plain-token", &s);
        assert!(resp.contains("200"), "got: {resp}");
        assert!(s.lock().unwrap().auth_token == Some("my-plain-token".to_string()));
    }

    #[test]
    fn auth_set_json_body() {
        let s = state();
        let resp = rs(
            "POST",
            "/api/v1/auth/set",
            r#"{"scheme":"bearer","token":"json-token"}"#,
            &s,
        );
        assert!(resp.contains("200"), "got: {resp}");
        assert!(s.lock().unwrap().auth_token == Some("json-token".to_string()));
    }

    #[test]
    fn auth_set_then_enforce() {
        let s = state();
        // Set a token
        rs("POST", "/api/v1/auth/set", "gate-token", &s);
        // Request without auth → 401
        let bad = route(&fake_req("GET", "/api/v1/metrics", ""), &s, "r1");
        assert!(
            bad.contains("401"),
            "expected 401 without token, got: {bad}"
        );
        // Request with correct bearer → 200
        let good = route(
            &fake_req_with_header(
                "GET",
                "/api/v1/metrics",
                "",
                "Authorization",
                "Bearer gate-token",
            ),
            &s,
            "r2",
        );
        assert!(
            good.contains("200"),
            "expected 200 with valid token, got: {good}"
        );
    }

    // ── JSON field extractor ──────────────────────────────────────────────

    #[test]
    fn extract_json_field_string() {
        assert_eq!(
            extract_json_field(r#"{"token":"abc"}"#, "token"),
            Some("abc".to_string())
        );
    }

    #[test]
    fn extract_json_field_missing() {
        assert_eq!(extract_json_field(r#"{"other":"x"}"#, "token"), None);
    }

    // ── Middleware HTTP endpoints ─────────────────────────────────────────

    #[test]
    fn middleware_list_empty() {
        let resp = r("GET", "/api/v1/middleware", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("[]"), "got: {resp}");
    }

    #[test]
    fn middleware_register_and_list() {
        let s = state();
        let body = r#"{"name":"logger","scope":"global","before":"log"}"#;
        let reg = rs("POST", "/api/v1/middleware", body, &s);
        assert!(reg.contains("200"), "got: {reg}");
        assert!(reg.contains("logger"), "got: {reg}");

        let list = rs("GET", "/api/v1/middleware", "", &s);
        assert!(list.contains("logger"), "got: {list}");
        assert!(list.contains("global"), "got: {list}");
    }

    #[test]
    fn middleware_register_service_scope() {
        let s = state();
        let body = r#"{"name":"svc-mw","scope":"service:users","before":"log"}"#;
        let resp = rs("POST", "/api/v1/middleware", body, &s);
        assert!(resp.contains("200"), "got: {resp}");
        let list = rs("GET", "/api/v1/middleware", "", &s);
        assert!(list.contains("service:users"), "got: {list}");
    }

    #[test]
    fn middleware_register_endpoint_scope() {
        let s = state();
        let body = r#"{"name":"ep-mw","scope":"GET:/health","before":"log"}"#;
        let resp = rs("POST", "/api/v1/middleware", body, &s);
        assert!(resp.contains("200"), "got: {resp}");
        let list = rs("GET", "/api/v1/middleware", "", &s);
        assert!(list.contains("GET:/health"), "got: {list}");
    }

    #[test]
    fn middleware_remove() {
        let s = state();
        rs(
            "POST",
            "/api/v1/middleware",
            r#"{"name":"to-remove","scope":"global","before":"log"}"#,
            &s,
        );
        let del = rs(
            "DELETE",
            "/api/v1/middleware",
            r#"{"name":"to-remove"}"#,
            &s,
        );
        assert!(del.contains("200"), "got: {del}");
        assert!(del.contains("to-remove"), "got: {del}");
        let list = rs("GET", "/api/v1/middleware", "", &s);
        assert!(
            !list.contains("to-remove"),
            "still present after remove: {list}"
        );
    }

    #[test]
    fn middleware_remove_not_found() {
        let resp = r("DELETE", "/api/v1/middleware", r#"{"name":"nonexistent"}"#);
        assert!(resp.contains("404"), "got: {resp}");
    }

    #[test]
    fn middleware_register_replaces_existing() {
        let s = state();
        rs(
            "POST",
            "/api/v1/middleware",
            r#"{"name":"dup","scope":"global","before":"log"}"#,
            &s,
        );
        rs(
            "POST",
            "/api/v1/middleware",
            r#"{"name":"dup","scope":"service:api","before":"log"}"#,
            &s,
        );
        // Should still only have one entry named "dup"
        let list = rs("GET", "/api/v1/middleware", "", &s);
        assert_eq!(
            list.matches("\"name\":\"dup\"").count(),
            1,
            "expected exactly one entry, got: {list}"
        );
    }

    #[test]
    fn middleware_reject_hook_short_circuits() {
        let s = state();
        let body = r#"{"name":"blocker","scope":"global","before":"reject:403:forbidden"}"#;
        rs("POST", "/api/v1/middleware", body, &s);
        // /api/v1/metrics is protected by our new middleware
        let resp = route(&fake_req("GET", "/api/v1/metrics", ""), &s, "r1");
        assert!(resp.contains("403"), "expected 403, got: {resp}");
    }

    #[test]
    fn middleware_after_header_hook_injects_header() {
        let s = state();
        let body = r#"{"name":"tagger","scope":"global","after":"header:X-Test:hello"}"#;
        rs("POST", "/api/v1/middleware", body, &s);
        let resp = route(&fake_req("GET", "/health", ""), &s, "r1");
        assert!(
            resp.contains("X-Test"),
            "expected X-Test header, got: {resp}"
        );
        assert!(resp.contains("hello"), "got: {resp}");
    }

    #[test]
    fn middleware_register_missing_name() {
        let resp = r("POST", "/api/v1/middleware", r#"{"scope":"global"}"#);
        assert!(resp.contains("400"), "got: {resp}");
    }

    #[test]
    fn middleware_register_invalid_scope() {
        let resp = r(
            "POST",
            "/api/v1/middleware",
            r#"{"name":"x","scope":"bad-scope"}"#,
        );
        assert!(resp.contains("400"), "got: {resp}");
    }

    #[test]
    fn parse_scope_global() {
        assert_eq!(parse_scope("global").unwrap(), Scope::Global);
    }

    #[test]
    fn parse_scope_service() {
        assert_eq!(
            parse_scope("service:users").unwrap(),
            Scope::Service("users".into())
        );
    }

    #[test]
    fn parse_scope_endpoint() {
        let s = parse_scope("DELETE:/items/1").unwrap();
        assert_eq!(
            s,
            Scope::Endpoint {
                method: "DELETE".into(),
                path: "/items/1".into()
            }
        );
    }

    // ── Watch / hot-reload HTTP endpoints ─────────────────────────────────

    #[test]
    fn watch_status_empty() {
        let resp = r("GET", "/api/v1/watch", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("watching"), "got: {resp}");
        assert!(resp.contains("poll_ms"), "got: {resp}");
    }

    #[test]
    fn watch_add_file() {
        let s = state();
        let resp = rs("POST", "/api/v1/watch/files", "/app/svc.bridge", &s);
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("svc.bridge"), "got: {resp}");
        let status = rs("GET", "/api/v1/watch", "", &s);
        assert!(
            status.contains("svc.bridge"),
            "file not in status: {status}"
        );
    }

    #[test]
    fn watch_add_non_bridge_file_rejected() {
        let resp = r("POST", "/api/v1/watch/files", "/app/not-a.ts");
        assert!(resp.contains("400"), "got: {resp}");
    }

    #[test]
    fn watch_remove_file() {
        let s = state();
        rs("POST", "/api/v1/watch/files", "/app/a.bridge", &s);
        let del = rs("DELETE", "/api/v1/watch/files", "/app/a.bridge", &s);
        assert!(del.contains("200"), "got: {del}");
        let status = rs("GET", "/api/v1/watch", "", &s);
        assert!(
            !status.contains("/app/a.bridge"),
            "should be removed: {status}"
        );
    }

    #[test]
    fn watch_remove_nonexistent_file_404() {
        let resp = r("DELETE", "/api/v1/watch/files", "/no/such/file.bridge");
        assert!(resp.contains("404"), "got: {resp}");
    }

    #[test]
    fn watch_add_dir_bad_path_returns_ok_with_zero_files() {
        // Non-existent dir doesn't error — just finds no files
        let resp = r("POST", "/api/v1/watch/dirs", "/no/such/directory");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("new_files"), "got: {resp}");
    }

    // ── Rate limiting HTTP endpoints ──────────────────────────────────────

    #[test]
    fn ratelimit_list_empty() {
        let resp = r("GET", "/api/v1/ratelimit", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("[]"), "got: {resp}");
    }

    #[test]
    fn ratelimit_add_rule() {
        let s = state();
        let body = r#"{"method":"GET","path":"/items","capacity":10,"refill_rate":1}"#;
        let resp = rs("POST", "/api/v1/ratelimit", body, &s);
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("capacity"), "got: {resp}");
        let list = rs("GET", "/api/v1/ratelimit", "", &s);
        assert!(list.contains("/items"), "got: {list}");
    }

    #[test]
    fn ratelimit_add_missing_capacity() {
        let resp = r(
            "POST",
            "/api/v1/ratelimit",
            r#"{"method":"GET","path":"/x","refill_rate":1}"#,
        );
        assert!(resp.contains("400"), "got: {resp}");
    }

    #[test]
    fn ratelimit_add_missing_refill_rate() {
        let resp = r(
            "POST",
            "/api/v1/ratelimit",
            r#"{"method":"GET","path":"/x","capacity":5}"#,
        );
        assert!(resp.contains("400"), "got: {resp}");
    }

    #[test]
    fn ratelimit_remove_rule() {
        let s = state();
        rs(
            "POST",
            "/api/v1/ratelimit",
            r#"{"method":"POST","path":"/submit","capacity":5,"refill_rate":1}"#,
            &s,
        );
        let del = rs(
            "DELETE",
            "/api/v1/ratelimit",
            r#"{"method":"POST","path":"/submit"}"#,
            &s,
        );
        assert!(del.contains("200"), "got: {del}");
        let list = rs("GET", "/api/v1/ratelimit", "", &s);
        assert!(!list.contains("/submit"), "rule should be removed: {list}");
    }

    #[test]
    fn ratelimit_remove_not_found() {
        let resp = r(
            "DELETE",
            "/api/v1/ratelimit",
            r#"{"method":"GET","path":"/nonexistent"}"#,
        );
        assert!(resp.contains("404"), "got: {resp}");
    }

    #[test]
    fn ratelimit_enforced_returns_429() {
        let s = state();
        // Add rule with capacity 1
        rs(
            "POST",
            "/api/v1/ratelimit",
            r#"{"method":"GET","path":"/health","capacity":1,"refill_rate":0.1}"#,
            &s,
        );
        // First request should pass
        let r1 = route(&fake_req("GET", "/health", ""), &s, "r1");
        assert!(r1.contains("200"), "first request should pass, got: {r1}");
        // Second request should be rate-limited
        let r2 = route(&fake_req("GET", "/health", ""), &s, "r2");
        assert!(
            r2.contains("429"),
            "second request should be 429, got: {r2}"
        );
        assert!(r2.contains("rate limit exceeded"), "got: {r2}");
    }

    #[test]
    fn ratelimit_headers_on_passing_request() {
        let s = state();
        rs(
            "POST",
            "/api/v1/ratelimit",
            r#"{"method":"GET","path":"/api/v1/version","capacity":100,"refill_rate":10}"#,
            &s,
        );
        let resp = route(&fake_req("GET", "/api/v1/version", ""), &s, "r1");
        assert!(
            resp.contains("X-RateLimit-Limit"),
            "missing Limit header, got: {resp}"
        );
        assert!(
            resp.contains("X-RateLimit-Remaining"),
            "missing Remaining header, got: {resp}"
        );
        assert!(
            resp.contains("X-RateLimit-Reset"),
            "missing Reset header, got: {resp}"
        );
    }

    #[test]
    fn ratelimit_retry_after_on_429() {
        let s = state();
        rs(
            "POST",
            "/api/v1/ratelimit",
            r#"{"method":"POST","path":"/api/v1/compile","capacity":1,"refill_rate":0.01}"#,
            &s,
        );
        route(&fake_req("POST", "/api/v1/compile", "x"), &s, "r1"); // consume token
        let resp = route(&fake_req("POST", "/api/v1/compile", "x"), &s, "r2");
        assert!(resp.contains("429"), "expected 429, got: {resp}");
        assert!(
            resp.contains("Retry-After"),
            "missing Retry-After header, got: {resp}"
        );
    }
}

// ── Streaming Endpoints ────────────────────────────────────────────────────

fn stream_traces(state: &SharedState, _req: &Request) -> String {
    // One-shot SSE burst of recent traces; long-lived streaming is served
    // inline by handle_state_stream (chunked, per-connection).
    let g = state.lock().unwrap();
    if g.traces.is_empty() {
        return streaming::SseFrame::new("traces", r#"{"status":"stream-active","traces":0}"#)
            .encode();
    }
    streaming::render_traces(&g, 50)
}

fn stream_metrics(state: &SharedState, _req: &Request) -> String {
    let g = state.lock().unwrap();
    streaming::render_metrics(&g)
}

fn stream_services(state: &SharedState, _req: &Request) -> String {
    let g = state.lock().unwrap();
    streaming::render_services(&g)
}
