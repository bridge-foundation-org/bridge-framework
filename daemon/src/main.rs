//! Bridge daemon — TCP + HTTP server orchestrating all workspace crates.
//!
//! # Architecture
//!
//! The daemon is split into four modules:
//!
//! - **`state`** — Shared state (mode, key-value store, Redis info)
//! - **`sqldb`** — Docker Postgres lifecycle management
//! - **`tcp`** — TCP protocol server (line-oriented commands)
//! - **`http`** — HTTP REST API server
//!
//! On startup the daemon:
//! 1. Starts miniredis in a background thread
//! 2. Launches the HTTP server in a background thread
//! 3. Runs the TCP server on the main thread

mod http;
mod sqldb;
mod state;
mod tcp;

use std::env;
use std::sync::{Arc, Mutex};
use std::thread;

const DEFAULT_TCP_ADDR: &str = "127.0.0.1:7878";
const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8787";
const DEFAULT_REDIS_ADDR: &str = "127.0.0.1:6399";

fn main() -> std::io::Result<()> {
    let tcp_addr = env::var("BRIDGE_TCP_ADDR").unwrap_or_else(|_| DEFAULT_TCP_ADDR.to_string());
    let http_addr = env::var("BRIDGE_HTTP_ADDR").unwrap_or_else(|_| DEFAULT_HTTP_ADDR.to_string());
    let redis_addr =
        env::var("BRIDGE_REDIS_ADDR").unwrap_or_else(|_| DEFAULT_REDIS_ADDR.to_string());

    // ── Start miniredis in background ──
    let (redis_info_addr, redis_conn_count) = match miniredis::MiniRedis::start(&redis_addr) {
        Ok((server, _handle)) => {
            let addr_str = server.addr.to_string();
            let conn_count = Arc::clone(&server.connection_count);
            eprintln!("miniredis started on {addr_str}");
            (Some(addr_str), Some(conn_count))
        }
        Err(e) => {
            eprintln!("miniredis failed to start: {e}");
            (None, None)
        }
    };

    // ── Shared state ──
    let shared = Arc::new(Mutex::new(state::State::new(
        redis_info_addr,
        redis_conn_count,
    )));

    // ── HTTP server (background thread) ──
    let http_state = Arc::clone(&shared);
    let http_addr_for_thread = http_addr.clone();
    thread::spawn(move || {
        if let Err(err) = http::run_http_server(&http_addr_for_thread, http_state) {
            eprintln!("http server error: {err}");
        }
    });

    // ── TCP server (main thread) ──
    tcp::run_tcp_server(&tcp_addr, shared)
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use db::Store;

    use crate::state::State;
    use crate::tcp::process_line_command;

    fn test_state() -> Arc<Mutex<State>> {
        Arc::new(Mutex::new(State {
            mode: "full".to_string(),
            store: Store::new(),
            redis_addr: Some("127.0.0.1:6399".to_string()),
            redis_connections: Some(Arc::new(std::sync::atomic::AtomicUsize::new(0))),
        }))
    }

    #[test]
    fn compile_command_generates_code() {
        let state = test_state();
        let source = "service%20hello%0Aendpoint%20ping%20GET%20/ping";
        let response = process_line_command(&format!("COMPILE {source}"), Arc::clone(&state));
        assert!(response.starts_with("DATA "));
        assert!(
            state
                .lock()
                .expect("state lock poisoned")
                .store
                .get("codegen", "latest")
                .is_some()
        );
    }

    #[test]
    fn db_create_command_without_docker() {
        let state = test_state();
        let response = process_line_command("DB CREATE testdb", Arc::clone(&state));
        assert!(response.starts_with("OK ") || response.starts_with("ERR "));
    }

    #[test]
    fn db_status_command() {
        let state = test_state();
        let response = process_line_command("DB STATUS", Arc::clone(&state));
        assert!(response.starts_with("DATA ") || response.starts_with("ERR "));
    }

    #[test]
    fn redis_status_command() {
        let state = test_state();
        let response = process_line_command("REDIS STATUS", Arc::clone(&state));
        assert!(response.starts_with("DATA "));
        assert!(response.contains("addr=127.0.0.1:6399"));
    }
}
