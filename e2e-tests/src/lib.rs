//! End-to-end tests for Bridge Framework.
//!
//! These tests require a running daemon. They are marked `#[ignore]` so they
//! don't run in CI by default. To run them:
//!
//! ```bash
//! # Terminal 1: start daemon on test ports
//! BRIDGE_TCP_ADDR=127.0.0.1:17878 BRIDGE_HTTP_ADDR=127.0.0.1:18787 \
//!   BRIDGE_REDIS_ADDR=127.0.0.1:16399 cargo run -p daemon
//!
//! # Terminal 2: run e2e tests
//! cargo test -p e2e-tests -- --include-ignored
//! ```

#[cfg(test)]
mod tests {
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
            if TcpStream::connect(TCP_ADDR).is_ok() && TcpStream::connect(HTTP_ADDR).is_ok() {
                *guard = Some(DaemonGuard(child));
                thread::sleep(Duration::from_millis(500)); // let miniredis bind
                return true;
            }
        }
        false
    }

    // ── TCP helper: send one command, read one response line ──────────────────

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
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
        let body = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    fn docker_available() -> bool {
        Command::new("docker").arg("version").output()
            .map(|o| o.status.success()).unwrap_or(false)
    }

    // ── Redis helper: send raw RESP bytes, read one response line ─────────────

    fn redis_line(writer: &mut TcpStream, reader: &mut BufReader<TcpStream>, cmd: &[u8]) -> String {
        writer.write_all(cmd).expect("redis write");
        writer.flush().expect("redis flush");
        let mut line = String::new();
        reader.read_line(&mut line).expect("redis read");
        line.trim_end().to_string()
    }

    // ── TCP tests ─────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_ping() {
        assert!(ensure_daemon(), "daemon not available");
        assert_eq!(tcp_cmd("PING"), "PONG");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_version() {
        assert!(ensure_daemon(), "daemon not available");
        let r = tcp_cmd("VERSION");
        assert!(r.starts_with("DATA "), "got: {r}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_health() {
        assert!(ensure_daemon(), "daemon not available");
        let r = tcp_cmd("HEALTH");
        assert!(r.starts_with("DATA "), "got: {r}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_help() {
        assert!(ensure_daemon(), "daemon not available");
        let r = tcp_cmd("HELP");
        assert!(r.starts_with("DATA "), "got: {r}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_compile() {
        assert!(ensure_daemon(), "daemon not available");
        let r = tcp_cmd("COMPILE service%20hello%0Aendpoint%20ping%20GET%20/ping");
        assert!(r.starts_with("DATA "), "got: {r}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_mode_get_set() {
        assert!(ensure_daemon(), "daemon not available");
        let r = tcp_cmd("MODE GET");
        assert!(r.starts_with("MODE "), "got: {r}");
        let r2 = tcp_cmd("MODE SET lite");
        assert!(r2.starts_with("OK "), "got: {r2}");
        tcp_cmd("MODE SET full");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_db_lifecycle() {
        assert!(ensure_daemon(), "daemon not available");
        assert!(tcp_cmd("DB PUT e2e testkey hello%20world").starts_with("OK "));
        assert!(tcp_cmd("DB GET e2e testkey").starts_with("DATA "));
        assert!(tcp_cmd("DB DEL e2e testkey").starts_with("OK "));
        assert!(tcp_cmd("DB GET e2e testkey").starts_with("ERR "));
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_auth_lifecycle() {
        assert!(ensure_daemon(), "daemon not available");
        assert!(tcp_cmd("AUTH STATUS").starts_with("DATA "));
        assert!(tcp_cmd("AUTH SET bearer my-secret-token").starts_with("OK "));
        let r = tcp_cmd("AUTH STATUS");
        assert!(r.contains("true"), "expected configured=true, got: {r}");
        tcp_cmd("AUTH CLEAR");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_tcp_trace_lifecycle() {
        assert!(ensure_daemon(), "daemon not available");
        assert!(tcp_cmd("TRACE LIST").starts_with("DATA "));
        assert!(tcp_cmd("TRACE CLEAR").starts_with("OK "));
    }

    // ── HTTP tests ────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_http_health() {
        assert!(ensure_daemon(), "daemon not available");
        let (status, body) = http("GET", "/health", "");
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("ok") || body.contains("status"), "body: {body}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_http_api_health() {
        assert!(ensure_daemon(), "daemon not available");
        let (status, body) = http("GET", "/api/v1/health", "");
        assert_eq!(status, 200, "body: {body}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_http_mode() {
        assert!(ensure_daemon(), "daemon not available");
        let (status, body) = http("GET", "/mode", "");
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("mode"), "body: {body}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_http_compile() {
        assert!(ensure_daemon(), "daemon not available");
        let source = "service test\nendpoint health GET /health";
        let (status, body) = http("POST", "/compile", source);
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("test") || body.contains("BridgeClient"), "body: {body}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_http_db_status() {
        assert!(ensure_daemon(), "daemon not available");
        let (status, _body) = http("GET", "/db/status", "");
        assert_eq!(status, 200);
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_http_redis_status() {
        assert!(ensure_daemon(), "daemon not available");
        let (status, body) = http("GET", "/redis/status", "");
        assert_eq!(status, 200, "body: {body}");
        assert!(body.contains("addr") || body.contains("connection"), "body: {body}");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_http_cors_preflight() {
        assert!(ensure_daemon(), "daemon not available");
        let (status, _body) = http("OPTIONS", "/health", "");
        assert!(status == 200 || status == 204, "OPTIONS should succeed, got: {status}");
    }

    // ── Miniredis tests ───────────────────────────────────────────────────────

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_miniredis_set_get_del() {
        assert!(ensure_daemon(), "daemon not available");
        thread::sleep(Duration::from_millis(300));

        let stream = TcpStream::connect(REDIS_ADDR).expect("connect miniredis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;

        assert_eq!(redis_line(&mut writer, &mut reader, b"*1\r\n$4\r\nPING\r\n"), "+PONG");
        assert_eq!(redis_line(&mut writer, &mut reader,
            b"*3\r\n$3\r\nSET\r\n$7\r\ne2etest\r\n$5\r\nhello\r\n"), "+OK");
        assert_eq!(redis_line(&mut writer, &mut reader,
            b"*2\r\n$3\r\nGET\r\n$7\r\ne2etest\r\n"), "$5");
        assert_eq!(redis_line(&mut writer, &mut reader, b""), "hello");
        assert_eq!(redis_line(&mut writer, &mut reader,
            b"*2\r\n$3\r\nDEL\r\n$7\r\ne2etest\r\n"), ":1");
        assert_eq!(redis_line(&mut writer, &mut reader,
            b"*2\r\n$3\r\nGET\r\n$7\r\ne2etest\r\n"), "$-1");
    }

    #[test]
    #[ignore = "requires running daemon"]
    fn e2e_miniredis_incr_expire() {
        assert!(ensure_daemon(), "daemon not available");
        thread::sleep(Duration::from_millis(100));

        let stream = TcpStream::connect(REDIS_ADDR).expect("connect miniredis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;

        assert_eq!(redis_line(&mut writer, &mut reader,
            b"*2\r\n$4\r\nINCR\r\n$8\r\ncounter1\r\n"), ":1");
        assert_eq!(redis_line(&mut writer, &mut reader,
            b"*2\r\n$4\r\nINCR\r\n$8\r\ncounter1\r\n"), ":2");
        assert_eq!(redis_line(&mut writer, &mut reader,
            b"*3\r\n$6\r\nEXPIRE\r\n$8\r\ncounter1\r\n$2\r\n60\r\n"), ":1");
        let ttl = redis_line(&mut writer, &mut reader,
            b"*2\r\n$3\r\nTTL\r\n$8\r\ncounter1\r\n");
        assert!(ttl.starts_with(':'), "expected integer TTL, got: {ttl}");
        let n: i64 = ttl[1..].parse().unwrap_or(-99);
        assert!(n > 0, "TTL should be > 0, got: {n}");
    }

    // ── Docker Postgres tests ─────────────────────────────────────────────────

    #[test]
    #[ignore = "requires running daemon and Docker"]
    fn e2e_docker_postgres_lifecycle() {
        assert!(ensure_daemon(), "daemon not available");
        if !docker_available() { eprintln!("SKIP: Docker not available"); return; }

        let (status, body) = http("POST", "/db/create", "e2e_test");
        eprintln!("db create: {status} {body}");
        thread::sleep(Duration::from_secs(3));

        let (status, body) = http("GET", "/db/status", "");
        assert_eq!(status, 200, "body: {body}");
        eprintln!("db status: {body}");

        let (status, _) = http("DELETE", "/db/destroy", "e2e_test");
        assert_eq!(status, 200);
    }
}
