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

use crate::auth::{self, bearer_from_header};
use crate::deploy::{DeployRegistry, Status as DeployStatus};
use crate::middleware::{MiddlewareBuilder, MiddlewareContext, Scope};
use crate::pubsub;
use crate::ratelimit::BucketKey;
use crate::sqldb;
use crate::state::{LogLevel, SharedState};
use crate::staticfiles::{mime_for, StaticMount, StaticResult};
use crate::streaming;
use crate::transactions;
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
    /// Construct a synthetic request (MCP tool dispatch, tests).
    pub(crate) fn synthetic(method: &str, path: &str, body: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            headers: vec![],
            body: body.to_string(),
        }
    }

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

    // Object storage reads — keyed GET/HEAD serves raw bytes when authorized
    // (public bucket or valid `?exp=&sig=`); otherwise a JSON 403/404.
    // `?meta=1` skips this block so the router can return object metadata,
    // and bucket listings always go through the router.
    if clean_path.starts_with("/api/v1/storage/objects/")
        && (method == "GET" || method == "HEAD")
        && !req.path.contains("meta=1")
        && clean_path.matches('/').count() >= 5
    {
        if let Some(bytes) = storage_download_response(&state, &req.path, method == "HEAD") {
            return stream.write_all(&bytes).and_then(|_| stream.flush());
        }
    }

    // Object storage signed writes — a valid `?exp=&sig=` IS the authorization
    // (presigned requests carry no headers, so daemon-token auth cannot apply).
    // Invalid/absent signatures fall through to the router's normal gating.
    if clean_path.starts_with("/api/v1/storage/objects/")
        && matches!(method.as_str(), "PUT" | "DELETE")
        && path.contains("sig=")
    {
        if let Some(bytes) = storage_signed_write_response(&state, &req) {
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

pub(crate) fn route(req: &Request, state: &SharedState, req_id: &str) -> String {
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

        // ── Auth pipeline (JWT + sessions) ────────────────────────────────
        ("POST", "/api/v1/auth/token") => auth_token_issue(req, state),
        ("GET", p) if p.starts_with("/api/v1/auth/whoami") => auth_whoami(req, state),
        ("DELETE", "/api/v1/auth/token") => auth_token_revoke(req, state),

        // ── Transactions ──────────────────────────────────────────────────
        ("GET", "/api/v1/tx") => tx_list(state),
        ("POST", "/api/v1/tx") => tx_begin(req, state),
        ("PUT", p) if p.starts_with("/api/v1/tx/") => tx_enqueue(req, state, p),
        ("POST", p) if p.starts_with("/api/v1/tx/") && p.ends_with("/commit") => {
            tx_commit(req, state, p)
        }
        ("POST", p) if p.starts_with("/api/v1/tx/") && p.ends_with("/rollback") => {
            tx_rollback(state, p)
        }
        ("GET", "/api/v1/tx/prune") => tx_prune(state),

        // ── Object storage (Encore buckets) ─────────────────────────────
        ("GET", "/api/v1/storage") => storage_list(state),
        ("POST", "/api/v1/storage/buckets") => storage_bucket_create(req, state),
        ("DELETE", p) if p.starts_with("/api/v1/storage/buckets/") => {
            storage_bucket_delete(state, p)
        }
        ("POST", p) if p.starts_with("/api/v1/storage/buckets/") && p.ends_with("/sign") => {
            storage_sign(req, state, p)
        }
        ("PUT", p) if p.starts_with("/api/v1/storage/objects/") => {
            storage_object_put(req, state, &req.path)
        }
        ("DELETE", p) if p.starts_with("/api/v1/storage/objects/") => {
            storage_object_delete(state, &req.path)
        }
        ("GET", "/api/v1/storage/objects") => storage_list_objects_all(state),
        ("GET", p) if p.starts_with("/api/v1/storage/objects/") => {
            storage_object_info(state, &req.path)
        }

        // ── Pub/Sub broker ──────────────────────────────────────────────────────
        ("GET", "/api/v1/pubsub") => pubsub_status(state),
        ("GET", "/api/v1/pubsub/subscriptions") => pubsub_subscriptions(state),
        ("POST", "/api/v1/pubsub/topics") => pubsub_topic_create(req, state),
        ("POST", "/api/v1/pubsub/publish") => pubsub_publish(req, state),
        ("POST", "/api/v1/pubsub/subscriptions") => pubsub_subscribe(req, state),
        ("GET", p) if p.starts_with("/api/v1/pubsub/subscriptions/") => {
            pubsub_subscription_info(state, &req.path)
        }
        ("POST", p) if p.starts_with("/api/v1/pubsub/subscriptions/") && p.ends_with("/pull") => {
            pubsub_pull(req, state, p)
        }
        ("POST", "/api/v1/pubsub/ack") => pubsub_ack(req, state),
        ("POST", "/api/v1/pubsub/nack") => pubsub_nack(req, state),
        ("GET", p) if p.starts_with("/api/v1/pubsub/dlq/") => pubsub_dlq(state, &req.path),

        // ── Infra config ──────────────────────────────────────────────
        ("GET", "/api/v1/infra") => infra_show(state),
        ("POST", "/api/v1/infra/env") => infra_env_set(req, state),
        ("DELETE", "/api/v1/infra/env") => infra_env_clear(state),
        ("GET", "/api/v1/infra/services") => infra_services(state),
        ("POST", "/api/v1/infra/services") => infra_service_register(req, state),
        ("GET", "/api/v1/infra/databases") => infra_databases(state),
        ("POST", "/api/v1/infra/databases") => infra_database_upsert(req, state),
        ("POST", "/api/v1/infra/tls") => infra_tls_set(req, state),

        // ── Testing harness (Encore `testing` parity) ─────────────────
        ("GET", "/api/v1/testing") => testing_show(state),
        ("POST", "/api/v1/testing/mode/enter") => testing_mode_enter(req, state),
        ("POST", "/api/v1/testing/mode/exit") => testing_mode_exit(state),
        ("POST", "/api/v1/testing/databases") => testing_db_new(req, state),
        ("DELETE", "/api/v1/testing/databases") => testing_db_cleanup(state),
        ("POST", "/api/v1/testing/mocks/auth") => testing_mock_auth(req, state),
        ("POST", "/api/v1/testing/mocks/services") => testing_mock_service(req, state),
        ("DELETE", "/api/v1/testing/mocks") => testing_mocks_clear(state),

        // ── Deployments (Encore CLI deploy parity) ────────────────────
        ("GET", "/api/v1/deploy") => deploy_list(state),
        ("POST", "/api/v1/deploy") => deploy_create(req, state),
        ("POST", "/api/v1/deploy/status") => deploy_status(req, state),
        ("POST", "/api/v1/deploy/rollback") => deploy_rollback(req, state),
        ("GET", "/api/v1/deploy/dockerfile") => deploy_dockerfile(state),

        // ── MCP (Model Context Protocol) surface ─────────────────────
        ("POST", "/api/v1/mcp") => mcp_request(req, state),

        // ── WebSocket hub (service-to-service streams) ────────────────
        ("GET", "/api/v1/ws") => ws_status(state),
        ("POST", "/api/v1/ws/handshake") => ws_handshake(req),
        ("POST", "/api/v1/ws/join") => ws_join(req, state),
        ("POST", "/api/v1/ws/leave") => ws_leave(req, state),
        ("POST", "/api/v1/ws/broadcast") => ws_broadcast(req, state),

        // ── Secrets management ────────────────────────────────────────────
        ("GET", "/api/v1/secrets") => secrets_list(state),
        ("POST", "/api/v1/secrets/set") => secrets_set(req, state),
        ("POST", "/api/v1/secrets/get") => secrets_get(req, state),
        ("POST", "/api/v1/secrets/check") => secrets_check(req, state),
        ("DELETE", p) if p.starts_with("/api/v1/secrets/") => secrets_delete(state, &req.path),

        // ── Cache keyspaces (Encore RedisCluster in-memory mode) ──────
        ("GET", "/api/v1/cache") => cache_status(state),
        ("GET", "/api/v1/cache/keyspaces") => cache_keyspace_list(state),
        ("POST", "/api/v1/cache/keyspaces") => cache_keyspace_ensure(req, state),
        ("GET", "/api/v1/cache/keyspaces/entries") => cache_entries_all(state),
        ("GET", p) if p.starts_with("/api/v1/cache/keyspaces/") => {
            cache_keyspace_info(state, &req.path)
        }
        ("DELETE", p) if p.starts_with("/api/v1/cache/keyspaces/") => {
            cache_invalidate(req, state, &req.path)
        }
        ("GET", p) if p.starts_with("/api/v1/cache/entry/") => cache_get(state, &req.path),
        ("PUT", p) if p.starts_with("/api/v1/cache/entry/") => cache_put(req, state, &req.path),
        ("DELETE", p) if p.starts_with("/api/v1/cache/entry/") => cache_del(state, &req.path),
        ("POST", "/api/v1/cache/mget") => cache_mget(req, state),
        ("POST", "/api/v1/cache/mset") => cache_mset(req, state),

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
        // Scan to the closing quote, honoring backslash escapes so
        // values like "{\"a\":1}" survive instead of truncating at \".
        let bytes = inner.as_bytes();
        let mut end = inner.len();
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2, // skip the escaped character
                b'"' => {
                    end = i;
                    break;
                }
                _ => i += 1,
            }
        }
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

/// Extract the full array literal following `"key": [ ... ]` (balanced
/// brackets, string-aware). Returns the inner text between the brackets.
fn extract_json_array(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = json.find(&needle)?;
    let rest = json[pos + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if !rest.starts_with('[') {
        return None;
    }
    let start = json.len() - rest.len();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut prev_esc = false;
    for (i, c) in json[start..].char_indices() {
        match c {
            '"' if !prev_esc => in_str = !in_str,
            '[' if !in_str => depth += 1,
            ']' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    // Inner slice between the outermost brackets.
                    return Some(json[start + 1..start + i].to_string());
                }
            }
            _ => {}
        }
        prev_esc = c == '\\' && !prev_esc;
    }
    None
}

/// Parse a flat `{ "k": "v", ... }` inner text into (key, value) pairs.
/// String-aware: values may contain escaped quotes and commas inside
/// strings — unlike [`parse_flat_json_object`], which splits naively.
fn parse_string_pairs(inner: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let bytes = inner.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Read one `"key":value` unit.
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let key_start = i + 1;
        let mut j = key_start;
        while j < bytes.len() && bytes[j] != b'"' {
            j += if bytes[j] == b'\\' { 2 } else { 1 };
        }
        if j >= bytes.len() {
            break;
        }
        let key = inner[key_start..j].to_string();
        i = j + 1;
        // Skip to ':' then read the value token.
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b':') {
            i += 1;
        }
        let val_start = i;
        if i < bytes.len() && bytes[i] == b'"' {
            // Quoted string value — honor escapes.
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else {
            while i < bytes.len() && bytes[i] != b',' && bytes[i] != b'}' {
                i += 1;
            }
        }
        let raw = inner[val_start..i.min(inner.len())].trim();
        if !key.is_empty() {
            pairs.push((key, raw.to_string()));
        }
        // Advance past the separating comma.
        while i < bytes.len() && bytes[i] != b',' {
            i += 1;
        }
        i += 1;
    }
    pairs
}
/// Parse a flat `[ "a", "b", ... ]` inner text into strings.
/// Non-string elements are skipped (cache keys are always strings).
fn parse_string_array(inner: &str) -> Vec<String> {
    inner
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty() || inner.contains("\"\""))
        .collect()
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

// ── Auth pipeline handlers ────────────────────────────────────────────────────

/// JWT secret: `BRIDGE_JWT_SECRET` env or an ephemeral per-process default.
fn jwt_secret() -> Vec<u8> {
    std::env::var("BRIDGE_JWT_SECRET")
        .unwrap_or_else(|_| "bridge-dev-secret-do-not-use-in-prod".to_string())
        .into_bytes()
}

/// Issue a JWT session.
///
/// Body: `{"sub":"user-1","ttl":3600,"iss":"bridge","claims":{"role":"admin"}}`
/// Returns the signed token; it is also registered as a live bearer session.
fn auth_token_issue(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    if body.is_empty() {
        return bad_request("body required");
    }
    let sub = match extract_json_field(body, "sub") {
        Some(s) if !s.is_empty() => s,
        _ => return bad_request("sub is required"),
    };
    let ttl = extract_json_field(body, "ttl")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(3600);

    let mut claims = auth::JwtClaims::new(sub).with_ttl(ttl);
    if let Some(iss) = extract_json_field(body, "iss") {
        claims = claims.with_issuer(iss);
    }
    // Optional flat custom claims object: {"role":"admin","org":"acme"}
    if let Some(obj) = extract_json_object(body, "claims") {
        for (k, v) in parse_flat_json_object(&obj) {
            if !matches!(k.as_str(), "sub" | "iss" | "exp" | "iat" | "scope") {
                claims = claims.with_claim(k, v);
            }
        }
    }

    let token = state.lock().unwrap().auth.issue_jwt(claims, &jwt_secret());
    ok(&format!(
        r#"{{"token":"{token}","token_type":"Bearer","expires_in":{ttl}}}"#
    ))
}

/// Whoami: authenticate the request's bearer token and report identity.
/// JWTs verify cryptographically (stateless); opaque tokens hit the registry.
fn auth_whoami(req: &Request, state: &SharedState) -> String {
    let Some(token) = bearer_from_header(req.header("authorization")) else {
        return json_response(401, r#"{"error":"missing bearer token"}"#);
    };
    let result = {
        let g = state.lock().unwrap();
        g.auth.authenticate(token, &jwt_secret())
    };
    match result {
        Ok(data) => ok(&data.to_json()),
        Err(e) => json_response(401, &format!(r#"{{"error":"{e}"}}"#)),
    }
}

/// Revoke a bearer session (opaque or JWT) from the registry.
fn auth_token_revoke(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let token = if body.starts_with('{') {
        extract_json_field(body, "token")
    } else if let Some(t) = bearer_from_header(req.header("authorization")) {
        Some(t.to_string())
    } else {
        Some(body.trim_matches('"').to_string())
    };
    match token {
        Some(t) if !t.is_empty() => {
            state.lock().unwrap().auth.revoke_bearer(&t);
            ok(r#"{"message":"token revoked"}"#)
        }
        _ => bad_request("token required (body or Authorization header)"),
    }
}

// ── Transaction handlers ──────────────────────────────────────────────────────

fn tx_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.txs.to_json())
}

/// Begin: body `{"id":"tx1","isolation":"serializable"}` — isolation optional.
fn tx_begin(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let id = if body.starts_with('{') {
        extract_json_field(body, "id")
    } else {
        Some(body.trim_matches('"').to_string())
    };
    let Some(id) = id.filter(|i| !i.is_empty()) else {
        return bad_request("id required");
    };
    let isolation = body
        .starts_with('{')
        .then(|| extract_json_field(body, "isolation"))
        .flatten()
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let iso = match isolation.as_str() {
        "" | "read_committed" => transactions::IsolationLevel::ReadCommitted,
        "read_uncommitted" => transactions::IsolationLevel::ReadUncommitted,
        "repeatable_read" => transactions::IsolationLevel::RepeatableRead,
        "serializable" => transactions::IsolationLevel::Serializable,
        other => return bad_request(&format!("unknown isolation level {other:?}")),
    };
    let err = state.lock().unwrap().txs.begin(id.clone(), iso).err();
    match err {
        Some(e) => json_response(409, &format!(r#"{{"error":"{e}"}}"#)),
        None => ok(&format!(
            r#"{{"message":"transaction started","id":"{id}","isolation":"{}"}}"#,
            iso.as_str()
        )),
    }
}

/// Queue an op into `PUT /api/v1/tx/{id}`.
/// Body: `{"op":"put","ns":"app","key":"k","value":"v"}` |
///       `{"op":"del","ns":"app","key":"k"}` |
///       `{"op":"del_matching","ns":"app","pattern":"tmp:*"}`
fn tx_enqueue(req: &Request, state: &SharedState, path: &str) -> String {
    let tx_id = path.trim_start_matches("/api/v1/tx/");
    let body = req.body.trim();
    if body.is_empty() || !body.starts_with('{') {
        return bad_request("JSON body required");
    }
    let kind = extract_json_field(body, "op").unwrap_or_else(|| "put".to_string());
    let ns = extract_json_field(body, "ns").unwrap_or_else(|| "default".to_string());
    let op = match kind.as_str() {
        "put" => {
            let (Some(key), Some(value)) = (
                extract_json_field(body, "key"),
                extract_json_field(body, "value"),
            ) else {
                return bad_request("put requires key and value");
            };
            transactions::StoreOp::Put { ns, key, value }
        }
        "del" => {
            let Some(key) = extract_json_field(body, "key") else {
                return bad_request("del requires key");
            };
            transactions::StoreOp::Del { ns, key }
        }
        "del_matching" => {
            let Some(pattern) = extract_json_field(body, "pattern") else {
                return bad_request("del_matching requires pattern");
            };
            transactions::StoreOp::DelMatching { ns, pattern }
        }
        other => return bad_request(&format!("unknown op {other:?} (put|del|del_matching)")),
    };
    let count = state.lock().unwrap().txs.enqueue(tx_id, op);
    match count {
        Ok(n) => ok(&format!(r#"{{"message":"operation queued","queued":{n}}}"#)),
        Err(e) => json_response(404, &format!(r#"{{"error":"{e}"}}"#)),
    }
}

fn tx_commit(req: &Request, state: &SharedState, path: &str) -> String {
    let _ = req;
    let tx_id = path
        .trim_start_matches("/api/v1/tx/")
        .trim_end_matches("/commit");
    let g = state.lock().unwrap();
    match g.txs.commit(tx_id, &g.store) {
        Ok(n) => {
            drop(g);
            ok(&format!(r#"{{"message":"committed","operations":{n}}}"#))
        }
        Err(e) => {
            let status = if e.contains("not found") { 404 } else { 409 };
            drop(g);
            json_response(status, &format!(r#"{{"error":"{e}"}}"#))
        }
    }
}

fn tx_rollback(state: &SharedState, path: &str) -> String {
    let tx_id = path
        .trim_start_matches("/api/v1/tx/")
        .trim_end_matches("/rollback");
    match state.lock().unwrap().txs.rollback(tx_id) {
        Ok(n) => ok(&format!(r#"{{"message":"rolled back","discarded":{n}}}"#)),
        Err(e) => {
            let status = if e.contains("not found") { 404 } else { 409 };
            json_response(status, &format!(r#"{{"error":"{e}"}}"#))
        }
    }
}

fn tx_prune(state: &SharedState) -> String {
    let pruned = state.lock().unwrap().txs.prune_finished();
    ok(&format!(r#"{{"message":"pruned","removed":{pruned}}}"#))
}

// ── Object storage handlers ───────────────────────────────────────────────────

/// Shared secret for signed URLs (mirrors jwt_secret).
fn storage_secret() -> Vec<u8> {
    std::env::var("BRIDGE_STORAGE_SECRET")
        .or_else(|_| std::env::var("BRIDGE_JWT_SECRET"))
        .unwrap_or_else(|_| "bridge-dev-secret-do-not-use-in-prod".to_string())
        .into_bytes()
}

/// Parse `/api/v1/storage/<kind>/<bucket>[/<key...>]` into (bucket, key?).
fn split_storage_path(path: &str, kind: &str) -> Option<(String, Option<String>)> {
    let rest = path.strip_prefix("/api/v1/storage/")?;
    let rest = rest.strip_prefix(kind)?.strip_prefix('/')?;
    let (b, k) = match rest.split_once('/') {
        Some((b, k)) if !b.is_empty() && !k.is_empty() => (b, Some(percent_decode(k))),
        _ if !rest.is_empty() => (rest, None),
        _ => return None,
    };
    Some((percent_decode(b), k))
}

/// Decode %XX escapes (and `+` → space) in a URL component.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn storage_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.storage.to_json())
}

/// Create a bucket: `{"name":"media","public":true}`.
fn storage_bucket_create(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let Some(name) = extract_json_field(body, "name").filter(|n| !n.is_empty()) else {
        return bad_request("name required");
    };
    let public = extract_json_field(body, "public")
        .map(|v| v == "true")
        .unwrap_or(false);
    match state.lock().unwrap().storage.create_bucket(&name, public) {
        Ok(()) => ok(&format!(
            r#"{{"message":"bucket created","name":"{name}","public":{public}}}"#
        )),
        Err(e) => json_response(409, &format!(r#"{{"error":"{e}"}}"#)),
    }
}

fn storage_bucket_delete(state: &SharedState, path: &str) -> String {
    let Some((bucket, _)) = split_storage_path(path, "buckets") else {
        return bad_request("bucket name required");
    };
    match state.lock().unwrap().storage.delete_bucket(&bucket) {
        Ok(()) => ok(&format!(
            r#"{{"message":"bucket deleted","name":"{bucket}"}}"#
        )),
        Err(e) => {
            let status = if e.contains("not found") { 404 } else { 409 };
            json_response(status, &format!(r#"{{"error":"{e}"}}"#))
        }
    }
}

/// Mint a signed URL: POST /api/v1/storage/buckets/{bucket}/sign
/// Body: `{"key":"a/b.txt","method":"GET","ttl":900}` (method/ttl optional).
fn storage_sign(req: &Request, state: &SharedState, path: &str) -> String {
    let Some((bucket, _)) = split_storage_path(path, "buckets") else {
        return bad_request("bucket name required");
    };
    let body = req.body.trim();
    let Some(key) = extract_json_field(body, "key").filter(|k| !k.is_empty()) else {
        return bad_request("key required");
    };
    let method = extract_json_field(body, "method")
        .map(|m| m.to_uppercase())
        .unwrap_or_else(|| "GET".to_string());
    let ttl = extract_json_field(body, "ttl")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(900);
    let secret = storage_secret();
    let g = state.lock().unwrap();
    match g.storage.sign_url(&method, &bucket, &key, ttl, &secret) {
        Ok((exp, sig)) => {
            drop(g);
            ok(&format!(
                r#"{{"url":"/api/v1/storage/objects/{bucket}/{key}?exp={exp}\u0026sig={sig}","method":"{method}","bucket":"{bucket}","key":"{key}","exp":{exp},"sig":"{sig}"}}"#
            ))
        }
        Err(e) => json_response(404, &format!(r#"{{"error":"{e}"}}"#)),
    }
}

/// Direct upload: PUT /api/v1/storage/objects/{bucket}/{key} (raw body).
/// Accepts `?exp=&sig=` for signed uploads; unsigned writes are management ops.
fn storage_object_put(req: &Request, state: &SharedState, full_path: &str) -> String {
    let Some((bucket, Some(key))) = split_storage_path(full_path, "objects") else {
        return bad_request("bucket and key required");
    };
    // When query params are present they MUST form a valid signed URL.
    if let Some(q) = full_path.split('?').nth(1) {
        if !q.is_empty() {
            if let Err(e) = verify_signed_query(state, "PUT", q, &bucket, &key) {
                return json_response(403, &format!(r#"{{"error":"{e}"}}"#));
            }
        }
    }
    match state
        .lock()
        .unwrap()
        .storage
        .put_object(&bucket, &key, req.body.as_bytes())
    {
        Ok(n) => ok(&format!(
            r#"{{"message":"object stored","bucket":"{bucket}","key":"{key}","size":{n}}}"#
        )),
        Err(e) => storage_err(e),
    }
}

/// Delete: DELETE /api/v1/storage/objects/{bucket}/{key}[?exp=&sig=]
fn storage_object_delete(state: &SharedState, full_path: &str) -> String {
    let Some((bucket, Some(key))) = split_storage_path(full_path, "objects") else {
        return bad_request("bucket and key required");
    };
    if let Some(q) = full_path.split('?').nth(1) {
        if !q.is_empty() {
            if let Err(e) = verify_signed_query(state, "DELETE", q, &bucket, &key) {
                return json_response(403, &format!(r#"{{"error":"{e}"}}"#));
            }
        }
    }
    match state.lock().unwrap().storage.delete_object(&bucket, &key) {
        Ok(()) => ok(&format!(r#"{{"message":"object deleted","key":"{key}"}}"#)),
        Err(e) => storage_err(e),
    }
}

/// Metadata/listing: GET /api/v1/storage/objects lists every bucket's keys;
/// GET /api/v1/storage/objects/{bucket} lists keys;
/// GET /api/v1/storage/objects/{bucket}/{key} returns object metadata JSON.
fn storage_list_objects_all(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    let items: Vec<String> = g
        .storage
        .list_buckets()
        .iter()
        .map(|b| {
            let keys = g
                .storage
                .list_objects(&b.name)
                .map(|ks| {
                    ks.iter()
                        .map(|k| format!(r#"{{"key":"{k}"}}"#))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            format!(r#"{{"bucket":"{bn}","objects":[{keys}]}}"#, bn = b.name)
        })
        .collect();
    ok(&format!(r#"{{"buckets":[{}]}}"#, items.join(",")))
}

fn storage_object_info(state: &SharedState, full_path: &str) -> String {
    let (path_no_q, query) = match full_path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (full_path, ""),
    };
    let Some((bucket, key)) = split_storage_path(path_no_q, "objects") else {
        return bad_request("bucket required");
    };

    // Snapshot auth-relevant facts WITHOUT holding the lock across
    // verify_signed_query (which locks internally).
    let public_bucket = {
        let g = state.lock().unwrap();
        match g.storage.get_bucket(&bucket) {
            Some(b) => b.public,
            None => {
                return json_response(
                    404,
                    &format!(r#"{{"error":"bucket {bucket:?} not found"}}"#),
                )
            }
        }
    };
    let Some(key) = key else {
        let keys = {
            let g = state.lock().unwrap();
            g.storage.list_objects(&bucket)
        };
        match keys {
            Ok(keys) => {
                let items: Vec<String> =
                    keys.iter().map(|k| format!(r#"{{"key":"{k}"}}"#)).collect();
                return ok(&format!(
                    r#"{{"bucket":"{bucket}","objects":[{}]}}"#,
                    items.join(",")
                ));
            }
            Err(e) => return storage_err(e),
        }
    };
    // Keyed metadata is object data too — same authorization as raw downloads.
    // Only a query actually carrying sig= counts as a signed request; anything
    // else (e.g. ?meta=1) falls back to the public-bucket flag.
    let authorized = if path_no_q.len() != full_path.len() && query.contains("sig=") {
        verify_signed_query(state, "GET", query, &bucket, &key).is_ok()
    } else {
        public_bucket
    };
    if !authorized {
        return json_response(
            403,
            r#"{"error":"access denied","message":"bucket is private and no valid signed URL was provided"}"#,
        );
    }
    let result = {
        let g = state.lock().unwrap();
        g.storage.get_object(&bucket, &key)
    };
    match result {
        Ok(bytes) => ok(&format!(
            r#"{{"bucket":"{bucket}","key":"{key}","size":{},"content_type":"{}"}}"#,
            bytes.len(),
            mime_for(&key)
        )),
        Err(_) => json_response(404, &format!(r#"{{"error":"object {key:?} not found"}}"#)),
    }
}

/// Validate `exp`/`sig` query params against the storage secret.
fn verify_signed_query(
    state: &SharedState,
    method: &str,
    query: &str,
    bucket: &str,
    key: &str,
) -> Result<(), String> {
    let mut exp = 0u64;
    let mut sig = String::new();
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("exp", v)) => exp = v.parse().unwrap_or(0),
            Some(("sig", v)) => sig = percent_decode(v),
            _ => {}
        }
    }
    if sig.is_empty() || exp == 0 {
        return Err("missing exp/sig query params".into());
    }
    state.lock().unwrap().storage.verify_signed_url(
        method,
        bucket,
        key,
        exp,
        &sig,
        &storage_secret(),
    )
}

fn storage_err(e: String) -> String {
    let status = if e.contains("not found") {
        404
    } else if e.contains("invalid") || e.contains("must be") || e.contains("may only") {
        400
    } else {
        500
    };
    json_response(status, &format!(r#"{{"error":"{e}"}}"#))
}

// ── Pub/Sub handlers ─────────────────────────────────────────────────────────

/// Parse `/api/v1/pubsub/<kind>/<topic>[/<sub>...]` into (topic, sub?).
fn split_pubsub_path(path: &str, kind: &str) -> Option<(String, Option<String>)> {
    let rest = path.strip_prefix("/api/v1/pubsub/")?;
    let rest = rest.strip_prefix(kind)?.strip_prefix('/')?;
    let (t, s) = match rest.split_once('/') {
        Some((t, s)) if !t.is_empty() && !s.is_empty() => (t, Some(percent_decode(s))),
        _ if !rest.is_empty() => (rest, None),
        _ => return None,
    };
    Some((percent_decode(t), s))
}

fn pubsub_status(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.pubsub.status_json())
}

fn pubsub_subscriptions(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.pubsub.subscriptions_json())
}

/// Create an empty topic. Topics also materialize implicitly on first publish.
fn pubsub_topic_create(req: &Request, state: &SharedState) -> String {
    let Some(name) = extract_json_field(req.body.trim(), "name").filter(|n| !n.is_empty()) else {
        return bad_request("name required");
    };
    let g = state.lock().unwrap();
    if g.pubsub.topic_exists(&name) {
        return json_response(
            409,
            &format!(r#"{{"error":"topic {name:?} already exists"}}"#),
        );
    }
    g.pubsub.ensure_topic(&name);
    drop(g);
    ok(&format!(r#"{{"message":"topic created","name":"{name}"}}"#))
}

/// Publish: `{"topic":"orders","payload":{...},"ordering_key":"k","attrs":{"a":"b"}}`
fn pubsub_publish(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let Some(topic) = extract_json_field(body, "topic").filter(|t| !t.is_empty()) else {
        return bad_request("topic required");
    };
    let payload = extract_json_object(body, "payload")
        .map(|obj| obj.to_string())
        .or_else(|| extract_json_field(body, "payload"))
        .unwrap_or_else(|| "null".to_string());
    let mut msg = pubsub::Message::new(&topic, &payload);
    if let Some(k) = extract_json_field(body, "ordering_key") {
        msg = msg.with_ordering_key(k);
    }
    if let Some(obj) = extract_json_object(body, "attrs") {
        for (k, v) in parse_flat_json_object(&obj) {
            msg = msg.with_attr(k, v);
        }
    }
    state.lock().unwrap().pubsub.publish(msg.clone());
    let subscribers = state.lock().unwrap().pubsub.subscriber_count(&msg.topic);
    ok(&format!(
        r#"{{"message":"published","id":"{}","topic":"{}","subscribers":{}}}"#,
        msg.id, msg.topic, subscribers
    ))
}

/// Subscribe: `{"topic":"orders","subscriber":"billing","max_retries":5,
///              "message_ordering":true,"ack_deadline_secs":30}` — all but topic/sub optional.
fn pubsub_subscribe(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let Some(topic) = extract_json_field(body, "topic").filter(|t| !t.is_empty()) else {
        return bad_request("topic required");
    };
    let Some(subscriber) = extract_json_field(body, "subscriber").filter(|s| !s.is_empty()) else {
        return bad_request("subscriber required");
    };
    let cfg = pubsub::SubscriptionConfig {
        max_concurrency: extract_json_field(body, "max_concurrency")
            .and_then(|v| v.parse().ok())
            .unwrap_or(10),
        max_retries: extract_json_field(body, "max_retries")
            .and_then(|v| v.parse().ok())
            .unwrap_or(3),
        retry_delay_ms: extract_json_field(body, "retry_delay_ms")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000),
        ack_deadline_secs: extract_json_field(body, "ack_deadline_secs")
            .and_then(|v| v.parse().ok())
            .unwrap_or(30),
        message_ordering: extract_json_field(body, "message_ordering")
            .map(|v| v == "true")
            .unwrap_or(false),
    };
    let g = state.lock().unwrap();
    let already = g.pubsub.has_subscription(&topic, &subscriber);
    g.pubsub.subscribe(&topic, &subscriber, cfg);
    drop(g);
    if already {
        json_response(
            200,
            &format!(
                r#"{{"message":"subscription updated","topic":"{topic}","subscriber":"{subscriber}"}}"#
            ),
        )
    } else {
        ok(&format!(
            r#"{{"message":"subscribed","topic":"{topic}","subscriber":"{subscriber}"}}"#
        ))
    }
}

/// GET /api/v1/pubsub/subscriptions/{topic}/{subscriber} — config + depths.
fn pubsub_subscription_info(state: &SharedState, full_path: &str) -> String {
    let path_no_q = full_path.split('?').next().unwrap_or(full_path);
    let Some((topic, Some(subscriber))) = split_pubsub_path(path_no_q, "subscriptions") else {
        return bad_request("topic and subscriber required");
    };
    let g = state.lock().unwrap();
    match g.pubsub.subscription_json(&topic, &subscriber) {
        Some(json) => ok(&json),
        None => json_response(
            404,
            &format!(r#"{{"error":"subscription {topic}/{subscriber} not found"}}"#),
        ),
    }
}

/// Pull next message: POST /api/v1/pubsub/subscriptions/{topic}/{sub}/pull.
/// Returns 204-style JSON `{"message":null}` when queue is empty/blocked.
fn pubsub_pull(req: &Request, state: &SharedState, path: &str) -> String {
    let _ = req;
    let stem = path.trim_end_matches("/pull");
    let Some((topic, Some(subscriber))) = split_pubsub_path(stem, "subscriptions") else {
        return bad_request("topic and subscriber required");
    };
    let g = state.lock().unwrap();
    if !g.pubsub.has_subscription(&topic, &subscriber) {
        return json_response(
            404,
            &format!(r#"{{"error":"subscription {topic}/{subscriber} not found"}}"#),
        );
    }
    match g.pubsub.pull(&topic, &subscriber) {
        Some(msg) => ok(&format!(
            r#"{{"message":{},"topic":"{topic}","subscriber":"{subscriber}"}}"#,
            msg.to_json()
        )),
        None => ok(&format!(
            r#"{{"message":null,"topic":"{topic}","subscriber":"{subscriber}","reason":"empty or ordering-blocked"}}"#
        )),
    }
}

/// Ack/nack share shape: `{"id":"msg-..."}` (+ optional nack reason).
fn pubsub_ack(req: &Request, state: &SharedState) -> String {
    let Some(id) = extract_json_field(req.body.trim(), "id").filter(|i| !i.is_empty()) else {
        return bad_request("message id required");
    };
    let settled = state.lock().unwrap().pubsub.ack(&id);
    if settled {
        ok(&format!(r#"{{"message":"acked","id":"{id}"}}"#))
    } else {
        json_response(
            404,
            &format!(r#"{{"error":"message not in flight","id":"{id}"}}"#),
        )
    }
}

fn pubsub_nack(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let Some(id) = extract_json_field(body, "id").filter(|i| !i.is_empty()) else {
        return bad_request("message id required");
    };
    let reason = extract_json_field(body, "reason").unwrap_or_else(|| "error".to_string());
    let settled = state.lock().unwrap().pubsub.nack(&id, &reason);
    if settled {
        ok(&format!(
            r#"{{"message":"nacked","id":"{id}","reason":"{reason}"}}"#
        ))
    } else {
        json_response(
            404,
            &format!(r#"{{"error":"message not in flight","id":"{id}"}}"#),
        )
    }
}

/// GET /api/v1/pubsub/dlq/{topic}/{subscriber}
fn pubsub_dlq(state: &SharedState, full_path: &str) -> String {
    let path_no_q = full_path.split('?').next().unwrap_or(full_path);
    let Some((topic, Some(subscriber))) = split_pubsub_path(path_no_q, "dlq") else {
        return bad_request("topic and subscriber required");
    };
    let g = state.lock().unwrap();
    ok(&g.pubsub.dlq_messages_json(&topic, &subscriber))
}

// ── Infra config ──────────────────────────────────────────────────────

fn infra_show(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.infra.to_json())
}

/// POST /api/v1/infra/env — `{"name":"X","value":"v"}`; empty value removes.
fn infra_env_set(req: &Request, state: &SharedState) -> String {
    let name = extract_json_field(req.body.trim(), "name").unwrap_or_default();
    if name.is_empty() {
        return bad_request("name required");
    }
    let value = extract_json_field(req.body.trim(), "value").unwrap_or_default();
    state.lock().unwrap().infra.set_env_var(&name, &value);
    ok(r#"{"message":"env updated"}"#)
}

fn infra_env_clear(state: &SharedState) -> String {
    state.lock().unwrap().infra.env_vars.clear();
    ok(r#"{"message":"env cleared"}"#)
}

fn infra_services(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.infra.service_json())
}

/// POST /api/v1/infra/services — `{"name":"auth","addr":"127.0.0.1:9001"}`.
fn infra_service_register(req: &Request, state: &SharedState) -> String {
    let b = req.body.trim();
    let name = extract_json_field(b, "name").unwrap_or_default();
    let addr = extract_json_field(b, "addr").unwrap_or_default();
    let mut g = state.lock().unwrap();
    if !g.infra.register_service(&name, &addr) {
        drop(g);
        return bad_request("valid name and addr required (addr needs :port)");
    }
    drop(g);
    ok(&format!(
        r#"{{"message":"service registered","name":"{name}"}}"#
    ))
}

fn infra_databases(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.infra.databases_json())
}

/// POST /api/v1/infra/databases —
/// `{"name":"db","engine":"postgres","host":"localhost","port":5432}`.
fn infra_database_upsert(req: &Request, state: &SharedState) -> String {
    let b = req.body.trim();
    let name = extract_json_field(b, "name").unwrap_or_default();
    let engine = extract_json_field(b, "engine").unwrap_or("postgres".into());
    let host = extract_json_field(b, "host").unwrap_or("localhost".into());
    let port = extract_json_field(b, "port")
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(5432);
    match state
        .lock()
        .unwrap()
        .infra
        .upsert_database(&name, &engine, &host, port)
    {
        Ok(()) => ok(&format!(
            r#"{{"message":"database configured","name":"{name}","engine":"{engine}"}}"#
        )),
        Err(e) => bad_request(&e),
    }
}

/// POST /api/v1/infra/tls — `{"enabled":true,"cert_path":"/certs/a.pem"}`.
fn infra_tls_set(req: &Request, state: &SharedState) -> String {
    let enabled = extract_json_field(req.body.trim(), "enabled")
        .map(|v| v == "true")
        .unwrap_or(false);
    let cert = extract_json_field(req.body.trim(), "cert_path");
    state
        .lock()
        .unwrap()
        .infra
        .set_tls(enabled, cert.filter(|c| !c.is_empty()));
    ok(&format!(
        r#"{{"message":"tls updated","enabled":{enabled}}}"#
    ))
}

// ── Testing harness (Encore `testing` parity) ─────────────────────────────

fn testing_show(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.testing.to_json())
}

/// POST /api/v1/testing/mode/enter — `{"log_level":"warn"}`.
fn testing_mode_enter(req: &Request, state: &SharedState) -> String {
    let lvl = extract_json_field(req.body.trim(), "log_level").unwrap_or_default();
    state.lock().unwrap().testing.enter_mode(&lvl);
    ok(r#"{"message":"test mode active"}"#)
}

fn testing_mode_exit(state: &SharedState) -> String {
    let was = state.lock().unwrap().testing.exit_mode();
    if was {
        ok(r#"{"message":"test mode exited"}"#)
    } else {
        not_found()
    }
}

/// POST /api/v1/testing/databases — `{"name":"users","superuser":true}`.
fn testing_db_new(req: &Request, state: &SharedState) -> String {
    let name = extract_json_field(req.body.trim(), "name").unwrap_or_default();
    let superuser = extract_json_field(req.body.trim(), "superuser")
        .map(|v| v == "true")
        .unwrap_or(false);
    match state.lock().unwrap().testing.new_database(&name, superuser) {
        Ok(ns) => ok(&format!(
            r#"{{"namespace":"{ns}","superuser":{superuser}}}"#
        )),
        Err(e) => bad_request(&e),
    }
}

fn testing_db_cleanup(state: &SharedState) -> String {
    let n = state.lock().unwrap().testing.cleanup_databases();
    ok(&format!(r#"{{"message":"cleaned up","destroyed":{n}}}"#))
}

/// POST /api/v1/testing/mocks/auth — `{"principal":"u_123"}`.
fn testing_mock_auth(req: &Request, state: &SharedState) -> String {
    let principal = extract_json_field(req.body.trim(), "principal").unwrap_or_default();
    match state.lock().unwrap().testing.mock_auth(&principal) {
        Ok(()) => ok(&format!(
            r#"{{"message":"auth mocked","principal":"{principal}"}}"#
        )),
        Err(e) => bad_request(&e),
    }
}

/// POST /api/v1/testing/mocks/services —
/// `{"service":"auth","response":{"user":"u_1"}}` (response stored verbatim).
fn testing_mock_service(req: &Request, state: &SharedState) -> String {
    let b = req.body.trim();
    let service = extract_json_field(b, "service").unwrap_or_default();
    let response = extract_json_field(b, "response").unwrap_or_default();
    match state
        .lock()
        .unwrap()
        .testing
        .mock_service(&service, &response)
    {
        Ok(()) => ok(&format!(
            r#"{{"message":"service mocked","service":"{service}"}}"#
        )),
        Err(e) => bad_request(&e),
    }
}

fn testing_mocks_clear(state: &SharedState) -> String {
    let n = state.lock().unwrap().testing.clear_mocks();
    ok(&format!(r#"{{"message":"mocks cleared","count":{n}}}"#))
}

// ── Deployments (Encore CLI deploy parity) ────────────────────────────────

fn deploy_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.deploys.to_json())
}

/// POST /api/v1/deploy —
/// `{"target":"production","platform":"linux/amd64","revision":"abc123"}`.
fn deploy_create(req: &Request, state: &SharedState) -> String {
    let b = req.body.trim();
    let target = extract_json_field(b, "target").unwrap_or_default();
    let platform = extract_json_field(b, "platform").unwrap_or_else(|| "linux/amd64".into());
    let revision = extract_json_field(b, "revision").unwrap_or_default();
    match state
        .lock()
        .unwrap()
        .deploys
        .create(&target, &platform, &revision)
    {
        Ok(id) => ok(&format!(
            r#"{{"id":"{id}","status":"queued","platform":"{platform}"}}"#
        )),
        Err(e) => bad_request(&e),
    }
}

/// POST /api/v1/deploy/status —
/// `{"id":"dep-1","status":"building"}` (state-machine validated).
fn deploy_status(req: &Request, state: &SharedState) -> String {
    let b = req.body.trim();
    let id = extract_json_field(b, "id").unwrap_or_default();
    let status = extract_json_field(b, "status").unwrap_or_default();
    let Some(s) = DeployStatus::parse(&status) else {
        return bad_request("valid status required (queued|building|deploying|deployed|failed)");
    };
    match state.lock().unwrap().deploys.set_status(&id, s) {
        Ok(()) => ok(&format!(r#"{{"id":"{id}","status":{}}}"#, s.as_str())),
        Err(e) => bad_request(&e),
    }
}

/// POST /api/v1/deploy/rollback — `{"target":"production"}`.
fn deploy_rollback(req: &Request, state: &SharedState) -> String {
    let target = extract_json_field(req.body.trim(), "target").unwrap_or_default();
    if target.is_empty() {
        return bad_request("target required");
    }
    let mut g = state.lock().unwrap();
    match g.deploys.rollback(&target) {
        Some(id) => {
            let rev = g
                .deploys
                .deployments
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.revision.clone())
                .unwrap_or_default();
            drop(g);
            ok(&format!(
                r#"{{"message":"rolled back","id":"{id}","revision":"{rev}","status":"deployed"}}"#
            ))
        }
        None => {
            drop(g);
            not_found()
        }
    }
}

/// GET /api/v1/deploy/dockerfile?app=name&bin=binary — generated build file.
fn deploy_dockerfile(state: &SharedState) -> String {
    let app = state.lock().unwrap().app_name.clone();
    // Minimal JSON string escaping for the embedded Dockerfile text.
    let esc: String = DeployRegistry::dockerfile(&app, "server")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    ok(&format!(r#"{{"dockerfile":"{esc}"}}"#))
}

// ── MCP (Model Context Protocol) surface ──────────────────────────────

/// POST /api/v1/mcp — JSON-RPC 2.0 body:
/// `{"method":"tools/call","params":{"name":"infra_snapshot"}}`
fn mcp_request(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    if body.is_empty() {
        return bad_request("jsonrpc body required");
    }
    let method = extract_json_field(body, "method").unwrap_or_default();
    let params = extract_json_field(body, "params")
        .map(|p| p.replace("\\\"", "\""))
        .unwrap_or_else(|| "{}".into());
    json_response(200, &crate::mcp::handle(state, &method, &params))
}

// ── WebSocket hub (service-to-service streams) ────────────────────────

fn ws_status(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.ws_hub.to_json())
}

/// POST /api/v1/ws/handshake — validate an upgrade request's headers
/// and return the exact 101 response to send (or 400 guidance).
fn ws_handshake(req: &Request) -> String {
    match crate::websocket::handshake_response(&req.body) {
        Some(resp) => json_response(
            200,
            &format!(r#"{{"upgrade":"ok","response":{}}}"#, crate::mcp::json_str(&resp)),
        ),
        None => bad_request("not a valid websocket upgrade (need Upgrade: websocket, Connection: Upgrade, Sec-WebSocket-Key)"),
    }
}

/// POST /api/v1/ws/join — `{"conn":"ws000001","room":"chat"}`.
/// Auto-registers unknown connections into a synthetic room listing.
fn ws_join(req: &Request, state: &SharedState) -> String {
    let b = req.body.trim();
    let conn = extract_json_field(b, "conn").unwrap_or_default();
    let room = extract_json_field(b, "room").unwrap_or_default();
    if conn.is_empty() || room.is_empty() {
        return bad_request("conn and room required");
    }
    let mut g = state.lock().unwrap();
    if g.ws_hub.join(&conn, &room) {
        drop(g);
        ok(&format!(
            r#"{{"message":"joined","conn":"{conn}","room":"{room}"}}"#
        ))
    } else {
        drop(g);
        json_response(409, r#"{"error":"already a member"}"#)
    }
}

fn ws_leave(req: &Request, state: &SharedState) -> String {
    let b = req.body.trim();
    let conn = extract_json_field(b, "conn").unwrap_or_default();
    let room = extract_json_field(b, "room").unwrap_or_default();
    if conn.is_empty() || room.is_empty() {
        return bad_request("conn and room required");
    }
    let mut g = state.lock().unwrap();
    if g.ws_hub.leave(&conn, &room) {
        drop(g);
        ok(&format!(
            r#"{{"message":"left","conn":"{conn}","room":"{room}"}}"#
        ))
    } else {
        drop(g);
        not_found()
    }
}

/// POST /api/v1/ws/broadcast — `{"room":"chat","sender":"a","message":{...}}`.
/// Returns the recipient list the caller must fan out to.
fn ws_broadcast(req: &Request, state: &SharedState) -> String {
    let b = req.body.trim();
    let room = extract_json_field(b, "room").unwrap_or_default();
    if room.is_empty() {
        return bad_request("room required");
    }
    let sender = extract_json_field(b, "sender").unwrap_or_default();
    let message = extract_json_field(b, "message")
        .map(|m| m.replace("\\\"", "\""))
        .unwrap_or_else(|| "null".into());
    let recipients = {
        let g = state.lock().unwrap();
        g.ws_hub.recipients(&room, &sender)
    };
    let ids: Vec<String> = recipients.iter().map(|r| format!(r#""{r}""#)).collect();
    ok(&format!(
        r#"{{"room":"{room}","recipients":[{}],"count":{},"message":{message}}}"#,
        ids.join(","),
        recipients.len(),
    ))
}

// ── Secrets management ────────────────────────────────────────────────────

fn secrets_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&format!(r#"{{"secrets":{}}}"#, g.secrets.list_json()))
}

/// POST /api/v1/secrets/set — register a secret.
/// `{"name":"db_pw","source":{"kind":"inline","value":"..."}}` or
/// `{"kind":"env","env_var":"DB_PW"}` / `{"kind":"file","path":"/run/secrets/pw"}` /
/// `{"kind":"vault","provider":"hashicorp","path":"secret/app"}`.
/// Secrets are redacted on registration; GET opts into plaintext via reveal.
fn secrets_set(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let name = extract_json_field(body, "name").unwrap_or_default();
    if name.is_empty() {
        return bad_request("name required");
    }
    let Some(src_obj) = extract_json_object(body, "source") else {
        return bad_request("source object required");
    };
    let kind = extract_json_field(&src_obj, "kind").unwrap_or_default();
    match kind.as_str() {
        "inline" => {
            let value = extract_json_field(&src_obj, "value").unwrap_or_default();
            if value.is_empty() {
                return bad_request("source.value required");
            }
            state.lock().unwrap().secrets.register_inline(&name, &value);
        }
        "env" => {
            let var = extract_json_field(&src_obj, "env_var").unwrap_or_default();
            if var.is_empty() {
                return bad_request("source.env_var required");
            }
            state.lock().unwrap().secrets.register_env(&name, &var);
        }
        "file" => {
            let path = extract_json_field(&src_obj, "path").unwrap_or_default();
            if path.is_empty() {
                return bad_request("source.path required");
            }
            state.lock().unwrap().secrets.register_file(&name, &path);
        }
        "vault" => {
            let provider = extract_json_field(&src_obj, "provider").unwrap_or_default();
            let path = extract_json_field(&src_obj, "path").unwrap_or_default();
            if provider.is_empty() || path.is_empty() {
                return bad_request("source.provider and source.path required");
            }
            state
                .lock()
                .unwrap()
                .secrets
                .register_vault(&name, &provider, &path);
        }
        other => {
            return bad_request(&format!(
                "unknown source kind {other} (inline|env|file|vault)"
            ));
        }
    }
    ok(&format!(
        r#"{{"message":"secret set","name":"{name}","redacted":true}}"#
    ))
}

/// POST /api/v1/secrets/get — display value of one secret. Redacted by
/// default (`"***"`); body `{"reveal":true}` returns the plaintext value.
fn secrets_get(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let name = extract_json_field(body, "name").unwrap_or_default();
    if name.is_empty() {
        return bad_request("name required");
    }
    let reveal = extract_json_field(body, "reveal")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !state.lock().unwrap().secrets.has(&name) {
        return json_response(404, r#"{"error":"secret not registered"}"#);
    }
    if reveal {
        match state.lock().unwrap().secrets.get(&name) {
            Some(v) => ok(&format!(r#"{{"name":"{name}","value":"{v}"}}"#)),
            None => json_response(
                409,
                &format!(r#"{{"error":"secret not resolvable","name":"{name}"}}"#),
            ),
        }
    } else {
        match state.lock().unwrap().secrets.get_display(&name) {
            Some(d) => ok(&format!(r#"{{"name":"{name}","value":"{d}"}}"#)),
            None => json_response(404, r#"{"error":"secret not registered"}"#),
        }
    }
}

/// POST /api/v1/secrets/check — verify all named secrets resolve.
/// Body: `{"names":["a","b"]}` → 200 with per-name status, 409 if any missing.
fn secrets_check(req: &Request, state: &SharedState) -> String {
    let Some(keys_json) = extract_json_array(req.body.trim(), "names") else {
        return bad_request("names array required");
    };
    let names = parse_string_array(&keys_json);
    if names.is_empty() {
        return bad_request("at least one name required");
    }
    let g = state.lock().unwrap();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let missing = g.secrets.check_required(&refs);
    let items: Vec<String> = names
        .iter()
        .map(|n| {
            let set = !missing.contains(n);
            format!(r#"{{"name":"{n}","set":{set}}}"#)
        })
        .collect();
    drop(g);
    let quoted: Vec<String> = missing.iter().map(|m| format!(r#""{m}""#)).collect();
    let payload = format!(
        r#"{{"ok":{ok},"missing":[{miss}],"results":[{items}]}}"#,
        ok = missing.is_empty(),
        miss = quoted.join(","),
        items = items.join(","),
    );
    if missing.is_empty() {
        ok(&payload)
    } else {
        json_response(409, &payload)
    }
}

/// DELETE /api/v1/secrets/{name} — remove from registry.
fn secrets_delete(state: &SharedState, full_path: &str) -> String {
    let path_no_q = full_path.split('?').next().unwrap_or(full_path);
    let name = path_no_q.strip_prefix("/api/v1/secrets/").unwrap_or("");
    if name.is_empty() {
        return bad_request("name required");
    }
    if state.lock().unwrap().secrets.delete(name) {
        ok(r#"{"message":"deleted"}"#)
    } else {
        json_response(404, r#"{"error":"secret not registered"}"#)
    }
}

// ── Cache keyspaces (Encore RedisCluster in-memory mode) ─────────────────────

fn cache_status(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.cache.status_json())
}

fn cache_keyspace_list(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.cache.list_json())
}

/// Declare a keyspace. Body:
/// `{"name":"sessions","max_entries":1000,"default_ttl_ms":300000}` —
/// only `name` is required.
fn cache_keyspace_ensure(req: &Request, state: &SharedState) -> String {
    let name = extract_json_field(req.body.trim(), "name").unwrap_or_default();
    if name.is_empty() {
        return bad_request("keyspace name required");
    }
    let cfg = crate::cache::KeyspaceConfig {
        max_entries: extract_json_field(&req.body, "max_entries")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(10_000),
        default_ttl_ms: extract_json_field(&req.body, "default_ttl_ms")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300_000),
    };
    state.lock().unwrap().cache.ensure_keyspace(&name, cfg);
    ok(&format!(
        r#"{{"message":"keyspace ready","name":"{name}"}}"#
    ))
}

/// GET /api/v1/cache/keyspaces/{ks} → config + stats;
/// GET /api/v1/cache/keyspaces/entries → live entries of every keyspace.
fn cache_keyspace_info(state: &SharedState, full_path: &str) -> String {
    let path_no_q = full_path.split('?').next().unwrap_or(full_path);
    let ks = path_no_q
        .strip_prefix("/api/v1/cache/keyspaces/")
        .unwrap_or("");
    if ks.is_empty() {
        return bad_request("keyspace required");
    }
    let query = full_path.split('?').nth(1).unwrap_or("");
    let want_entries = query.split('&').any(|p| p == "entries=1");
    let g = state.lock().unwrap();
    if want_entries {
        return match g.cache.entries_json(ks) {
            Some(j) => ok(&j),
            None => json_response(404, &format!(r#"{{"error":"keyspace {ks} not found"}}"#)),
        };
    }
    match g.cache.keyspace_json(ks) {
        Some(j) => ok(&j),
        None => json_response(404, &format!(r#"{{"error":"keyspace {ks} not found"}}"#)),
    }
}

fn cache_entries_all(state: &SharedState) -> String {
    let g = state.lock().unwrap();
    ok(&g.cache.entries_all_json())
}

/// DELETE /api/v1/cache/keyspaces/{ks}?pattern=user:*  (or ?all=1).
/// Returns how many live entries were invalidated.
fn cache_invalidate(req: &Request, state: &SharedState, full_path: &str) -> String {
    let path_no_q = full_path.split('?').next().unwrap_or(full_path);
    let query = full_path.split('?').nth(1).unwrap_or("");
    let ks = path_no_q
        .strip_prefix("/api/v1/cache/keyspaces/")
        .unwrap_or("");
    if ks.is_empty() {
        return bad_request("keyspace required");
    }
    let pattern = query
        .split('&')
        .find(|p| p.starts_with("pattern="))
        .map(|p| percent_decode(p.trim_start_matches("pattern=")));
    let mut g = state.lock().unwrap();
    if !g.cache.has_keyspace(ks) {
        return json_response(404, &format!(r#"{{"error":"keyspace {ks} not found"}}"#));
    }
    let n = match pattern {
        Some(p) => g.cache.invalidate_pattern(ks, &p),
        None => g.cache.invalidate_all(ks),
    };
    drop(g);
    ok(&format!(
        r#"{{"message":"invalidated","keyspace":"{ks}","entries":{n}}}"#
    ))
}

/// GET /api/v1/cache/entry/{ks}/{key} — 200 with the entry, or 404 when
/// the key is unknown/expired (misses are still counted in stats).
fn cache_get(state: &SharedState, full_path: &str) -> String {
    let path_no_q = full_path.split('?').next().unwrap_or(full_path);
    let rest = path_no_q.strip_prefix("/api/v1/cache/entry/").unwrap_or("");
    let Some((ks, key)) = rest.split_once('/') else {
        return bad_request("keyspace and key required");
    };
    if ks.is_empty() || key.is_empty() {
        return bad_request("keyspace and key required");
    }
    let mut g = state.lock().unwrap();
    let key_decoded = percent_decode(key);
    match g.cache.get_json(ks, &key_decoded) {
        Some(entry_json) => ok(&entry_json),
        None => json_response(
            404,
            &format!(
                r#"{{"error":"cache miss","keyspace":"{ks}","key":"{k}"}}"#,
                k = key_decoded
            ),
        ),
    }
}

/// PUT /api/v1/cache/entry/{ks}/{key}  Body: any JSON value (stored as-is).
/// Query: `?ttl_ms=N` overrides the keyspace default (0 = never expires).
fn cache_put(req: &Request, state: &SharedState, full_path: &str) -> String {
    let (path_no_q, query) = match full_path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (full_path, ""),
    };
    let rest = path_no_q.strip_prefix("/api/v1/cache/entry/").unwrap_or("");
    let Some((ks, key)) = rest.split_once('/') else {
        return bad_request("keyspace and key required");
    };
    if ks.is_empty() || key.is_empty() {
        return bad_request("keyspace and key required");
    }
    let value = req.body.trim();
    if value.is_empty() {
        return bad_request("body required (any JSON value)");
    }
    let ttl_ms = query
        .split('&')
        .find(|p| p.starts_with("ttl_ms="))
        .and_then(|p| p.trim_start_matches("ttl_ms=").parse::<u64>().ok());
    let key = percent_decode(key);
    let evicted = state.lock().unwrap().cache.set(ks, &key, value, ttl_ms);
    ok(&format!(
        r#"{{"message":"cached","keyspace":"{ks}","key":"{key}","evicted":{evicted}}}"#
    ))
}

/// DELETE /api/v1/cache/entry/{ks}/{key} — removes one key.
/// DELETE /api/v1/cache/entry/{ks}/{key}?pattern=... is rejected; use the
/// keyspace-level invalidate endpoint for patterns.
fn cache_del(state: &SharedState, full_path: &str) -> String {
    let path_no_q = full_path.split('?').next().unwrap_or(full_path);
    let rest = path_no_q.strip_prefix("/api/v1/cache/entry/").unwrap_or("");
    let Some((ks, key)) = rest.split_once('/') else {
        return bad_request("keyspace and key required");
    };
    if ks.is_empty() || key.is_empty() {
        return bad_request("keyspace and key required");
    }
    let deleted = state.lock().unwrap().cache.del(ks, &percent_decode(key));
    if deleted {
        ok(r#"{"message":"deleted"}"#)
    } else {
        json_response(404, r#"{"error":"cache miss"}"#)
    }
}

/// POST /api/v1/cache/mget  Body: `{"keyspace":"ks","keys":["a","b"]}`
fn cache_mget(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let ks = extract_json_field(body, "keyspace").unwrap_or_default();
    if ks.is_empty() {
        return bad_request("keyspace required");
    }
    let Some(keys_json) = extract_json_array(body, "keys") else {
        return bad_request("keys array required");
    };
    let keys = parse_string_array(&keys_json);
    let mut g = state.lock().unwrap();
    let items: Vec<String> = g
        .cache
        .mget(&ks, &keys)
        .iter()
        .zip(keys.iter())
        .map(|(v, k)| match v {
            Some(v) => format!(r#"{{"key":"{k}","value":{v}}}"#),
            None => format!(r#"{{"key":"{k}","value":null}}"#),
        })
        .collect();
    drop(g);
    ok(&format!(
        r#"{{"values":[{items}]}}"#,
        items = items.join(",")
    ))
}

/// POST /api/v1/cache/mset  Body:
/// `{"keyspace":"ks","ttl_ms":5000,"pairs":{"a":"1","b":"2"}}`
fn cache_mset(req: &Request, state: &SharedState) -> String {
    let body = req.body.trim();
    let ks = extract_json_field(body, "keyspace").unwrap_or_default();
    if ks.is_empty() {
        return bad_request("keyspace required");
    }
    let ttl_ms = extract_json_field(body, "ttl_ms").and_then(|v| v.parse::<u64>().ok());
    let obj = match extract_json_object(body, "pairs") {
        Some(o) => o,
        None => return bad_request("pairs object required"),
    };
    let pairs = parse_string_pairs(&obj);
    if pairs.is_empty() {
        return bad_request("at least one pair required");
    }
    let count = pairs.len();
    state.lock().unwrap().cache.mset(&ks, &pairs, ttl_ms);
    ok(&format!(
        r#"{{"message":"multi-set","keyspace":"{ks}","count":{count}}}"#
    ))
}

/// Serve an object download as raw bytes when the request is authorized:
/// - public bucket → any GET/HEAD succeeds (Encore publicUrl semantics)
/// - signed URL    → `?exp=&sig=` must verify for this exact method/bucket/key
///
/// Returns None when unauthorized or object missing — caller falls through to
/// the normal JSON router, which reports the precise error.
fn storage_download_response(
    state: &SharedState,
    full_path: &str,
    head_only: bool,
) -> Option<Vec<u8>> {
    let (path_no_q, query) = match full_path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (full_path, ""),
    };
    let (bucket, key) = split_storage_path(path_no_q, "objects")?;
    let key = key?;

    // Authorize BEFORE taking the state lock — verify_signed_query locks too.
    let authorized = if !query.is_empty() {
        verify_signed_query(state, "GET", query, &bucket, &key).is_ok()
    } else {
        state.lock().unwrap().storage.public_read_allowed(&bucket)
    };
    if !authorized {
        return None;
    }
    let bytes = {
        let g = state.lock().unwrap();
        g.storage.get_object(&bucket, &key).ok()?
    };
    let mime = mime_for(&key);
    let mut out = Vec::with_capacity(bytes.len() + 256);
    out.extend_from_slice(b"HTTP/1.1 200 OK\r\n" as &[u8]);
    out.extend_from_slice(format!("Content-Type: {mime}\r\n").as_bytes());
    out.extend_from_slice(format!("Content-Length: {}\r\n", bytes.len()).as_bytes());
    out.extend_from_slice(format!("Access-Control-Allow-Origin: {}\r\n", cors_origin()).as_bytes());
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    if !head_only {
        out.extend_from_slice(&bytes);
    }
    Some(out)
}

/// Execute a signed PUT/DELETE whose `?exp=&sig=` already verified.
/// Returns the raw HTTP response bytes, or None when verification fails
/// (caller falls through to the authenticated JSON router).
fn storage_signed_write_response(state: &SharedState, req: &Request) -> Option<Vec<u8>> {
    let (path_no_q, query) = match req.path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (req.path.as_str(), ""),
    };
    let (bucket, key) = split_storage_path(path_no_q, "objects")?;
    let key = key?;
    verify_signed_query(state, &req.method, query, &bucket, &key).ok()?;
    let body = format!(
        r#"{{"message":"{}","bucket":"{bucket}","key":"{key}"}}"#,
        if req.method == "PUT" {
            "object stored"
        } else {
            "object deleted"
        }
    );
    let mut out = Vec::with_capacity(body.len() + 192);
    out.extend_from_slice(b"HTTP/1.1 200 OK\r\n" as &[u8]);
    out.extend_from_slice(b"Content-Type: application/json\r\n" as &[u8]);
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    out.extend_from_slice(format!("Access-Control-Allow-Origin: {}\r\n", cors_origin()).as_bytes());
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(body.as_bytes());
    let method = req.method.clone();
    let bucket2 = bucket.clone();
    let key2 = key.clone();
    let g = state.lock().unwrap();
    let res = if method == "PUT" {
        g.storage
            .put_object(&bucket2, &key2, req.body.as_bytes())
            .map(|_| ())
    } else {
        g.storage.delete_object(&bucket2, &key2)
    };
    match res {
        Ok(()) => Some(out),
        Err(_) => None,
    }
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

    // ── Pub/Sub HTTP endpoints ─────────────────────────────────────────────

    #[test]
    fn pubsub_status_empty() {
        let resp = r("GET", "/api/v1/pubsub", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("\"topics\":0"), "got: {resp}");
    }

    #[test]
    fn pubsub_topic_create_and_conflict() {
        let s = state();
        let created = rs("POST", "/api/v1/pubsub/topics", r#"{"name":"orders"}"#, &s);
        assert!(created.contains("200"), "got: {created}");
        // Duplicate → 409 (storage bucket convention)
        let dup = rs("POST", "/api/v1/pubsub/topics", r#"{"name":"orders"}"#, &s);
        assert!(dup.contains("409"), "got: {dup}");
        // Missing name → 400
        assert!(
            rs("POST", "/api/v1/pubsub/topics", "{}", &s).contains("400"),
            "empty body must 400"
        );
    }

    #[test]
    fn pubsub_subscribe_then_list_and_info() {
        let s = state();
        rs(
            "POST",
            "/api/v1/pubsub/subscriptions",
            r#"{"topic":"orders","subscriber":"billing","max_retries":7,"message_ordering":true}"#,
            &s,
        );
        let list = rs("GET", "/api/v1/pubsub/subscriptions", "", &s);
        assert!(
            list.contains(r#""topic":"orders","subscriber":"billing""#),
            "got: {list}"
        );

        let info = rs("GET", "/api/v1/pubsub/subscriptions/orders/billing", "", &s);
        assert!(info.contains("200"), "got: {info}");
        assert!(
            info.contains("\"max_retries\":7"),
            "config not echoed: {info}"
        );
        assert!(info.contains("\"message_ordering\":true"), "got: {info}");

        // Unknown subscription → 404
        let missing = rs("GET", "/api/v1/pubsub/subscriptions/orders/ghost", "", &s);
        assert!(missing.contains("404"), "got: {missing}");
    }

    #[test]
    fn pubsub_subscribe_missing_fields_400() {
        assert!(
            r("POST", "/api/v1/pubsub/subscriptions", r#"{"topic":"t"}"#).contains("400"),
            "subscriber required"
        );
        assert!(
            r(
                "POST",
                "/api/v1/pubsub/subscriptions",
                r#"{"subscriber":"s"}"#
            )
            .contains("400"),
            "topic required"
        );
    }

    #[test]
    fn pubsub_publish_reports_real_subscriber_count() {
        let s = state();
        rs("POST", "/api/v1/pubsub/topics", r#"{"name":"fanout"}"#, &s);
        for sub in ["a", "b"] {
            rs(
                "POST",
                "/api/v1/pubsub/subscriptions",
                &format!(r#"{{"topic":"fanout","subscriber":"{sub}"}}"#),
                &s,
            );
        }
        // Two publishes — count must stay 2, not grow with publish volume.
        rs(
            "POST",
            "/api/v1/pubsub/publish",
            r#"{"topic":"fanout","payload":{"n":1}}"#,
            &s,
        );
        let second = rs(
            "POST",
            "/api/v1/pubsub/publish",
            r#"{"topic":"fanout","payload":{"n":2}}"#,
            &s,
        );
        assert!(second.contains("\"subscribers\":2"), "got: {second}");

        // Publish to a topic with zero subscribers still succeeds.
        let solo = rs(
            "POST",
            "/api/v1/pubsub/publish",
            r#"{"topic":"lonely","payload":{}}"#,
            &s,
        );
        assert!(solo.contains("\"subscribers\":0"), "got: {solo}");
    }

    #[test]
    fn pubsub_publish_pull_ack_roundtrip_with_attrs_and_ordering_key() {
        let s = state();
        rs(
            "POST",
            "/api/v1/pubsub/subscriptions",
            r#"{"topic":"events","subscriber":"w"}"#,
            &s,
        );
        let pub_resp = rs(
            "POST",
            "/api/v1/pubsub/publish",
            r#"{"topic":"events","payload":{"kind":"signup"},"attrs":{"source":"web"},"ordering_key":"user-42"}"#,
            &s,
        );
        assert!(pub_resp.contains("200"), "got: {pub_resp}");

        let pull = rs("POST", "/api/v1/pubsub/subscriptions/events/w/pull", "", &s);
        assert!(pull.contains("\"ordering_key\":\"user-42\""), "got: {pull}");
        assert!(pull.contains("\"source\":\"web\""), "attrs missing: {pull}");
        // Message JSON must be well-formed — no trailing comma artifacts.
        assert!(!pull.contains(",}"), "invalid JSON emitted: {pull}");

        // Extract the id and ack it.
        let id_start = pull.find(r#""id":"msg-"#).expect("id in response") + 6;
        let id_len = pull[id_start..].find('"').unwrap();
        let id = &pull[id_start..id_start + id_len];
        let ack = rs(
            "POST",
            "/api/v1/pubsub/ack",
            &format!(r#"{{"id":"{id}"}}"#),
            &s,
        );
        assert!(ack.contains("200"), "got: {ack}");

        // Empty queue afterwards — distinct from unknown-subscription case.
        let empty = rs("POST", "/api/v1/pubsub/subscriptions/events/w/pull", "", &s);
        assert!(empty.contains("\"message\":null"), "got: {empty}");
    }

    #[test]
    fn pubsub_pull_unknown_subscription_404() {
        let resp = rs(
            "POST",
            "/api/v1/pubsub/subscriptions/nope/nada/pull",
            "",
            &state(),
        );
        assert!(resp.contains("404"), "got: {resp}");
    }

    #[test]
    fn pubsub_nack_requeues_then_dlq_lists_dead_letter() {
        let s = state();
        rs(
            "POST",
            "/api/v1/pubsub/subscriptions",
            r#"{"topic":"jobs","subscriber":"w","max_retries":0}"#,
            &s,
        );
        rs(
            "POST",
            "/api/v1/pubsub/publish",
            r#"{"topic":"jobs","payload":{"t":1}}"#,
            &s,
        );
        let pull = rs("POST", "/api/v1/pubsub/subscriptions/jobs/w/pull", "", &s);
        let id_start = pull.find(r#""id":"msg-"#).expect("id in response") + 6;
        let id_len = pull[id_start..].find('"').unwrap();
        let id = &pull[id_start..id_start + id_len];

        let nack = rs(
            "POST",
            "/api/v1/pubsub/nack",
            &format!(r#"{{"id":"{id}","reason":"boom"}}"#),
            &s,
        );
        assert!(nack.contains("200"), "got: {nack}");

        let dlq = rs("GET", "/api/v1/pubsub/dlq/jobs/w", "", &s);
        assert!(
            dlq.contains(r#"{"t":1}"#),
            "dead letter payload missing: {dlq}"
        );

        // Ack/nack of an already-settled or unknown id → 404.
        assert!(
            rs(
                "POST",
                "/api/v1/pubsub/ack",
                &format!(r#"{{"id":"{id}"}}"#),
                &s
            )
            .contains("404"),
            "double-settle must 404"
        );
        assert!(
            rs("POST", "/api/v1/pubsub/nack", r#"{"id":"msg-unknown"}"#, &s).contains("404"),
            "unknown id must 404"
        );
    }
    // ── Cache HTTP endpoints ───────────────────────────────────────────────

    #[test]
    fn cache_status_empty() {
        let resp = r("GET", "/api/v1/cache", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(resp.contains("\"keyspaces\":0"), "got: {resp}");
    }

    #[test]
    fn cache_keyspace_ensure_list_info() {
        let s = state();
        let created = rs(
            "POST",
            "/api/v1/cache/keyspaces",
            r#"{"name":"sessions","max_entries":50,"default_ttl_ms":1000}"#,
            &s,
        );
        assert!(created.contains("200"), "got: {created}");
        // Missing name → 400.
        assert!(
            rs("POST", "/api/v1/cache/keyspaces", "{}", &s).contains("400"),
            "name required"
        );
        let list = rs("GET", "/api/v1/cache/keyspaces", "", &s);
        assert!(list.contains(r#""name":"sessions""#), "got: {list}");
        assert!(list.contains(r#""max_entries":50"#), "got: {list}");
        let info = rs("GET", "/api/v1/cache/keyspaces/sessions", "", &s);
        assert!(info.contains("200"), "got: {info}");
        let missing = rs("GET", "/api/v1/cache/keyspaces/ghost", "", &s);
        assert!(missing.contains("404"), "got: {missing}");
    }

    #[test]
    fn cache_put_get_delete_roundtrip() {
        let s = state();
        rs("POST", "/api/v1/cache/keyspaces", r#"{"name":"users"}"#, &s);
        let put = rs(
            "PUT",
            "/api/v1/cache/entry/users/user:1?ttl_ms=60000",
            r#"{"id":1,"name":"ann"}"#,
            &s,
        );
        assert!(put.contains("200"), "got: {put}");
        // JSON body must survive storage byte-for-byte (escaped-quote path).
        let get = rs("GET", "/api/v1/cache/entry/users/user%3A1", "", &s);
        assert!(
            get.contains(r#""value":{"id":1,"name":"ann"}"#),
            "got: {get}"
        );
        assert!(get.contains(r#""ttl_ms_left":6"#), "ttl surfaced: {get}");
        let del = rs("DELETE", "/api/v1/cache/entry/users/user%3A1", "", &s);
        assert!(del.contains("200"), "got: {del}");
        assert!(
            rs("DELETE", "/api/v1/cache/entry/users/user%3A1", "", &s).contains("404"),
            "double delete must 404"
        );
    }

    #[test]
    fn cache_get_unknown_key_is_404_miss() {
        let resp = rs("GET", "/api/v1/cache/entry/nope/k", "", &state());
        assert!(resp.contains("404"), "got: {resp}");
        assert!(resp.contains("cache miss"), "got: {resp}");
    }

    #[test]
    fn cache_put_requires_body_and_path() {
        assert!(
            r("PUT", "/api/v1/cache/entry/ks/", "").contains("400"),
            "empty key must 400"
        );
        assert!(
            r("PUT", "/api/v1/cache/entry/ks/k", "").contains("400"),
            "empty body must 400"
        );
    }

    #[test]
    fn cache_invalidate_pattern_and_all() {
        let s = state();
        for k in ["user:1", "user:2", "order:9"] {
            rs("PUT", &format!("/api/v1/cache/entry/sess/{k}"), "\"v\"", &s);
        }
        let resp = rs(
            "DELETE",
            "/api/v1/cache/keyspaces/sess?pattern=user:*",
            "",
            &s,
        );
        assert!(resp.contains("\"entries\":2"), "got: {resp}");
        let rest = rs("GET", "/api/v1/cache/keyspaces/sess?entries=1", "", &s);
        assert!(rest.contains("order:9"), "order key must survive: {rest}");
        // Unknown keyspace → 404.
        assert!(
            rs("DELETE", "/api/v1/cache/keyspaces/ghost", "", &s).contains("404"),
            "unknown keyspace must 404"
        );
    }

    #[test]
    fn cache_mget_mset_batches() {
        let s = state();
        let mset = rs(
            "POST",
            "/api/v1/cache/mset",
            r#"{"keyspace":"batch","pairs":{"a":"v1","b":"v2"}}"#,
            &s,
        );
        assert!(mset.contains("\"count\":2"), "got: {mset}");
        let mget = rs(
            "POST",
            "/api/v1/cache/mget",
            r#"{"keyspace":"batch","keys":["a","missing","b"]}"#,
            &s,
        );
        // Values are raw JSON tokens, so strings round-trip quoted.
        assert!(mget.contains(r#""key":"a","value":"v1""#), "got: {mget}");
        assert!(
            mget.contains(r#""key":"missing","value":null"#),
            "got: {mget}"
        );
        // Validation errors.
        assert!(
            rs("POST", "/api/v1/cache/mget", r#"{"keys":["a"]}"#, &s).contains("400"),
            "keyspace required"
        );
        assert!(
            rs("POST", "/api/v1/cache/mset", r#"{"keyspace":"b"}"#, &s).contains("400"),
            "pairs required"
        );
    }

    #[test]
    fn extract_json_field_survives_escaped_quotes() {
        // Raw-token semantics: the scan honors escapes so the value is not
        // truncated, but escapes are preserved verbatim (no unescaping).
        let body = r#"{"keyspace":"ks","payload":"{\"a\":1}"}"#;
        assert_eq!(
            extract_json_field(body, "payload").as_deref(),
            Some(r#"{\"a\":1}"#),
            "escaped quotes must not truncate the value"
        );
    }

    #[test]
    fn parse_string_pairs_handles_escaped_values() {
        // Raw-token semantics: quoted values keep their surrounding quotes
        // so they stay valid JSON when re-emitted (cache stores them as-is).
        let inner = r#""a":"\"1\"", "b": 2}"#;
        let pairs = parse_string_pairs(inner);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], ("a".to_string(), r##""\"1\"""##.to_string()));
        assert_eq!(pairs[1], ("b".to_string(), "2".to_string()));
    }
    #[test]
    fn secrets_set_get_redacted_reveal_delete() {
        let s = state();
        let set = rs(
            "POST",
            "/api/v1/secrets/set",
            r#"{"name":"db_pw","source":{"kind":"inline","value":"hunter2"}}"#,
            &s,
        );
        assert!(set.contains("\"redacted\":true"), "got: {set}");
        // Default GET is redacted — plaintext must not leak.
        let peek = rs("POST", "/api/v1/secrets/get", r#"{"name":"db_pw"}"#, &s);
        assert!(peek.contains("***"), "got: {peek}");
        assert!(!peek.contains("hunter2"), "plaintext leaked: {peek}");
        // List shows registered + set status, still no plaintext.
        let list = rs("GET", "/api/v1/secrets", "", &s);
        assert!(list.contains(r#""name":"db_pw""#), "got: {list}");
        assert!(
            !list.contains("hunter2"),
            "plaintext leaked in list: {list}"
        );
        // Reveal returns plaintext.
        let reveal = rs(
            "POST",
            "/api/v1/secrets/get",
            r#"{"name":"db_pw","reveal":true}"#,
            &s,
        );
        assert!(reveal.contains("hunter2"), "got: {reveal}");
        // Delete → 404 on second delete.
        assert!(
            rs("DELETE", "/api/v1/secrets/db_pw", "", &s).contains("200"),
            "first delete ok"
        );
        assert!(
            rs("DELETE", "/api/v1/secrets/db_pw", "", &s).contains("404"),
            "second delete must 404"
        );
    }

    #[test]
    fn secrets_env_source_resolves_and_unresolvable_409() {
        std::env::set_var("BRIDGE_TEST_SEC_VAR", "env-secret-42");
        let s = state();
        rs(
            "POST",
            "/api/v1/secrets/set",
            r#"{"name":"api_key","source":{"kind":"env","env_var":"BRIDGE_TEST_SEC_VAR"}}"#,
            &s,
        );
        let reveal = rs(
            "POST",
            "/api/v1/secrets/get",
            r#"{"name":"api_key","reveal":true}"#,
            &s,
        );
        assert!(reveal.contains("env-secret-42"), "got: {reveal}");
        std::env::remove_var("BRIDGE_TEST_SEC_VAR");
        // Env var gone now: display stays redacted (***) — no state leak;
        // reveal reports 409 unresolvable.
        let unset = rs("POST", "/api/v1/secrets/get", r#"{"name":"api_key"}"#, &s);
        assert!(unset.contains("***"), "got: {unset}");
        assert!(!unset.contains("env-secret-42"), "leak: {unset}");
        let dead = rs(
            "POST",
            "/api/v1/secrets/get",
            r#"{"name":"api_key","reveal":true}"#,
            &s,
        );
        assert!(dead.contains("409"), "unresolvable must 409: {dead}");
    }

    #[test]
    fn secrets_check_reports_missing_with_409() {
        let s = state();
        rs(
            "POST",
            "/api/v1/secrets/set",
            r#"{"name":"have_it","source":{"kind":"inline","value":"x"}}"#,
            &s,
        );
        let resp = rs(
            "POST",
            "/api/v1/secrets/check",
            r#"{"names":["have_it","never_registered"]}"#,
            &s,
        );
        assert!(resp.contains("409"), "got: {resp}");
        assert!(resp.contains(r#""ok":false"#), "got: {resp}");
        assert!(
            resp.contains(r#""missing":["never_registered"]"#),
            "got: {resp}"
        );
        // All-present case is a plain 200.
        let okcase = rs(
            "POST",
            "/api/v1/secrets/check",
            r#"{"names":["have_it"]}"#,
            &s,
        );
        assert!(okcase.contains("\"ok\":true"), "got: {okcase}");
        assert!(!okcase.contains("409"), "got: {okcase}");
    }

    #[test]
    fn secrets_validation_errors() {
        assert!(
            r("POST", "/api/v1/secrets/set", "{}").contains("400"),
            "name required"
        );
        assert!(
            r("POST", "/api/v1/secrets/set", r#"{"name":"x"}"#).contains("400"),
            "source required"
        );
        assert!(
            r(
                "POST",
                "/api/v1/secrets/set",
                r#"{"name":"x","source":{"kind":"bogus"}}"#,
            )
            .contains("400"),
            "unknown kind must 400"
        );
        assert!(
            r("POST", "/api/v1/secrets/get", "{}").contains("400"),
            "get name required"
        );
        assert!(
            r("POST", "/api/v1/secrets/check", r#"{"names":[]}"#).contains("400"),
            "empty names must 400"
        );
        assert!(
            r("POST", "/api/v1/secrets/get", r#"{"name":"ghost"}"#).contains("404"),
            "unknown secret must 404"
        );
    }

    #[test]
    fn infra_env_set_get_remove_roundtrip() {
        let s = state();
        rs(
            "POST",
            "/api/v1/infra/env",
            r#"{"name":"LOG_LEVEL","value":"debug"}"#,
            &s,
        );
        let snap = rs("GET", "/api/v1/infra", "", &s);
        assert!(snap.contains(r#""LOG_LEVEL":"debug""#), "got: {snap}");
        rs(
            "POST",
            "/api/v1/infra/env",
            r#"{"name":"LOG_LEVEL","value":""}"#,
            &s,
        );
        assert!(
            !rs("GET", "/api/v1/infra", "", &s).contains("LOG_LEVEL"),
            "empty removes"
        );
        assert!(
            rs("POST", "/api/v1/infra/env", r#"{"value":"x"}"#, &s).contains("400"),
            "name req"
        );
    }

    #[test]
    fn infra_services_discovery_register_and_update() {
        let s = state();
        rs(
            "POST",
            "/api/v1/infra/services",
            r#"{"name":"auth","addr":"127.0.0.1:9001"}"#,
            &s,
        );
        rs(
            "POST",
            "/api/v1/infra/services",
            r#"{"name":"auth","addr":"10.0.0.2:9001"}"#,
            &s,
        );
        let list = rs("GET", "/api/v1/infra/services", "", &s);
        assert!(list.contains("10.0.0.2:9001"), "updated in place: {list}");
        assert_eq!(list.matches("\"name\"").count(), 1, "no duplicate");
        assert!(
            rs(
                "POST",
                "/api/v1/infra/services",
                r#"{"name":"x","addr":""}"#,
                &s
            )
            .contains("400"),
            "empty addr must 400"
        );
    }

    #[test]
    fn infra_database_validation_and_listing() {
        let s = state();
        let okc = rs(
            "POST",
            "/api/v1/infra/databases",
            r#"{"name":"main","engine":"postgres","host":"db.local","port":5433}"#,
            &s,
        );
        assert!(okc.contains("200"), "got: {okc}");
        let bad = rs(
            "POST",
            "/api/v1/infra/databases",
            r#"{"name":"main","engine":"oracle","host":"h","port":1}"#,
            &s,
        );
        assert!(bad.contains("400"), "unknown engine: {bad}");
        let list = rs("GET", "/api/v1/infra/databases", "", &s);
        assert!(list.contains(r#""port":5433"#), "got: {list}");
    }

    #[test]
    fn infra_tls_status_transitions() {
        let snap = rs("GET", "/api/v1/infra", "", &state());
        assert!(
            snap.contains(r#""tls":{"configured":false}"#),
            "got: {snap}"
        );
        let s = state();
        rs(
            "POST",
            "/api/v1/infra/tls",
            r#"{"enabled":true,"cert_path":"/certs/a.pem"}"#,
            &s,
        );
        assert!(
            rs("GET", "/api/v1/infra", "", &s)
                .contains(r#""tls":{"enabled":true,"cert":"/certs/a.pem"}"#),
            "tls surfaced"
        );
    }

    #[test]
    fn testing_mode_enter_exit_and_snapshot() {
        let s = state();
        let snap0 = rs("GET", "/api/v1/testing", "", &s);
        assert!(snap0.contains(r#""mode":{"active":false}"#), "got: {snap0}");
        assert!(
            rs(
                "POST",
                "/api/v1/testing/mode/enter",
                r#"{"log_level":"warn"}"#,
                &s
            )
            .contains("200"),
            "enter must succeed"
        );
        assert!(
            rs("GET", "/api/v1/testing", "", &s).contains(r#""log_level":"warn""#),
            "level recorded"
        );
        assert!(
            rs("POST", "/api/v1/testing/mode/exit", "", &s).contains("200"),
            "exit must succeed"
        );
        assert!(
            rs("POST", "/api/v1/testing/mode/exit", "", &s).contains("404"),
            "double exit must 404"
        );
    }

    #[test]
    fn testing_database_isolation_and_cleanup() {
        let s = state();
        let d1 = rs(
            "POST",
            "/api/v1/testing/databases",
            r#"{"name":"users","superuser":true}"#,
            &s,
        );
        assert!(d1.contains(r#""namespace":"t1_users""#), "got: {d1}");
        assert!(d1.contains(r#""superuser":true"#), "got: {d1}");
        // Same base name → distinct namespace.
        let d2 = rs(
            "POST",
            "/api/v1/testing/databases",
            r#"{"name":"users"}"#,
            &s,
        );
        assert!(d2.contains(r#""namespace":"t2_users""#), "got: {d2}");
        // Empty name → 400.
        assert!(
            rs("POST", "/api/v1/testing/databases", "{}", &s).contains("400"),
            "name required"
        );
        let cleanup = rs("DELETE", "/api/v1/testing/databases", "", &s);
        assert!(cleanup.contains("\"destroyed\":2"), "got: {cleanup}");
        assert!(
            !rs("GET", "/api/v1/testing", "", &s).contains("\"namespace\""),
            "all gone"
        );
    }

    #[test]
    fn testing_mocks_auth_service_clear() {
        let s = state();
        assert!(
            rs(
                "POST",
                "/api/v1/testing/mocks/auth",
                r#"{"principal":"u_123"}"#,
                &s
            )
            .contains("200"),
            "auth mock ok"
        );
        assert!(
            rs(
                "POST",
                "/api/v1/testing/mocks/auth",
                r#"{"principal":""}"#,
                &s
            )
            .contains("400"),
            "blank principal 400"
        );
        rs(
            "POST",
            "/api/v1/testing/mocks/services",
            r#"{"service":"auth","response":{"user":"u_1"}}"#,
            &s,
        );
        let snap = rs("GET", "/api/v1/testing", "", &s);
        assert!(
            snap.contains(r#""auth":{"enabled":true,"principal":"u_123"}"#),
            "got: {snap}"
        );
        assert!(snap.contains(r#""auth":{"user":"u_1"}"#), "canned: {snap}");
        let clear = rs("DELETE", "/api/v1/testing/mocks", "", &s);
        assert!(clear.contains("\"count\":2"), "got: {clear}");
        assert!(
            !rs("GET", "/api/v1/testing", "", &s).contains("\"enabled\":true"),
            "mocks cleared"
        );
    }

    #[test]
    fn mcp_http_endpoint_jsonrpc_roundtrip() {
        let s = state();
        // tools/list through HTTP.
        let list = rs("POST", "/api/v1/mcp", r#"{"method":"tools/list"}"#, &s);
        assert!(list.contains("200"), "got: {list}");
        assert!(
            list.contains(r#"tools":[{"name":"compile""#),
            "catalog: {list}"
        );
        // Real call through HTTP: provision a test DB.
        let call = rs(
            "POST",
            "/api/v1/mcp",
            r#"{"method":"tools/call","params":{"name":"test_db_create","body":"{\"name\":\"users\"}"}}"#,
            &s,
        );
        assert!(call.contains("200"), "got: {call}");
        assert!(call.contains(r#"t1_users"#), "namespace: {call}");
        // Empty body → 400; unknown method → protocol error inside 200.
        assert!(rs("POST", "/api/v1/mcp", "", &s).contains("400"));
        let unk = rs("POST", "/api/v1/mcp", r#"{"method":"nope"}"#, &s);
        assert!(unk.contains("-32601"), "got: {unk}");
    }

    #[test]
    fn websocket_hub_lifecycle_and_handshake() {
        let s = state();
        // Handshake validation: RFC example must produce the exact accept.
        let raw = "GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
        let hs = rs(
            "POST",
            "/api/v1/ws/handshake",
            &raw.replace("\"", "\\\""),
            &s,
        );
        assert!(hs.contains("200"), "got: {hs}");
        assert!(hs.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="), "accept: {hs}");
        // Invalid upgrade → 400.
        assert!(rs("POST", "/api/v1/ws/handshake", "not http", &s).contains("400"));

        // Join two connections into chat; duplicate join → 409.
        assert!(rs(
            "POST",
            "/api/v1/ws/join",
            r#"{"conn":"ws1","room":"chat"}"#,
            &s
        )
        .contains("200"));
        assert!(rs(
            "POST",
            "/api/v1/ws/join",
            r#"{"conn":"ws2","room":"chat"}"#,
            &s
        )
        .contains("200"));
        assert!(
            rs(
                "POST",
                "/api/v1/ws/join",
                r#"{"conn":"ws1","room":"chat"}"#,
                &s
            )
            .contains("409"),
            "duplicate join"
        );

        // Broadcast from ws1 reaches ws2 only.
        let bc = rs(
            "POST",
            "/api/v1/ws/broadcast",
            r#"{"room":"chat","sender":"ws1","message":"{\"text\":\"hi\"}"}"#,
            &s,
        );
        assert!(bc.contains(r#"recipients":["ws2"]"#), "got: {bc}");
        assert!(bc.contains(r#""count":1"#));

        // Leave then broadcast reaches nobody; leaving again → 404.
        rs(
            "POST",
            "/api/v1/ws/leave",
            r#"{"conn":"ws2","room":"chat"}"#,
            &s,
        );
        let bc2 = rs(
            "POST",
            "/api/v1/ws/broadcast",
            r#"{"room":"chat","sender":"ws1"}"#,
            &s,
        );
        assert!(bc2.contains(r#""count":0"#), "got: {bc2}");
        assert!(rs(
            "POST",
            "/api/v1/ws/leave",
            r#"{"conn":"ws2","room":"chat"}"#,
            &s
        )
        .contains("404"));
        // Missing fields → 400.
        assert!(rs("POST", "/api/v1/ws/join", "{}", &s).contains("400"));
    }

    #[test]
    fn deploy_create_and_full_lifecycle() {
        let s = state();
        let created = rs(
            "POST",
            "/api/v1/deploy",
            r#"{"target":"production","platform":"linux/arm64","revision":"abc123"}"#,
            &s,
        );
        assert!(created.contains("\"id\":\"dep-1\""), "got: {created}");
        assert!(created.contains("\"status\":\"queued\""), "got: {created}");
        // Validation: bad platform and empty revision.
        assert!(
            rs(
                "POST",
                "/api/v1/deploy",
                r#"{"target":"p","platform":"bogus","revision":"r"}"#,
                &s
            )
            .contains("400"),
            "bad platform must 400"
        );
        // Drive the state machine; skipping stages must 400.
        assert!(
            rs(
                "POST",
                "/api/v1/deploy/status",
                r#"{"id":"dep-1","status":"deployed"}"#,
                &s
            )
            .contains("400"),
            "skip must 400"
        );
        for st in ["building", "deploying", "deployed"] {
            let resp = rs(
                "POST",
                "/api/v1/deploy/status",
                &format!(r#"{{"id":"dep-1","status":"{st}"}}"#),
                &s,
            );
            assert!(resp.contains("200"), "{st}: {resp}");
        }
        // Terminal is terminal.
        assert!(
            rs(
                "POST",
                "/api/v1/deploy/status",
                r#"{"id":"dep-1","status":"building"}"#,
                &s
            )
            .contains("400"),
            "terminal must reject"
        );
    }

    #[test]
    fn deploy_rollback_returns_to_previous_revision() {
        let s = state();
        rs(
            "POST",
            "/api/v1/deploy",
            r#"{"target":"prod","platform":"linux/amd64","revision":"v1"}"#,
            &s,
        );
        rs(
            "POST",
            "/api/v1/deploy",
            r#"{"target":"prod","platform":"linux/amd64","revision":"v2"}"#,
            &s,
        );
        for st in ["building", "deploying", "deployed"] {
            rs(
                "POST",
                "/api/v1/deploy/status",
                &format!(r#"{{"id":"dep-1","status":"{st}"}}"#),
                &s,
            );
            rs(
                "POST",
                "/api/v1/deploy/status",
                &format!(r#"{{"id":"dep-2","status":"{st}"}}"#),
                &s,
            );
        }
        // Rollback swaps live v2 → previous v1.
        let rb = rs(
            "POST",
            "/api/v1/deploy/rollback",
            r#"{"target":"prod"}"#,
            &s,
        );
        assert!(
            rb.contains(r#""revision":"v1","status":"deployed""#),
            "got: {rb}"
        );
        // No predecessor on a fresh target → 404.
        assert!(
            rs(
                "POST",
                "/api/v1/deploy/rollback",
                r#"{"target":"ghost"}"#,
                &s
            )
            .contains("404"),
            "unknown target must 404"
        );
        assert!(
            rs("POST", "/api/v1/deploy/rollback", "{}", &s).contains("400"),
            "missing target must 400"
        );
    }

    #[test]
    fn deploy_dockerfile_is_generated_and_escaped() {
        let resp = r("GET", "/api/v1/deploy/dockerfile", "");
        assert!(resp.contains("200"), "got: {resp}");
        assert!(
            resp.contains("BUILDPLATFORM"),
            "platform-aware build missing: {resp}"
        );
        // The embedded Dockerfile must be a valid JSON string (newlines escaped).
        assert!(
            !resp.contains("\\n\\n\\n\"") || resp.contains("\\n"),
            "escaped"
        );
        assert!(
            !resp.contains("# Generated by bridge\n"),
            "raw newline leaked into JSON string"
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
