//! Minimal Redis-compatible server in pure Rust (no external crates).
//!
//! Supports: PING, SET (EX/PX), GET, DEL, EXISTS, KEYS, EXPIRE, TTL, COMMAND
//!
//! Public API: `MiniRedis::start(addr) -> (JoinHandle, SocketAddr)`

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// ── RESP protocol ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum RespValue {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Option<String>),
    Array(Vec<RespValue>),
}

impl RespValue {
    pub fn ok() -> Self {
        RespValue::SimpleString("OK".to_string())
    }
    pub fn null() -> Self {
        RespValue::BulkString(None)
    }
    pub fn bulk(s: impl Into<String>) -> Self {
        RespValue::BulkString(Some(s.into()))
    }
    pub fn error(s: impl Into<String>) -> Self {
        RespValue::Error(s.into())
    }
    pub fn integer(n: i64) -> Self {
        RespValue::Integer(n)
    }

    pub fn serialize(&self) -> String {
        match self {
            RespValue::SimpleString(s) => format!("+{s}\r\n"),
            RespValue::Error(s) => format!("-{s}\r\n"),
            RespValue::Integer(n) => format!(":{n}\r\n"),
            RespValue::BulkString(None) => "$-1\r\n".to_string(),
            RespValue::BulkString(Some(s)) => format!("${}\r\n{}\r\n", s.len(), s),
            RespValue::Array(arr) => {
                let mut out = format!("*{}\r\n", arr.len());
                for item in arr {
                    out.push_str(&item.serialize());
                }
                out
            }
        }
    }
}

pub fn parse_resp(reader: &mut impl BufRead) -> Result<RespValue, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("read error: {e}"))?;
    if line.is_empty() {
        return Err("connection closed".to_string());
    }
    let line = line.trim_end_matches('\n').trim_end_matches('\r');
    if line.is_empty() {
        return Err("empty line".to_string());
    }
    let first = line.as_bytes()[0];
    let rest = &line[1..];
    match first {
        b'+' => Ok(RespValue::SimpleString(rest.to_string())),
        b'-' => Ok(RespValue::Error(rest.to_string())),
        b':' => {
            let n = rest
                .parse::<i64>()
                .map_err(|_| "invalid integer".to_string())?;
            Ok(RespValue::Integer(n))
        }
        b'$' => {
            let len = rest
                .parse::<i64>()
                .map_err(|_| "invalid bulk length".to_string())?;
            if len < 0 {
                return Ok(RespValue::BulkString(None));
            }
            let len = len as usize;
            let mut buf = vec![0u8; len + 2]; // +2 for \r\n
            reader
                .read_exact(&mut buf)
                .map_err(|e| format!("bulk read error: {e}"))?;
            let s = String::from_utf8_lossy(&buf[..len]).to_string();
            Ok(RespValue::BulkString(Some(s)))
        }
        b'*' => {
            let count = rest
                .parse::<i64>()
                .map_err(|_| "invalid array length".to_string())?;
            if count < 0 {
                return Ok(RespValue::Array(vec![]));
            }
            let mut items = Vec::with_capacity(count as usize);
            for _ in 0..count {
                items.push(parse_resp(reader)?);
            }
            Ok(RespValue::Array(items))
        }
        _ => {
            // Inline command (space-separated)
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                return Err("empty inline command".to_string());
            }
            Ok(RespValue::Array(
                parts.into_iter().map(|p| RespValue::bulk(p)).collect(),
            ))
        }
    }
}

// ── Store ───────────────────────────────────────────────────

struct Entry {
    value: String,
    expires_at: Option<Instant>,
}

impl Entry {
    fn new(value: String) -> Self {
        Self {
            value,
            expires_at: None,
        }
    }
    fn with_ttl(value: String, ttl: Duration) -> Self {
        Self {
            value,
            expires_at: Some(Instant::now() + ttl),
        }
    }
    fn is_expired(&self) -> bool {
        self.expires_at.map_or(false, |t| Instant::now() >= t)
    }
}

#[derive(Default)]
struct RedisStore {
    data: HashMap<String, Entry>,
}

impl RedisStore {
    fn get(&mut self, key: &str) -> Option<&str> {
        if let Some(entry) = self.data.get(key) {
            if entry.is_expired() {
                self.data.remove(key);
                return None;
            }
            // Re-borrow to satisfy borrow checker
            return self.data.get(key).map(|e| e.value.as_str());
        }
        None
    }

    fn set(&mut self, key: String, value: String, ttl: Option<Duration>) {
        let entry = match ttl {
            Some(d) => Entry::with_ttl(value, d),
            None => Entry::new(value),
        };
        self.data.insert(key, entry);
    }

    fn del(&mut self, keys: &[String]) -> i64 {
        let mut count = 0i64;
        for key in keys {
            if self.data.remove(key).is_some() {
                count += 1;
            }
        }
        count
    }

    fn exists(&mut self, keys: &[String]) -> i64 {
        let mut count = 0i64;
        for key in keys {
            if let Some(entry) = self.data.get(key) {
                if entry.is_expired() {
                    self.data.remove(key);
                } else {
                    count += 1;
                }
            }
        }
        count
    }

    fn keys(&mut self, pattern: &str) -> Vec<String> {
        // Remove expired entries first
        let expired: Vec<String> = self
            .data
            .iter()
            .filter(|(_, v)| v.is_expired())
            .map(|(k, _)| k.clone())
            .collect();
        for k in expired {
            self.data.remove(&k);
        }
        self.data
            .keys()
            .filter(|k| glob_match(pattern, k))
            .cloned()
            .collect()
    }

    fn expire(&mut self, key: &str, seconds: u64) -> bool {
        if let Some(entry) = self.data.get_mut(key) {
            if entry.is_expired() {
                self.data.remove(key);
                return false;
            }
            self.data.get_mut(key).unwrap().expires_at =
                Some(Instant::now() + Duration::from_secs(seconds));
            true
        } else {
            false
        }
    }

    fn ttl(&mut self, key: &str) -> i64 {
        if let Some(entry) = self.data.get(key) {
            if entry.is_expired() {
                self.data.remove(key);
                return -2;
            }
            match entry.expires_at {
                Some(t) => {
                    let remaining = t.saturating_duration_since(Instant::now());
                    remaining.as_secs() as i64
                }
                None => -1,
            }
        } else {
            -2
        }
    }
}

fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    // Simple glob: support leading *, trailing *, and *middle*
    if let Some(suffix) = pattern.strip_prefix('*') {
        if let Some(prefix) = suffix.strip_suffix('*') {
            return text.contains(prefix);
        }
        return text.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return text.starts_with(prefix);
    }
    pattern == text
}

// ── Command dispatch ────────────────────────────────────────

fn extract_strings(value: &RespValue) -> Vec<String> {
    match value {
        RespValue::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                RespValue::BulkString(Some(s)) => Some(s.clone()),
                RespValue::SimpleString(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

fn handle_command(args: &[String], store: &mut RedisStore) -> RespValue {
    if args.is_empty() {
        return RespValue::error("ERR no command");
    }
    let cmd = args[0].to_ascii_uppercase();
    match cmd.as_str() {
        "PING" => {
            if args.len() > 1 {
                RespValue::bulk(&args[1])
            } else {
                RespValue::SimpleString("PONG".to_string())
            }
        }
        "SET" => {
            if args.len() < 3 {
                return RespValue::error("ERR wrong number of arguments for 'SET'");
            }
            let key = args[1].clone();
            let value = args[2].clone();
            let mut ttl: Option<Duration> = None;
            let mut i = 3;
            while i < args.len() {
                match args[i].to_ascii_uppercase().as_str() {
                    "EX" => {
                        i += 1;
                        if i >= args.len() {
                            return RespValue::error("ERR syntax error");
                        }
                        let secs = args[i]
                            .parse::<u64>()
                            .map_err(|_| "ERR value is not an integer");
                        match secs {
                            Ok(s) => ttl = Some(Duration::from_secs(s)),
                            Err(e) => return RespValue::error(e),
                        }
                    }
                    "PX" => {
                        i += 1;
                        if i >= args.len() {
                            return RespValue::error("ERR syntax error");
                        }
                        let ms = args[i]
                            .parse::<u64>()
                            .map_err(|_| "ERR value is not an integer");
                        match ms {
                            Ok(m) => ttl = Some(Duration::from_millis(m)),
                            Err(e) => return RespValue::error(e),
                        }
                    }
                    _ => return RespValue::error("ERR syntax error"),
                }
                i += 1;
            }
            store.set(key, value, ttl);
            RespValue::ok()
        }
        "GET" => {
            if args.len() != 2 {
                return RespValue::error("ERR wrong number of arguments for 'GET'");
            }
            match store.get(&args[1]) {
                Some(v) => RespValue::bulk(v),
                None => RespValue::null(),
            }
        }
        "DEL" => {
            if args.len() < 2 {
                return RespValue::error("ERR wrong number of arguments for 'DEL'");
            }
            let keys: Vec<String> = args[1..].to_vec();
            RespValue::integer(store.del(&keys))
        }
        "EXISTS" => {
            if args.len() < 2 {
                return RespValue::error("ERR wrong number of arguments for 'EXISTS'");
            }
            let keys: Vec<String> = args[1..].to_vec();
            RespValue::integer(store.exists(&keys))
        }
        "KEYS" => {
            if args.len() != 2 {
                return RespValue::error("ERR wrong number of arguments for 'KEYS'");
            }
            let matched = store.keys(&args[1]);
            RespValue::Array(matched.into_iter().map(RespValue::bulk).collect())
        }
        "EXPIRE" => {
            if args.len() != 3 {
                return RespValue::error("ERR wrong number of arguments for 'EXPIRE'");
            }
            let seconds = match args[2].parse::<u64>() {
                Ok(s) => s,
                Err(_) => return RespValue::error("ERR value is not an integer"),
            };
            RespValue::integer(if store.expire(&args[1], seconds) {
                1
            } else {
                0
            })
        }
        "TTL" => {
            if args.len() != 2 {
                return RespValue::error("ERR wrong number of arguments for 'TTL'");
            }
            RespValue::integer(store.ttl(&args[1]))
        }
        "COMMAND" => {
            // Redis COMMAND — just return OK for compatibility
            RespValue::ok()
        }
        _ => RespValue::error(format!(
            "ERR unknown command '{}', with args beginning with: {}",
            cmd,
            args.get(1).unwrap_or(&String::new())
        )),
    }
}

// ── Server ──────────────────────────────────────────────────

pub struct MiniRedis {
    pub addr: SocketAddr,
    pub connection_count: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
}

impl MiniRedis {
    /// Start a miniredis server on the given address.
    /// Returns `(MiniRedis handle, JoinHandle)`.
    pub fn start(addr: &str) -> Result<(Self, JoinHandle<()>), String> {
        let listener = TcpListener::bind(addr).map_err(|e| format!("bind error: {e}"))?;
        let actual_addr = listener
            .local_addr()
            .map_err(|e| format!("addr error: {e}"))?;
        let store = Arc::new(Mutex::new(RedisStore::default()));
        let connection_count = Arc::new(AtomicUsize::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));

        let conn_count = Arc::clone(&connection_count);
        let shut = Arc::clone(&shutdown);

        // Set non-blocking so we can check shutdown flag
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("set_nonblocking: {e}"))?;

        let handle = thread::spawn(move || {
            eprintln!("miniredis listening on {actual_addr}");
            loop {
                if shut.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _peer)) => {
                        conn_count.fetch_add(1, Ordering::Relaxed);
                        let store = Arc::clone(&store);
                        thread::spawn(move || {
                            if let Err(e) = handle_redis_client(stream, store) {
                                eprintln!("miniredis client error: {e}");
                            }
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        eprintln!("miniredis accept error: {e}");
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });

        Ok((
            MiniRedis {
                addr: actual_addr,
                connection_count,
                shutdown,
            },
            handle,
        ))
    }

    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn connections(&self) -> usize {
        self.connection_count.load(Ordering::Relaxed)
    }
}

fn handle_redis_client(
    stream: TcpStream,
    store: Arc<Mutex<RedisStore>>,
) -> Result<(), String> {
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|e| format!("clone error: {e}"))?,
    );
    let mut writer = stream;
    loop {
        let value = match parse_resp(&mut reader) {
            Ok(v) => v,
            Err(_) => break, // client disconnected
        };
        let args = extract_strings(&value);
        let response = {
            let mut guard = store.lock().expect("store lock poisoned");
            handle_command(&args, &mut guard)
        };
        writer
            .write_all(response.serialize().as_bytes())
            .map_err(|e| format!("write error: {e}"))?;
        writer.flush().map_err(|e| format!("flush error: {e}"))?;
    }
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resp_parse_simple_string() {
        let input = b"+OK\r\n";
        let mut reader = std::io::BufReader::new(&input[..]);
        let val = parse_resp(&mut reader).unwrap();
        assert_eq!(val, RespValue::SimpleString("OK".to_string()));
    }

    #[test]
    fn resp_parse_integer() {
        let input = b":42\r\n";
        let mut reader = std::io::BufReader::new(&input[..]);
        let val = parse_resp(&mut reader).unwrap();
        assert_eq!(val, RespValue::Integer(42));
    }

    #[test]
    fn resp_parse_bulk_string() {
        let input = b"$5\r\nhello\r\n";
        let mut reader = std::io::BufReader::new(&input[..]);
        let val = parse_resp(&mut reader).unwrap();
        assert_eq!(val, RespValue::BulkString(Some("hello".to_string())));
    }

    #[test]
    fn resp_parse_null_bulk() {
        let input = b"$-1\r\n";
        let mut reader = std::io::BufReader::new(&input[..]);
        let val = parse_resp(&mut reader).unwrap();
        assert_eq!(val, RespValue::BulkString(None));
    }

    #[test]
    fn resp_parse_array() {
        let input = b"*2\r\n$4\r\nPING\r\n$5\r\nhello\r\n";
        let mut reader = std::io::BufReader::new(&input[..]);
        let val = parse_resp(&mut reader).unwrap();
        assert_eq!(
            val,
            RespValue::Array(vec![
                RespValue::BulkString(Some("PING".to_string())),
                RespValue::BulkString(Some("hello".to_string())),
            ])
        );
    }

    #[test]
    fn resp_serialize_roundtrip() {
        let val = RespValue::Array(vec![
            RespValue::bulk("SET"),
            RespValue::bulk("key"),
            RespValue::bulk("value"),
        ]);
        let serialized = val.serialize();
        assert!(serialized.starts_with("*3\r\n"));
    }

    #[test]
    fn store_set_get() {
        let mut store = RedisStore::default();
        store.set("key".to_string(), "value".to_string(), None);
        assert_eq!(store.get("key"), Some("value"));
    }

    #[test]
    fn store_del() {
        let mut store = RedisStore::default();
        store.set("a".to_string(), "1".to_string(), None);
        store.set("b".to_string(), "2".to_string(), None);
        assert_eq!(store.del(&["a".to_string(), "c".to_string()]), 1);
        assert_eq!(store.get("a"), None);
        assert_eq!(store.get("b"), Some("2"));
    }

    #[test]
    fn store_exists() {
        let mut store = RedisStore::default();
        store.set("a".to_string(), "1".to_string(), None);
        assert_eq!(store.exists(&["a".to_string(), "b".to_string()]), 1);
    }

    #[test]
    fn store_keys_glob() {
        let mut store = RedisStore::default();
        store.set("user:1".to_string(), "a".to_string(), None);
        store.set("user:2".to_string(), "b".to_string(), None);
        store.set("session:1".to_string(), "c".to_string(), None);
        let mut keys = store.keys("user:*");
        keys.sort();
        assert_eq!(keys, vec!["user:1", "user:2"]);
    }

    #[test]
    fn handle_ping() {
        let mut store = RedisStore::default();
        let resp = handle_command(&["PING".to_string()], &mut store);
        assert_eq!(resp, RespValue::SimpleString("PONG".to_string()));
    }

    #[test]
    fn handle_set_get() {
        let mut store = RedisStore::default();
        let resp = handle_command(
            &["SET".to_string(), "key".to_string(), "val".to_string()],
            &mut store,
        );
        assert_eq!(resp, RespValue::ok());
        let resp = handle_command(&["GET".to_string(), "key".to_string()], &mut store);
        assert_eq!(resp, RespValue::bulk("val"));
    }

    #[test]
    fn handle_unknown_command() {
        let mut store = RedisStore::default();
        let resp = handle_command(&["FOOBAR".to_string()], &mut store);
        match resp {
            RespValue::Error(e) => assert!(e.contains("unknown command")),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn server_start_stop() {
        let (server, handle) = MiniRedis::start("127.0.0.1:0").expect("start failed");
        assert_ne!(server.addr.port(), 0);
        server.stop();
        handle.join().expect("join failed");
    }

    #[test]
    fn glob_match_patterns() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("user:*", "user:1"));
        assert!(!glob_match("user:*", "session:1"));
        assert!(glob_match("*:1", "user:1"));
        assert!(glob_match("*ser*", "user:1"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "other"));
    }
}
