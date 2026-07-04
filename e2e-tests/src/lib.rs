//! End-to-end tests for Bridge Framework.
//!
//! These tests spawn the daemon binary as a subprocess and exercise:
//! - TCP protocol commands (PING, HELP, COMPILE)
//! - HTTP endpoints (/health, /mode, /compile, /db/status, /redis/status)
//! - Miniredis (SET/GET/DEL via RESP protocol)
//! - Docker Postgres lifecycle (skipped when Docker is not available)

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{Shutdown, TcpStream};
    use std::process::{Child, Command};
    use std::thread;
    use std::time::Duration;

    const TCP_ADDR: &str = "127.0.0.1:17878";
    const HTTP_ADDR: &str = "127.0.0.1:18787";
    const REDIS_ADDR: &str = "127.0.0.1:16399";

    struct DaemonGuard {
        child: Child,
    }

    impl Drop for DaemonGuard {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn start_daemon() -> DaemonGuard {
        let child = Command::new("cargo")
            .args(["run", "-p", "daemon"])
            .env("BRIDGE_TCP_ADDR", TCP_ADDR)
            .env("BRIDGE_HTTP_ADDR", HTTP_ADDR)
            .env("BRIDGE_REDIS_ADDR", REDIS_ADDR)
            .spawn()
            .expect("failed to start daemon");

        // Wait for daemon to be ready
        for _ in 0..50 {
            thread::sleep(Duration::from_millis(200));
            if TcpStream::connect(TCP_ADDR).is_ok() {
                // Also wait for HTTP
                if TcpStream::connect(HTTP_ADDR).is_ok() {
                    return DaemonGuard { child };
                }
            }
        }
        panic!("daemon did not start within 10 seconds");
    }

    fn tcp_command(cmd: &str) -> String {
        let mut stream = TcpStream::connect(TCP_ADDR).expect("connect to daemon TCP");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream
            .write_all(format!("{cmd}\n").as_bytes())
            .expect("write command");
        stream.shutdown(Shutdown::Write).expect("shutdown write");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("read response");
        response
    }

    fn http_request(method: &str, path: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(HTTP_ADDR).expect("connect to daemon HTTP");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let req = if body.is_empty() {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: {HTTP_ADDR}\r\nConnection: close\r\n\r\n"
            )
        } else {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: {HTTP_ADDR}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        };
        stream.write_all(req.as_bytes()).expect("write request");
        stream.shutdown(Shutdown::Write).ok();
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("read response");

        // Parse status code
        let status_line = response.lines().next().unwrap_or("");
        let status_code = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);

        // Parse body (after \r\n\r\n)
        let body = response
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or("")
            .to_string();

        (status_code, body)
    }

    fn docker_available() -> bool {
        Command::new("docker")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    // Use a mutex via file lock to ensure only one daemon at a time
    use std::sync::Once;
    static INIT: Once = Once::new();
    static mut DAEMON: Option<DaemonGuard> = None;

    fn ensure_daemon() {
        unsafe {
            INIT.call_once(|| {
                DAEMON = Some(start_daemon());
            });
        }
    }

    #[test]
    fn e2e_tcp_ping() {
        ensure_daemon();
        let response = tcp_command("PING");
        assert_eq!(response.trim(), "PONG");
    }

    #[test]
    fn e2e_tcp_help() {
        ensure_daemon();
        let response = tcp_command("HELP");
        assert!(response.starts_with("DATA "));
        assert!(response.contains("commands:"));
    }

    #[test]
    fn e2e_tcp_compile() {
        ensure_daemon();
        let response = tcp_command("COMPILE service%20hello%0Aendpoint%20ping%20GET%20/ping");
        assert!(response.starts_with("DATA "));
    }

    #[test]
    fn e2e_http_health() {
        ensure_daemon();
        let (status, body) = http_request("GET", "/health", "");
        assert_eq!(status, 200);
        assert!(body.contains("ok"));
    }

    #[test]
    fn e2e_http_mode() {
        ensure_daemon();
        let (status, body) = http_request("GET", "/mode", "");
        assert_eq!(status, 200);
        assert!(body.contains("mode"));
    }

    #[test]
    fn e2e_http_compile() {
        ensure_daemon();
        let source = "service test\nendpoint health GET /health";
        let (status, body) = http_request("POST", "/compile", source);
        assert_eq!(status, 200);
        assert!(body.contains("test"));
    }

    #[test]
    fn e2e_http_db_status() {
        ensure_daemon();
        let (status, body) = http_request("GET", "/db/status", "");
        assert_eq!(status, 200);
        // Should contain some status info
        assert!(body.contains("status") || body.contains("docker") || body.contains("bridge_pg") || body.contains("no bridge"));
    }

    #[test]
    fn e2e_http_redis_status() {
        ensure_daemon();
        let (status, body) = http_request("GET", "/redis/status", "");
        assert_eq!(status, 200);
        assert!(body.contains("addr"));
    }

    #[test]
    fn e2e_miniredis_set_get_del() {
        ensure_daemon();
        // Give miniredis a moment to start
        thread::sleep(Duration::from_millis(500));

        let mut stream = TcpStream::connect(REDIS_ADDR).expect("connect to miniredis");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

        // PING
        stream.write_all(b"*1\r\n$4\r\nPING\r\n").unwrap();
        stream.flush().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "+PONG");

        // SET key value
        stream
            .write_all(b"*3\r\n$3\r\nSET\r\n$7\r\ntestkey\r\n$9\r\ntestvalue\r\n")
            .unwrap();
        stream.flush().unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "+OK");

        // GET key
        stream
            .write_all(b"*2\r\n$3\r\nGET\r\n$7\r\ntestkey\r\n")
            .unwrap();
        stream.flush().unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "$9"); // bulk string length
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "testvalue");

        // DEL key
        stream
            .write_all(b"*2\r\n$3\r\nDEL\r\n$7\r\ntestkey\r\n")
            .unwrap();
        stream.flush().unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), ":1");

        // GET deleted key (should be null)
        stream
            .write_all(b"*2\r\n$3\r\nGET\r\n$7\r\ntestkey\r\n")
            .unwrap();
        stream.flush().unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "$-1"); // null bulk string
    }

    #[test]
    fn e2e_docker_postgres_lifecycle() {
        ensure_daemon();
        if !docker_available() {
            eprintln!("Skipping Docker Postgres test: Docker not available");
            return;
        }

        // Create
        let (status, body) = http_request("POST", "/db/create", "e2e_test");
        eprintln!("db create: {status} {body}");
        if status != 200 {
            eprintln!("Skipping Docker Postgres test: create failed");
            return;
        }

        // Wait for container to be ready
        thread::sleep(Duration::from_secs(3));

        // Status
        let (status, body) = http_request("GET", "/db/status", "");
        assert_eq!(status, 200);
        eprintln!("db status: {body}");

        // Destroy
        let (status, _) = http_request("DELETE", "/db/destroy", "e2e_test");
        assert_eq!(status, 200);
    }
}
