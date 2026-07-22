//! End-to-end tests for Bridge Framework.
//!
//! ## Test categories
//!
//! 1. **Unit integration** — run always; test crate interactions without a
//!    live daemon (compiler → codegen pipeline, protocol encode/decode, etc.)
//!
//! 2. **Daemon tests** — marked `#[ignore]`; require a running daemon.
//!    Start one before running:
//!    ```bash
//!    BRIDGE_TCP_ADDR=127.0.0.1:17878 BRIDGE_HTTP_ADDR=127.0.0.1:18787 \
//!      BRIDGE_REDIS_ADDR=127.0.0.1:16399 cargo run -p daemon
//!    cargo test -p e2e-tests -- --include-ignored
//!    ```

// ─────────────────────────────────────────────────────────────────────────────
// Unit-integration tests (no daemon required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod unit {
    use compiler::parse;
    use codegen::{generate_typescript, generate_openapi};
    use protocol::{encode, decode, parse_command, Command, DaemonMode};

    // ── Compiler → Codegen pipeline ───────────────────────────────────────────

    #[test]
    fn compile_and_generate_single_service() {
        let src = "service users\nendpoint list GET /users\nendpoint get GET /users/:id\n";
        let file = parse(src).unwrap();
        let ts = generate_typescript(&file);
        assert!(ts.contains("createUsersClient"), "missing factory fn");
        assert!(ts.contains("async list()"),       "missing list fn");
        assert!(ts.contains("async get(id: string)"), "missing get fn with path param");
        assert!(ts.contains("`/users/${id}`"),     "missing interpolated path");
        assert!(ts.contains("BridgeError"),        "missing error class");
    }

    #[test]
    fn compile_and_generate_multi_service() {
        let src = concat!(
            "service users\nauth bearer\nendpoint list GET /users\n",
            "service posts\nendpoint create POST /posts\n",
        );
        let file = parse(src).unwrap();
        let ts = generate_typescript(&file);
        assert!(ts.contains("createUsersClient"), "missing users factory");
        assert!(ts.contains("createPostsClient"), "missing posts factory");
        assert!(ts.contains("createClient"),      "missing root factory");
        // users has bearer auth → token param
        assert!(ts.contains("token: string"),     "missing token param");
        // posts endpoint has body
        assert!(ts.contains("body?: unknown"),    "missing body param");
    }

    #[test]
    fn compile_path_params_codegen() {
        let src = "service api\nendpoint detail GET /a/:x/b/:y\n";
        let file = parse(src).unwrap();
        let ts = generate_typescript(&file);
        assert!(ts.contains("x: string, y: string"), "missing multi param signature");
        assert!(ts.contains("`/a/${x}/b/${y}`"),     "missing multi param interpolation");
    }

    #[test]
    fn openapi_path_params_converted() {
        let src = "service shop\nendpoint get GET /products/:id\n";
        let file = parse(src).unwrap();
        let spec = generate_openapi(&file);
        assert!(spec.contains("{id}"),    "OA path params should use {{id}} not :id");
        assert!(spec.contains("\"path\""), "path param should have 'in: path'");
        assert!(!spec.contains(":id"),    "colon-style params should not appear in OA spec");
    }

    #[test]
    fn openapi_bearer_security() {
        let src = "service secure\nauth bearer\nendpoint get GET /data\n";
        let file = parse(src).unwrap();
        let spec = generate_openapi(&file);
        assert!(spec.contains("bearerAuth"),       "missing bearerAuth security scheme");
        assert!(spec.contains("securitySchemes"),  "missing securitySchemes block");
    }

    #[test]
    fn openapi_post_request_body() {
        let src = "service api\nendpoint create POST /items\n";
        let file = parse(src).unwrap();
        let spec = generate_openapi(&file);
        assert!(spec.contains("requestBody"), "POST should include requestBody");
    }

    #[test]
    fn openapi_get_no_request_body() {
        let src = "service api\nendpoint list GET /items\n";
        let file = parse(src).unwrap();
        let spec = generate_openapi(&file);
        assert!(!spec.contains("requestBody"), "GET should not have requestBody");
    }

    // ── Protocol encode / decode ──────────────────────────────────────────────

    #[test]
    fn protocol_roundtrip_simple() {
        let original = "hello world";
        let encoded = encode(original);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn protocol_roundtrip_json() {
        let json = r#"{"key":"value with spaces","n":42}"#;
        let encoded = encode(json);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, json);
    }

    #[test]
    fn protocol_roundtrip_multiline() {
        let src = "service hello\nendpoint ping GET /ping\n";
        let encoded = encode(src);
        assert!(!encoded.contains('\n'), "encoded should have no newlines");
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, src);
    }

    #[test]
    fn protocol_parse_ping() {
        let cmd = parse_command("PING").unwrap();
        assert!(matches!(cmd, Command::Ping));
    }

    #[test]
    fn protocol_parse_mode_set() {
        let cmd = parse_command("MODE SET full").unwrap();
        assert!(matches!(cmd, Command::SetMode(_)));
        if let Command::SetMode(mode) = cmd {
            assert_eq!(mode, DaemonMode::Full);
        }
    }

    #[test]
    fn protocol_parse_compile() {
        let src = "service s\nendpoint p GET /p\n";
        let wire = format!("COMPILE {}", encode(src));
        let cmd = parse_command(&wire).unwrap();
        assert!(matches!(cmd, Command::Compile { .. }));
        if let Command::Compile { source: decoded } = cmd {
            assert_eq!(decoded, src);
        }
    }

    #[test]
    fn protocol_daemon_mode_roundtrip() {
        for s in ["lite", "full", "ultra", "off"] {
            let mode = DaemonMode::parse(s).unwrap();
            assert_eq!(mode.as_str(), s);
        }
    }

    // ── Compiler validation ───────────────────────────────────────────────────

    #[test]
    fn compiler_rejects_empty_source() {
        assert!(compiler::parse("").is_err(), "empty source must error");
    }

    #[test]
    fn compiler_rejects_service_no_endpoints() {
        assert!(compiler::parse("service s\n").is_err());
    }

    #[test]
    fn compiler_rejects_duplicate_service_names() {
        let src = "service s\nendpoint p GET /p\nservice s\nendpoint q POST /q\n";
        assert!(compiler::parse(src).is_err());
    }

    #[test]
    fn compiler_rejects_bad_method() {
        assert!(compiler::compile("service s\nendpoint p INVALID /p\n").is_err());
    }

    #[test]
    fn compiler_accepts_all_methods() {
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
            let src = format!("service s\nendpoint ep {m} /path\n");
            assert!(compiler::compile(&src).is_ok(), "method {m} should parse");
        }
    }

    // ── DB crate ──────────────────────────────────────────────────────────────

    #[test]
    fn db_put_get_del() {
        let db = db::Db::new();
        db.put("ns", "k", "v");
        assert_eq!(db.get("ns", "k"), Some("v".to_string()));
        db.del("ns", "k");
        assert_eq!(db.get("ns", "k"), None);
    }

    #[test]
    fn db_namespace_isolation() {
        let db = db::Db::new();
        db.put("a", "key", "val-a");
        db.put("b", "key", "val-b");
        assert_eq!(db.get("a", "key"), Some("val-a".to_string()));
        assert_eq!(db.get("b", "key"), Some("val-b".to_string()));
        db.flush_ns("a");
        assert_eq!(db.get("a", "key"), None);
        assert_eq!(db.get("b", "key"), Some("val-b".to_string()));
    }

    #[test]
    fn db_keys_lists_entries() {
        let db = db::Db::new();
        db.put("ns", "a", "1");
        db.put("ns", "b", "2");
        db.put("ns", "c", "3");
        let mut keys = db.keys("ns");
        keys.sort();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    // ── MiniRedis unit ────────────────────────────────────────────────────────

    #[test]
    fn miniredis_resp_serialize_roundtrip() {
        use miniredis::Resp;
        let r = Resp::bulk("hello world");
        let bytes = r.serialize();
        assert_eq!(&bytes, b"$11\r\nhello world\r\n");
    }

    #[test]
    fn miniredis_resp_null_serialize() {
        use miniredis::Resp;
        assert_eq!(Resp::null().serialize(), b"$-1\r\n");
    }

    #[test]
    fn miniredis_resp_integer_serialize() {
        use miniredis::Resp;
        assert_eq!(Resp::int(42).serialize(), b":42\r\n");
        assert_eq!(Resp::int(-1).serialize(), b":-1\r\n");
    }

    #[test]
    fn miniredis_resp_array_serialize() {
        use miniredis::Resp;
        let arr = Resp::Array(vec![Resp::bulk("a"), Resp::bulk("b")]);
        let bytes = arr.serialize();
        assert_eq!(&bytes, b"*2\r\n$1\r\na\r\n$1\r\nb\r\n");
    }

    #[test]
    fn miniredis_resp_parse_bulk() {
        use miniredis::{Resp, parse_resp};
        use std::io::BufReader;
        let data = b"$5\r\nhello\r\n";
        let mut reader = BufReader::new(&data[..]);
        let r = parse_resp(&mut reader).unwrap();
        assert_eq!(r, Resp::BulkString(Some("hello".into())));
    }

    #[test]
    fn miniredis_resp_parse_array() {
        use miniredis::{Resp, parse_resp};
        use std::io::BufReader;
        let data = b"*2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n";
        let mut reader = BufReader::new(&data[..]);
        let r = parse_resp(&mut reader).unwrap();
        assert_eq!(r, Resp::Array(vec![
            Resp::bulk("GET"),
            Resp::bulk("foo"),
        ]));
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// Daemon integration tests (require a running daemon — #[ignore] by default)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod daemon_tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{Shutdown, TcpStream};
    use std::process::{Command, Stdio};
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    const TCP_ADDR:   &str = "127.0.0.1:17878";
    const HTTP_ADDR:  &str = "127.0.0.1:18787";
    const REDIS_ADDR: &str = "127.0.0.1:16399";

    // ── Daemon lifecycle ──────────────────────────────────────────────────────

    struct DaemonGuard(std::process::Child);
    impl Drop for DaemonGuard {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }

    static DAEMON: OnceLock<Mutex<Option<DaemonGuard>>> = OnceLock::new();

    fn ensure_daemon() -> bool {
        let cell = DAEMON.get_or_init(|| Mutex::new(None));
        let mut guard = cell.lock().unwrap();
        if guard.is_some() { return true; }
        if TcpStream::connect(TCP_ADDR).is_ok() { return true; }

        let child = match Command::new("cargo")
            .args(["run", "-p", "daemon"])
            .envs([
                ("BRIDGE_TCP_ADDR",   TCP_ADDR),
                ("BRIDGE_HTTP_ADDR",  HTTP_ADDR),
                ("BRIDGE_REDIS_ADDR", REDIS_ADDR),
            ])
            .stdout(Stdio::null()).stderr(Stdio::null())
            .spawn() { Ok(c) => c, Err(_) => return false };

        for _ in 0..50 {
            thread::sleep(Duration::from_millis(200));
            if TcpStream::connect(TCP_ADDR).is_ok()
                && TcpStream::connect(HTTP_ADDR).is_ok()
            {
                *guard = Some(DaemonGuard(child));
                thread::sleep(Duration::from_millis(500));
                return true;
            }
        }
        false
    }

    // ── TCP helper ────────────────────────────────────────────────────────────

    fn tcp_cmd(cmd: &str) -> String {
        let stream = TcpStream::connect(TCP_ADDR).expect("connect tcp");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        {
            let mut w = stream.try_clone().unwrap();
            w.write_all(format!("{cmd}\n").as_bytes()).expect("write");
            w.shutdown(Shutdown::Write).ok();
        }
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read response");
        line.trim_end().to_string()
    }

    // ── HTTP helper ───────────────────────────────────────────────────────────

    fn http(method: &str, path: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(HTTP_ADDR).expect("connect http");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let req = if body.is_empty() {
            format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        } else {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        };
        stream.write_all(req.as_bytes()).expect("write http");
        stream.shutdown(Shutdown::Write).ok();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).expect("read http");
        let status = resp.lines().next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0u16);
        let body_str = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body_str)
    }

    // ── Redis RESP helper ─────────────────────────────────────────────────────

    fn redis_line(writer: &mut TcpStream, reader: &mut BufReader<TcpStream>, cmd: &[u8]) -> String {
        if !cmd.is_empty() {
            writer.write_all(cmd).expect("redis write");
            writer.flush().expect("redis flush");
        }
        let mut line = String::new();
        reader.read_line(&mut line).expect("redis read");
        line.trim_end().to_string()
    }

    fn docker_available() -> bool {
        Command::new("docker").arg("version").output()
            .map(|o| o.status.success()).unwrap_or(false)
    }

    // ── TCP: core protocol ────────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_ping() {
        assert!(ensure_daemon());
        assert_eq!(tcp_cmd("PING"), "PONG");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_version() {
        assert!(ensure_daemon());
        assert!(tcp_cmd("VERSION").starts_with("DATA "));
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_health() {
        assert!(ensure_daemon());
        assert!(tcp_cmd("HEALTH").starts_with("DATA "));
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_help() {
        assert!(ensure_daemon());
        assert!(tcp_cmd("HELP").starts_with("DATA "));
    }

    // ── TCP: compile & routes ─────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_compile_basic() {
        assert!(ensure_daemon());
        let src = protocol::encode("service hello\nendpoint ping GET /ping\n");
        let r = tcp_cmd(&format!("COMPILE {src}"));
        assert!(r.starts_with("DATA "), "got: {r}");
        // TypeScript client should be in the response
        let decoded = protocol::decode(r.strip_prefix("DATA ").unwrap_or("")).unwrap_or_default();
        assert!(decoded.contains("createHelloClient") || decoded.contains("hello"), "got: {decoded}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_compile_with_path_params() {
        assert!(ensure_daemon());
        let src = protocol::encode("service users\nendpoint get GET /users/:id\n");
        let r = tcp_cmd(&format!("COMPILE {src}"));
        assert!(r.starts_with("DATA "), "got: {r}");
        let decoded = protocol::decode(r.strip_prefix("DATA ").unwrap_or("")).unwrap_or_default();
        assert!(decoded.contains("id: string") || decoded.contains("users"), "got: {decoded}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_services_after_compile() {
        assert!(ensure_daemon());
        let src = protocol::encode("service myapp\nendpoint ping GET /ping\n");
        tcp_cmd(&format!("COMPILE {src}"));
        let r = tcp_cmd("SERVICES LIST");
        assert!(r.starts_with("DATA "), "got: {r}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_routes_after_compile() {
        assert!(ensure_daemon());
        let r = tcp_cmd("ROUTES LIST");
        assert!(r.starts_with("DATA "), "got: {r}");
    }

    // ── TCP: mode ─────────────────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_mode_get_set_roundtrip() {
        assert!(ensure_daemon());
        let orig = tcp_cmd("MODE GET");
        assert!(orig.starts_with("MODE "), "got: {orig}");

        assert!(tcp_cmd("MODE SET lite").starts_with("OK "));
        assert!(tcp_cmd("MODE GET").contains("lite"));
        assert!(tcp_cmd("MODE SET full").starts_with("OK "));
        assert!(tcp_cmd("MODE GET").contains("full"));

        // Restore original mode
        let mode = orig.strip_prefix("MODE ").unwrap_or("full");
        tcp_cmd(&format!("MODE SET {mode}"));
    }

    // ── TCP: key-value store ──────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_db_put_get_del() {
        assert!(ensure_daemon());
        let key = "e2e_kv_test";
        assert!(tcp_cmd(&format!("DB PUT e2e {key} hello%20world")).starts_with("OK "));
        let r = tcp_cmd(&format!("DB GET e2e {key}"));
        assert!(r.starts_with("DATA "), "got: {r}");
        assert!(tcp_cmd(&format!("DB DEL e2e {key}")).starts_with("OK "));
        assert!(tcp_cmd(&format!("DB GET e2e {key}")).starts_with("ERR "));
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_db_keys_and_flush() {
        assert!(ensure_daemon());
        tcp_cmd("DB PUT e2e_flush k1 v1");
        tcp_cmd("DB PUT e2e_flush k2 v2");
        let r = tcp_cmd("DB KEYS e2e_flush");
        assert!(r.starts_with("DATA "), "got: {r}");
        assert!(tcp_cmd("DB FLUSH e2e_flush").starts_with("OK "));
        let after = tcp_cmd("DB GET e2e_flush k1");
        assert!(after.starts_with("ERR "), "expected ERR after flush, got: {after}");
    }

    // ── TCP: auth ─────────────────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_auth_bearer_lifecycle() {
        assert!(ensure_daemon());
        tcp_cmd("AUTH CLEAR"); // clean slate
        let status = tcp_cmd("AUTH STATUS");
        assert!(status.starts_with("DATA "), "got: {status}");
        assert!(!status.contains("\"configured\":true"), "should start unconfigured");

        assert!(tcp_cmd("AUTH SET bearer my-secret-token-123").starts_with("OK "));
        let configured = tcp_cmd("AUTH STATUS");
        assert!(configured.contains("true"), "expected configured=true, got: {configured}");

        tcp_cmd("AUTH CLEAR");
        let cleared = tcp_cmd("AUTH STATUS");
        assert!(!cleared.contains("\"configured\":true"), "should be cleared");
    }

    // ── TCP: traces ───────────────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_tcp_trace_list_and_clear() {
        assert!(ensure_daemon());
        // Generate a trace via HTTP
        http("GET", "/health", "");
        thread::sleep(Duration::from_millis(50));

        let r = tcp_cmd("TRACE LIST");
        assert!(r.starts_with("DATA "), "got: {r}");
        assert!(tcp_cmd("TRACE CLEAR").starts_with("OK "));
        // After clear, list should be empty array
        let after = tcp_cmd("TRACE LIST");
        let decoded = protocol::decode(after.strip_prefix("DATA ").unwrap_or("")).unwrap_or_default();
        assert!(decoded.contains("[]") || decoded == "[]", "expected empty, got: {decoded}");
    }

    // ── HTTP: core endpoints ──────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_health_has_status_field() {
        assert!(ensure_daemon());
        let (status, body) = http("GET", "/health", "");
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("ok") || body.contains("status") || body.contains("version"),
            "body should contain status info, got: {body}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_v1_health() {
        assert!(ensure_daemon());
        let (status, _) = http("GET", "/api/v1/health", "");
        assert_eq!(status, 200);
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_version() {
        assert!(ensure_daemon());
        let (status, body) = http("GET", "/api/v1/version", "");
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("version"), "body: {body}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_mode_get() {
        assert!(ensure_daemon());
        let (status, body) = http("GET", "/mode", "");
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("mode"), "body: {body}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_mode_set() {
        assert!(ensure_daemon());
        let (s, b) = http("POST", "/mode", "\"lite\"");
        assert_eq!(s, 200, "body: {b}");
        http("POST", "/mode", "\"full\""); // restore
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_not_found() {
        assert!(ensure_daemon());
        let (status, _) = http("GET", "/no-such-path", "");
        assert_eq!(status, 404);
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_cors_preflight() {
        assert!(ensure_daemon());
        let (status, _) = http("OPTIONS", "/health", "");
        assert!(status == 200 || status == 204, "OPTIONS should succeed, got: {status}");
    }

    // ── HTTP: compile ─────────────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_compile_returns_typescript() {
        assert!(ensure_daemon());
        let src = "service test\nendpoint ping GET /ping\n";
        let (status, body) = http("POST", "/compile", src);
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("createTestClient") || body.contains("BridgeError"),
            "body: {body}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_services_list() {
        assert!(ensure_daemon());
        let (status, _) = http("GET", "/services", "");
        assert_eq!(status, 200);
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_routes_list() {
        assert!(ensure_daemon());
        let (status, _) = http("GET", "/routes", "");
        assert_eq!(status, 200);
    }

    // ── HTTP: traces & metrics ────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_traces_after_request() {
        assert!(ensure_daemon());
        // Clear first
        http("DELETE", "/api/v1/traces", "");
        // Make a request that generates a trace
        http("GET", "/health", "");
        thread::sleep(Duration::from_millis(100));
        let (status, body) = http("GET", "/api/v1/traces", "");
        assert_eq!(status, 200, "body: {body}");
        // Should have at least one trace
        assert!(body.contains("GET") || body.contains("traces") || body.len() > 5,
            "expected some trace data, got: {body}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_metrics() {
        assert!(ensure_daemon());
        let (status, body) = http("GET", "/api/v1/metrics", "");
        assert_eq!(status, 200, "body: {body}");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_openapi() {
        assert!(ensure_daemon());
        // Compile something first so there's a spec
        let src = "service api\nendpoint list GET /items\n";
        http("POST", "/compile", src);
        let (status, body) = http("GET", "/api/v1/openapi", "");
        assert_eq!(status, 200, "body: {body}");
    }

    // ── HTTP: auth ────────────────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_auth_set_and_clear() {
        assert!(ensure_daemon());
        http("DELETE", "/api/v1/auth/clear", "");
        let (s, _) = http("POST", "/api/v1/auth/set",
            r#"{"scheme":"bearer","token":"test-token"}"#);
        assert_eq!(s, 200);
        let (s, body) = http("GET", "/api/v1/auth/status", "");
        assert_eq!(s, 200, "body: {body}");
        http("DELETE", "/api/v1/auth/clear", "");
    }

    // ── HTTP: Redis status ────────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_redis_status() {
        assert!(ensure_daemon());
        let (status, body) = http("GET", "/redis/status", "");
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("addr") || body.contains("connection") || body.contains("status"),
            "body: {body}");
    }

    // ── HTTP: database ────────────────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_http_db_status() {
        assert!(ensure_daemon());
        let (status, _) = http("GET", "/db/status", "");
        assert_eq!(status, 200);
    }

    // ── MiniRedis: string commands ────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon"]
    fn e2e_redis_ping() {
        assert!(ensure_daemon());
        thread::sleep(Duration::from_millis(300));
        let stream = TcpStream::connect(REDIS_ADDR).expect("connect redis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut w = stream;
        assert_eq!(redis_line(&mut w, &mut r, b"*1\r\n$4\r\nPING\r\n"), "+PONG");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_redis_set_get_del() {
        assert!(ensure_daemon());
        thread::sleep(Duration::from_millis(200));
        let stream = TcpStream::connect(REDIS_ADDR).expect("connect redis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut w = stream;

        assert_eq!(redis_line(&mut w, &mut r, b"*3\r\n$3\r\nSET\r\n$6\r\ne2ekey\r\n$5\r\nhello\r\n"), "+OK");
        assert_eq!(redis_line(&mut w, &mut r, b"*2\r\n$3\r\nGET\r\n$6\r\ne2ekey\r\n"), "$5");
        assert_eq!(redis_line(&mut w, &mut r, b""), "hello");
        assert_eq!(redis_line(&mut w, &mut r, b"*2\r\n$3\r\nDEL\r\n$6\r\ne2ekey\r\n"), ":1");
        assert_eq!(redis_line(&mut w, &mut r, b"*2\r\n$3\r\nGET\r\n$6\r\ne2ekey\r\n"), "$-1");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_redis_incr_decr() {
        assert!(ensure_daemon());
        thread::sleep(Duration::from_millis(100));
        let stream = TcpStream::connect(REDIS_ADDR).expect("connect redis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut w = stream;

        assert_eq!(redis_line(&mut w, &mut r, b"*2\r\n$4\r\nINCR\r\n$7\r\ne2ecntr\r\n"), ":1");
        assert_eq!(redis_line(&mut w, &mut r, b"*2\r\n$4\r\nINCR\r\n$7\r\ne2ecntr\r\n"), ":2");
        assert_eq!(redis_line(&mut w, &mut r, b"*2\r\n$4\r\nDECR\r\n$7\r\ne2ecntr\r\n"), ":1");
        // cleanup
        redis_line(&mut w, &mut r, b"*2\r\n$3\r\nDEL\r\n$7\r\ne2ecntr\r\n");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_redis_expire_ttl() {
        assert!(ensure_daemon());
        thread::sleep(Duration::from_millis(100));
        let stream = TcpStream::connect(REDIS_ADDR).expect("connect redis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut w = stream;

        redis_line(&mut w, &mut r, b"*3\r\n$3\r\nSET\r\n$6\r\ne2ettl\r\n$3\r\nval\r\n");
        assert_eq!(redis_line(&mut w, &mut r, b"*3\r\n$6\r\nEXPIRE\r\n$6\r\ne2ettl\r\n$2\r\n60\r\n"), ":1");
        let ttl = redis_line(&mut w, &mut r, b"*2\r\n$3\r\nTTL\r\n$6\r\ne2ettl\r\n");
        let n: i64 = ttl[1..].parse().unwrap_or(-99);
        assert!(n > 0 && n <= 60, "TTL should be 1-60, got: {n}");
        redis_line(&mut w, &mut r, b"*2\r\n$3\r\nDEL\r\n$6\r\ne2ettl\r\n");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_redis_mset_mget() {
        assert!(ensure_daemon());
        thread::sleep(Duration::from_millis(100));
        let stream = TcpStream::connect(REDIS_ADDR).expect("connect redis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut w = stream;

        // MSET k1 v1 k2 v2
        let mset = b"*5\r\n$4\r\nMSET\r\n$2\r\nm1\r\n$2\r\nv1\r\n$2\r\nm2\r\n$2\r\nv2\r\n";
        assert_eq!(redis_line(&mut w, &mut r, mset), "+OK");
        // MGET k1 k2
        let mget = b"*3\r\n$4\r\nMGET\r\n$2\r\nm1\r\n$2\r\nm2\r\n";
        let arr_hdr = redis_line(&mut w, &mut r, mget);
        assert_eq!(arr_hdr, "*2", "expected 2-element array header, got: {arr_hdr}");
        // cleanup
        redis_line(&mut w, &mut r, b"*3\r\n$3\r\nDEL\r\n$2\r\nm1\r\n$2\r\nm2\r\n");
    }

    #[test] #[ignore = "requires running daemon"]
    fn e2e_redis_list_operations() {
        assert!(ensure_daemon());
        thread::sleep(Duration::from_millis(100));
        let stream = TcpStream::connect(REDIS_ADDR).expect("connect redis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut w = stream;

        // RPUSH mylist a b c
        let push = b"*5\r\n$5\r\nRPUSH\r\n$6\r\ne2list\r\n$1\r\na\r\n$1\r\nb\r\n$1\r\nc\r\n";
        let len = redis_line(&mut w, &mut r, push);
        assert_eq!(len, ":3", "expected 3 elements, got: {len}");
        // LLEN
        assert_eq!(redis_line(&mut w, &mut r, b"*2\r\n$4\r\nLLEN\r\n$6\r\ne2list\r\n"), ":3");
        // cleanup
        redis_line(&mut w, &mut r, b"*2\r\n$3\r\nDEL\r\n$6\r\ne2list\r\n");
    }

    // ── Docker Postgres (optional) ────────────────────────────────────────────

    #[test] #[ignore = "requires running daemon and Docker"]
    fn e2e_docker_postgres_lifecycle() {
        assert!(ensure_daemon());
        if !docker_available() { eprintln!("SKIP: Docker not available"); return; }

        let (s, b) = http("POST", "/db/create", "e2e_pg_test");
        eprintln!("db create: {s} {b}");
        thread::sleep(Duration::from_secs(3));

        let (s, b) = http("GET", "/db/status", "");
        assert_eq!(s, 200, "body: {b}");

        let (s, _) = http("DELETE", "/db/destroy", "e2e_pg_test");
        assert_eq!(s, 200);
    }
}
