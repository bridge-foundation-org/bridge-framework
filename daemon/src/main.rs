//! Bridge daemon — entry point.
//!
//! Starts three servers concurrently:
//! 1. **Miniredis** — embedded Redis on `BRIDGE_REDIS_ADDR`
//! 2. **HTTP**      — REST API on `BRIDGE_HTTP_ADDR`
//! 3. **TCP**       — line-protocol server on `BRIDGE_TCP_ADDR` (main thread)
//!
//! All three share the same `Arc<Mutex<State>>`.

mod auth;
mod autocomplete;
mod cache;
mod config;
mod config_schema;
mod context;
mod cron;
mod errors;
mod go_codegen;
mod http;
mod infra_config;
mod logger;
mod metrics;
mod metrics_exporters;
mod middleware;
mod pubsub;
mod pubsub_provider;
mod ratelimit;
mod redis_cluster;
mod registry;
mod scaffold;
mod schema_introspect;
mod secrets;
mod services;
mod shutdown;
mod sqldb;
mod state;
mod staticfiles;
mod storage;
mod streaming;
mod tcp;
mod tracing;
mod transactions;
mod transport;
mod validation;
mod watcher;

use std::sync::{Arc, Mutex};
use std::thread;

use state::State;

const DEFAULT_TCP_ADDR: &str = "127.0.0.1:7878";
const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8787";
const DEFAULT_REDIS_ADDR: &str = "127.0.0.1:6399";

fn main() -> std::io::Result<()> {
    let tcp_addr = env("BRIDGE_TCP_ADDR", DEFAULT_TCP_ADDR);
    let http_addr = env("BRIDGE_HTTP_ADDR", DEFAULT_HTTP_ADDR);
    let redis_addr = env("BRIDGE_REDIS_ADDR", DEFAULT_REDIS_ADDR);

    // ── 1. Start miniredis ────────────────────────────────────────────────
    let (redis_info_addr, redis_conn_count) = match miniredis::MiniRedis::start(&redis_addr) {
        Ok((server, _handle)) => {
            let addr = server.addr.to_string();
            let conns = Arc::clone(&server.connection_count);
            eprintln!("[bridge] miniredis on {addr}");
            (Some(addr), Some(conns))
        }
        Err(e) => {
            eprintln!("[bridge] miniredis failed to start: {e}");
            (None, None)
        }
    };

    // ── 2. Shared state ───────────────────────────────────────────────────
    let shared = Arc::new(Mutex::new(State::new(redis_info_addr, redis_conn_count)));

    // ── 3. Load bridge.toml (optional) ────────────────────────────────────
    let config_path = env("BRIDGE_CONFIG", "bridge.toml");
    match config::BridgeConfig::load(&config_path) {
        Ok(Some(cfg)) => {
            eprintln!("[bridge] loaded config: {config_path}");
            config::apply(&cfg, &shared);
        }
        Ok(None) => {
            // Try current directory
            if let Ok(Some(cfg)) = config::BridgeConfig::load_from_dir(".") {
                eprintln!("[bridge] loaded config: ./bridge.toml");
                config::apply(&cfg, &shared);
            }
        }
        Err(e) => eprintln!("[bridge] config warning: {e}"),
    }

    // ── 3. HTTP server (background thread) ───────────────────────────────
    {
        let state = Arc::clone(&shared);
        let addr = http_addr.clone();
        thread::spawn(move || {
            if let Err(e) = http::run_http_server(&addr, state) {
                eprintln!("[bridge] HTTP error: {e}");
            }
        });
    }

    // ── 4. Hot-reload watcher (background thread) ─────────────────────────
    {
        let state = Arc::clone(&shared);
        // Watch current directory for .bridge files if BRIDGE_WATCH_DIR set
        if let Ok(dir) = std::env::var("BRIDGE_WATCH_DIR") {
            state.lock().unwrap().watcher.watch_dir(&dir);
        }
        state.lock().unwrap().watcher.running = true;
        watcher::start_watcher(Arc::clone(&state));
        eprintln!("[bridge] hot-reload watcher started");
    }

    // ── 5. TCP server (main thread — blocks until process exits) ─────────
    tcp::run_tcp_server(&tcp_addr, shared)
}

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
