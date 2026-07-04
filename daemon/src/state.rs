//! Shared daemon state.
//!
//! Contains the mode flag, in-memory key-value store, and Redis connection info.
//! This module is imported by both the TCP and HTTP servers.

use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use db::Store;

/// Central daemon state shared between TCP and HTTP servers.
#[derive(Debug)]
pub struct State {
    pub mode: String,
    pub store: Store,
    pub redis_addr: Option<String>,
    pub redis_connections: Option<Arc<AtomicUsize>>,
}

impl State {
    /// Create a new State with the given Redis info.
    pub fn new(redis_addr: Option<String>, redis_connections: Option<Arc<AtomicUsize>>) -> Self {
        Self {
            mode: "full".to_string(),
            store: Store::new(),
            redis_addr,
            redis_connections,
        }
    }
}

/// Convenience type alias.
pub type SharedState = Arc<Mutex<State>>;
