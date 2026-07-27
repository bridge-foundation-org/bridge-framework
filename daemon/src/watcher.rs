//! Hot-reload file watcher — watches `.bridge` files for changes and
//! recompiles them automatically, publishing SSE events to connected clients.
//!
//! ## Architecture
//!
//! ```text
//! WatchRegistry  ←────────────────────────────────────────────────────────────
//!   watched_dirs: Vec<String>          directories being scanned
//!   watched_files: Vec<WatchedFile>    per-file metadata + last mtime
//!   sse_senders: Vec<Sender<String>>   SSE channels for connected clients
//!
//! Background thread (run_watcher):
//!   loop every poll_interval:
//!     for each file in watched_files:
//!       read current mtime
//!       if mtime changed:
//!         recompile(path) → Ok(ts) | Err(msg)
//!         update WatchedFile.{mtime, last_result}
//!         broadcast SSE event to all connected clients
//! ```
//!
//! ## SSE event format
//!
//! Clients connect to `GET /api/v1/watch/events` (chunked Transfer-Encoding).
//! Each change produces a `data:` line in SSE format:
//!
//! ```text
//! event: reload\n
//! data: {"file":"/path/to/svc.bridge","status":"ok","ts":1720000000}\n
//! \n
//! ```
//!
//! On compile error:
//! ```text
//! event: error\n
//! data: {"file":"/path/to/svc.bridge","status":"error","message":"..."}\n
//! \n
//! ```

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

// ── Types ─────────────────────────────────────────────────────────────────────

/// Result of the last compile attempt for a watched file.
#[derive(Debug, Clone, PartialEq)]
pub enum CompileResult {
    Ok(String),          // TypeScript client source
    Err(String),         // compiler error message
    Pending,             // not yet compiled
}

/// Per-file watch metadata.
#[derive(Debug, Clone)]
pub struct WatchedFile {
    pub path:          String,
    /// Modification time at last check (seconds since UNIX epoch).
    pub last_mtime:    Option<u64>,
    /// Count of recompile events triggered.
    pub change_count:  u64,
    /// Result of the most recent compile.
    pub last_result:   CompileResult,
}

impl WatchedFile {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), last_mtime: None, change_count: 0, last_result: CompileResult::Pending }
    }

    pub fn to_json(&self) -> String {
        let status = match &self.last_result {
            CompileResult::Ok(_)      => "ok",
            CompileResult::Err(_)     => "error",
            CompileResult::Pending    => "pending",
        };
        let err_msg = match &self.last_result {
            CompileResult::Err(e) => format!(",\"error\":\"{}\"", e.replace('"', "\\\"")),
            _                     => String::new(),
        };
        format!(
            r#"{{"path":"{}","status":"{}","changes":{}{}}}"#,
            self.path, status, self.change_count, err_msg
        )
    }
}

// ── SSE sender ────────────────────────────────────────────────────────────────

/// A single connected SSE client.  The sender end is stored; when the client
/// disconnects the channel is broken and we prune the dead sender.
pub struct SseSender {
    pub id:     u64,
    sender:     std::sync::mpsc::SyncSender<String>,
}

impl SseSender {
    pub fn send(&self, msg: &str) -> bool {
        self.sender.try_send(msg.to_string()).is_ok()
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct WatchRegistry {
    /// Directories being scanned (for API display).
    pub watched_dirs:  Vec<String>,
    /// Per-file watch state.
    pub files:         Vec<WatchedFile>,
    /// Active SSE connections.
    pub sse_clients:   Vec<SseSender>,
    /// Next SSE client ID.
    next_client_id:    u64,
    /// Poll interval in milliseconds.
    pub poll_ms:        u64,
    /// Whether the watcher background thread is running.
    pub running:        bool,
    /// Total events broadcast.
    pub events_total:  u64,
}

impl std::fmt::Debug for WatchRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchRegistry")
            .field("watched_dirs", &self.watched_dirs)
            .field("files",        &self.files.len())
            .field("sse_clients",  &self.sse_clients.len())
            .field("poll_ms",      &self.poll_ms)
            .field("running",      &self.running)
            .finish()
    }
}

impl WatchRegistry {
    pub fn new() -> Self {
        Self { poll_ms: 500, ..Default::default() }
    }

    /// Add a directory to watch (scans for `.bridge` files).
    pub fn watch_dir(&mut self, dir: impl Into<String>) {
        let d = dir.into();
        if !self.watched_dirs.contains(&d) {
            self.watched_dirs.push(d.clone());
        }
        // Eagerly enumerate *.bridge files in the directory
        if let Ok(entries) = std::fs::read_dir(&d) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("bridge") {
                    let path_str = p.to_string_lossy().into_owned();
                    self.watch_file(path_str);
                }
            }
        }
    }

    /// Add a specific file to watch.
    pub fn watch_file(&mut self, path: impl Into<String>) {
        let p = path.into();
        if !self.files.iter().any(|f| f.path == p) {
            self.files.push(WatchedFile::new(p));
        }
    }

    /// Remove a file from the watch list.
    pub fn unwatch(&mut self, path: &str) -> bool {
        let before = self.files.len();
        self.files.retain(|f| f.path != path);
        self.files.len() < before
    }

    /// Register an SSE client.  Returns the receiver the HTTP handler will
    /// drain to write chunked SSE bytes to the client.
    pub fn add_sse_client(&mut self) -> (u64, std::sync::mpsc::Receiver<String>) {
        let (tx, rx) = std::sync::mpsc::sync_channel(64);
        let id = self.next_client_id;
        self.next_client_id += 1;
        self.sse_clients.push(SseSender { id, sender: tx });
        (id, rx)
    }

    /// Remove a disconnected SSE client by ID.
    pub fn remove_sse_client(&mut self, id: u64) {
        self.sse_clients.retain(|c| c.id != id);
    }

    /// Broadcast an SSE message to all live clients, pruning dead ones.
    pub fn broadcast(&mut self, msg: &str) {
        self.events_total += 1;
        self.sse_clients.retain(|c| c.send(msg));
    }

    /// Serialize registry status to JSON.
    pub fn to_json(&self) -> String {
        let files: Vec<String> = self.files.iter().map(|f| f.to_json()).collect();
        format!(
            r#"{{"watching":{},"dirs":{},"files":[{}],"sse_clients":{},"poll_ms":{},"events_total":{}}}"#,
            self.running,
            self.watched_dirs.len(),
            files.join(","),
            self.sse_clients.len(),
            self.poll_ms,
            self.events_total,
        )
    }
}

// ── Background watcher ────────────────────────────────────────────────────────

/// Start the hot-reload background thread.
///
/// The thread polls `registry` (via `SharedState`) every `poll_ms` milliseconds.
/// When a `.bridge` file's mtime changes it is recompiled and an SSE event is
/// broadcast to all connected clients.
pub fn start_watcher(state: Arc<Mutex<crate::state::State>>) {
    std::thread::spawn(move || {
        loop {
            let poll_ms = {
                let g = state.lock().unwrap();
                g.watcher.poll_ms
            };
            std::thread::sleep(Duration::from_millis(poll_ms));

            // Collect snapshot of file paths to check
            let paths: Vec<String> = {
                let g = state.lock().unwrap();
                g.watcher.files.iter().map(|f| f.path.clone()).collect()
            };

            for path in paths {
                let mtime = file_mtime(&path);
                let changed = {
                    let g = state.lock().unwrap();
                    match g.watcher.files.iter().find(|f| f.path == path) {
                        Some(wf) => wf.last_mtime != mtime,
                        None     => false,
                    }
                };

                if changed {
                    let result = recompile_file(&path);
                    let mut g = state.lock().unwrap();

                    // Update the file entry
                    if let Some(wf) = g.watcher.files.iter_mut().find(|f| f.path == path) {
                        wf.last_mtime   = mtime;
                        wf.change_count += 1;
                        wf.last_result  = result.clone();
                    }

                    // Update the service registry on success
                    if let CompileResult::Ok(ref ts) = result {
                        if let Ok(file) = compiler::parse(&std::fs::read_to_string(&path).unwrap_or_default()) {
                            if let Some(first) = file.services.first() {
                                g.store.put("codegen", &first.name, ts.clone());
                            }
                            g.store.put("codegen", "latest", ts.clone());
                            g.service_registry = Some(file);
                        }
                    }

                    // Build SSE event
                    let ts_now = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_secs()).unwrap_or(0);
                    let event = match &result {
                        CompileResult::Ok(_) => format!(
                            "event: reload\ndata: {{\"file\":\"{}\",\"status\":\"ok\",\"ts\":{}}}\n\n",
                            path, ts_now
                        ),
                        CompileResult::Err(e) => format!(
                            "event: error\ndata: {{\"file\":\"{}\",\"status\":\"error\",\"message\":\"{}\",\"ts\":{}}}\n\n",
                            path, e.replace('"', "\\\""), ts_now
                        ),
                        CompileResult::Pending => continue,
                    };

                    g.watcher.broadcast(&event);

                    let level = if matches!(result, CompileResult::Ok(_)) {
                        crate::state::LogLevel::Info
                    } else {
                        crate::state::LogLevel::Warn
                    };
                    g.push_log(level, &format!("[watcher] recompiled {path}"), Default::default());
                }
            }
        }
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Read the modification time of a file as seconds since UNIX epoch.
pub fn file_mtime(path: &str) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Read and compile a `.bridge` file, returning the TypeScript client or an error.
pub fn recompile_file(path: &str) -> CompileResult {
    let source = match std::fs::read_to_string(path) {
        Ok(s)  => s,
        Err(e) => return CompileResult::Err(format!("read error: {e}")),
    };
    match compiler::parse(&source) {
        Ok(file) => CompileResult::Ok(codegen::generate_typescript(&file)),
        Err(e)   => CompileResult::Err(e),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── WatchedFile ───────────────────────────────────────────────────────────

    #[test]
    fn watched_file_initial_state() {
        let wf = WatchedFile::new("/app/svc.bridge");
        assert_eq!(wf.path, "/app/svc.bridge");
        assert_eq!(wf.last_mtime, None);
        assert_eq!(wf.change_count, 0);
        assert_eq!(wf.last_result, CompileResult::Pending);
    }

    #[test]
    fn watched_file_to_json_pending() {
        let wf = WatchedFile::new("/a.bridge");
        let j = wf.to_json();
        assert!(j.contains("pending"), "got: {j}");
        assert!(j.contains("changes\":0"), "got: {j}");
    }

    #[test]
    fn watched_file_to_json_ok() {
        let mut wf = WatchedFile::new("/a.bridge");
        wf.last_result = CompileResult::Ok("// ts".into());
        wf.change_count = 3;
        let j = wf.to_json();
        assert!(j.contains("\"status\":\"ok\""),  "got: {j}");
        assert!(j.contains("\"changes\":3"),       "got: {j}");
        assert!(!j.contains("error"),              "ok result should not contain error key: {j}");
    }

    #[test]
    fn watched_file_to_json_error() {
        let mut wf = WatchedFile::new("/b.bridge");
        wf.last_result = CompileResult::Err("parse failed".into());
        let j = wf.to_json();
        assert!(j.contains("\"status\":\"error\""), "got: {j}");
        assert!(j.contains("parse failed"),          "got: {j}");
    }

    // ── WatchRegistry ─────────────────────────────────────────────────────────

    #[test]
    fn watch_file_deduplicates() {
        let mut reg = WatchRegistry::new();
        reg.watch_file("/x.bridge");
        reg.watch_file("/x.bridge");
        assert_eq!(reg.files.len(), 1);
    }

    #[test]
    fn unwatch_removes_file() {
        let mut reg = WatchRegistry::new();
        reg.watch_file("/a.bridge");
        reg.watch_file("/b.bridge");
        assert!(reg.unwatch("/a.bridge"));
        assert_eq!(reg.files.len(), 1);
        assert_eq!(reg.files[0].path, "/b.bridge");
    }

    #[test]
    fn unwatch_returns_false_when_not_present() {
        let mut reg = WatchRegistry::new();
        assert!(!reg.unwatch("/nonexistent.bridge"));
    }

    #[test]
    fn watch_dir_deduplicates_dir() {
        let mut reg = WatchRegistry::new();
        reg.watched_dirs.push("/some/dir".to_string());
        reg.watch_dir("/some/dir"); // should not double-add
        assert_eq!(reg.watched_dirs.len(), 1);
    }

    #[test]
    fn to_json_structure() {
        let mut reg = WatchRegistry::new();
        reg.watch_file("/svc.bridge");
        let j = reg.to_json();
        assert!(j.contains("watching"),      "got: {j}");
        assert!(j.contains("poll_ms"),        "got: {j}");
        assert!(j.contains("events_total"),   "got: {j}");
        assert!(j.contains("svc.bridge"),     "got: {j}");
    }

    // ── SSE broadcast ─────────────────────────────────────────────────────────

    #[test]
    fn add_and_broadcast_sse() {
        let mut reg = WatchRegistry::new();
        let (_id, rx) = reg.add_sse_client();
        reg.broadcast("event: reload\ndata: {}\n\n");
        let msg = rx.try_recv().expect("should have message");
        assert!(msg.contains("reload"), "got: {msg}");
        assert_eq!(reg.events_total, 1);
    }

    #[test]
    fn broadcast_prunes_dead_clients() {
        let mut reg = WatchRegistry::new();
        let (id, rx) = reg.add_sse_client();
        drop(rx); // close receiver — sender will be detected as dead
        reg.broadcast("test");
        // Dead client is pruned after failed send
        assert_eq!(reg.sse_clients.len(), 0, "dead client should be pruned");
        let _ = id;
    }

    #[test]
    fn multiple_clients_all_receive() {
        let mut reg = WatchRegistry::new();
        let (_, rx1) = reg.add_sse_client();
        let (_, rx2) = reg.add_sse_client();
        reg.broadcast("hello");
        assert_eq!(rx1.try_recv().unwrap(), "hello");
        assert_eq!(rx2.try_recv().unwrap(), "hello");
    }

    #[test]
    fn remove_sse_client_by_id() {
        let mut reg = WatchRegistry::new();
        let (id, _rx) = reg.add_sse_client();
        reg.remove_sse_client(id);
        assert_eq!(reg.sse_clients.len(), 0);
    }

    // ── recompile_file ────────────────────────────────────────────────────────

    #[test]
    fn recompile_missing_file_is_error() {
        let result = recompile_file("/nonexistent/path/does_not_exist.bridge");
        assert!(matches!(result, CompileResult::Err(_)), "expected Err for missing file");
    }

    #[test]
    fn recompile_valid_content() {
        // Write a temp .bridge file and compile it
        let dir = std::env::temp_dir();
        let path = dir.join("bridge_watcher_test.bridge");
        std::fs::write(&path, "service hello\nendpoint ping GET /ping\n").unwrap();
        let result = recompile_file(&path.to_string_lossy());
        assert!(matches!(result, CompileResult::Ok(_)), "expected Ok, got: {result:?}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recompile_invalid_content_is_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("bridge_watcher_invalid_test.bridge");
        std::fs::write(&path, "this is not valid bridge DSL !!!\n").unwrap();
        let result = recompile_file(&path.to_string_lossy());
        assert!(matches!(result, CompileResult::Err(_)), "expected Err for invalid content");
        let _ = std::fs::remove_file(&path);
    }

    // ── file_mtime ────────────────────────────────────────────────────────────

    #[test]
    fn file_mtime_missing_returns_none() {
        assert_eq!(file_mtime("/no/such/file"), None);
    }

    #[test]
    fn file_mtime_existing_file_returns_some() {
        let dir  = std::env::temp_dir();
        let path = dir.join("bridge_mtime_test.tmp");
        std::fs::write(&path, "x").unwrap();
        let mtime = file_mtime(&path.to_string_lossy());
        assert!(mtime.is_some(), "expected mtime for existing file");
        let _ = std::fs::remove_file(&path);
    }
}
