//! Embedded Redis-compatible server — pure Rust stdlib, no external crates.
//!
//! ## Supported commands
//!
//! | Strings        | Lists              | Meta              |
//! |----------------|--------------------|-------------------|
//! | GET / SET      | LPUSH / RPUSH      | KEYS / TYPE       |
//! | MGET / MSET    | LRANGE / LLEN      | EXISTS / DEL      |
//! | INCR / DECR    | LINDEX             | EXPIRE / TTL      |
//! | INCRBY/DECRBY  |                    | FLUSHDB / COMMAND |
//! | SETNX          |                    | PING              |
//!
//! `MiniRedis::start(addr)` launches a background TCP listener that speaks
//! the Redis Serialization Protocol (RESP).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// ── RESP codec ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Resp {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Option<String>),
    Array(Vec<Resp>),
}

impl Resp {
    pub fn ok()              -> Self { Resp::SimpleString("OK".into()) }
    pub fn null()            -> Self { Resp::BulkString(None) }
    pub fn bulk(s: impl Into<String>) -> Self { Resp::BulkString(Some(s.into())) }
    pub fn err(s: impl Into<String>)  -> Self { Resp::Error(s.into()) }
    pub fn int(n: i64)       -> Self { Resp::Integer(n) }

    pub fn serialize(&self) -> Vec<u8> {
        match self {
            Resp::SimpleString(s) => format!("+{s}\r\n").into_bytes(),
            Resp::Error(s)        => format!("-{s}\r\n").into_bytes(),
            Resp::Integer(n)      => format!(":{n}\r\n").into_bytes(),
            Resp::BulkString(None)      => b"$-1\r\n".to_vec(),
            Resp::BulkString(Some(s))   => {
                let mut v = format!("${}\r\n", s.len()).into_bytes();
                v.extend_from_slice(s.as_bytes());
                v.extend_from_slice(b"\r\n");
                v
            }
            Resp::Array(arr) => {
                let mut v = format!("*{}\r\n", arr.len()).into_bytes();
                for item in arr { v.extend(item.serialize()); }
                v
            }
        }
    }
}

pub fn parse_resp(reader: &mut impl BufRead) -> Result<Resp, String> {
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    if line.is_empty() { return Err("connection closed".into()); }

    let line = line.trim_end_matches(['\r', '\n']);
    if line.is_empty() { return Err("empty line".into()); }

    let (tag, rest) = line.split_at(1);
    match tag {
        "+" => Ok(Resp::SimpleString(rest.to_string())),
        "-" => Ok(Resp::Error(rest.to_string())),
        ":" => rest.parse::<i64>().map(Resp::Integer)
                   .map_err(|_| "bad integer".into()),
        "$" => {
            let len: i64 = rest.parse().map_err(|_| "bad bulk len")?;
            if len < 0 { return Ok(Resp::null()); }
            let len = len as usize;
            let mut buf = vec![0u8; len + 2];
            reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
            Ok(Resp::bulk(String::from_utf8_lossy(&buf[..len]).to_string()))
        }
        "*" => {
            let n: i64 = rest.parse().map_err(|_| "bad array len")?;
            if n < 0 { return Ok(Resp::Array(vec![])); }
            (0..n).map(|_| parse_resp(reader)).collect::<Result<Vec<_>, _>>()
                  .map(Resp::Array)
        }
        _ => {
            // Inline command
            Ok(Resp::Array(
                line.split_whitespace().map(|s| Resp::bulk(s)).collect()
            ))
        }
    }
}

// ── In-memory store ───────────────────────────────────────────────────────────

enum StoreVal {
    String(String),
    List(Vec<String>),
    Hash(HashMap<String, String>),
}

struct Entry {
    val: StoreVal,
    expires_at: Option<Instant>,
}

impl Entry {
    fn str(v: String) -> Self { Entry { val: StoreVal::String(v), expires_at: None } }
    fn str_ex(v: String, ttl: Duration) -> Self {
        Entry { val: StoreVal::String(v), expires_at: Some(Instant::now() + ttl) }
    }
    fn list() -> Self { Entry { val: StoreVal::List(vec![]), expires_at: None } }
    fn hash() -> Self { Entry { val: StoreVal::Hash(HashMap::new()), expires_at: None } }
    fn is_expired(&self) -> bool {
        self.expires_at.map_or(false, |t| Instant::now() >= t)
    }
    fn type_name(&self) -> &'static str {
        match &self.val {
            StoreVal::String(_) => "string",
            StoreVal::List(_)   => "list",
            StoreVal::Hash(_)   => "hash",
        }
    }
}

#[derive(Default)]
struct Store { data: HashMap<String, Entry> }

impl Store {
    // --- expiry helpers ---
    fn live(&mut self, key: &str) -> Option<&Entry> {
        if let Some(e) = self.data.get(key) {
            if e.is_expired() { self.data.remove(key); return None; }
        }
        self.data.get(key)
    }
    fn live_mut(&mut self, key: &str) -> Option<&mut Entry> {
        if let Some(e) = self.data.get(key) {
            if e.is_expired() { self.data.remove(key); return None; }
        }
        self.data.get_mut(key)
    }

    // --- GET / SET ---
    fn get(&mut self, key: &str) -> Resp {
        match self.live(key) {
            Some(e) => match &e.val {
                StoreVal::String(s) => Resp::bulk(s.clone()),
                _ => Resp::err("WRONGTYPE not a string"),
            },
            None => Resp::null(),
        }
    }

    fn set(&mut self, key: String, val: String, ttl: Option<Duration>) -> Resp {
        let entry = match ttl {
            Some(d) => Entry::str_ex(val, d),
            None    => Entry::str(val),
        };
        self.data.insert(key, entry);
        Resp::ok()
    }

    fn setnx(&mut self, key: String, val: String) -> Resp {
        if self.live(&key).is_none() {
            self.data.insert(key, Entry::str(val));
            Resp::int(1)
        } else {
            Resp::int(0)
        }
    }

    // --- INCR / DECR ---
    fn incr_by(&mut self, key: &str, by: i64) -> Resp {
        let cur: i64 = match self.live(key) {
            None => 0,
            Some(e) => match &e.val {
                StoreVal::String(s) => match s.parse() {
                    Ok(n) => n,
                    Err(_) => return Resp::err("ERR value not an integer"),
                },
                _ => return Resp::err("WRONGTYPE not a string"),
            },
        };
        let next = cur + by;
        self.data.insert(key.to_string(), Entry::str(next.to_string()));
        Resp::int(next)
    }

    // --- MGET / MSET ---
    fn mget(&mut self, keys: &[String]) -> Resp {
        Resp::Array(keys.iter().map(|k| self.get(k)).collect())
    }

    fn mset(&mut self, pairs: &[(String, String)]) -> Resp {
        for (k, v) in pairs {
            self.data.insert(k.clone(), Entry::str(v.clone()));
        }
        Resp::ok()
    }

    // --- DEL / EXISTS ---
    fn del(&mut self, keys: &[String]) -> Resp {
        let n = keys.iter().filter(|k| self.data.remove(*k).is_some()).count();
        Resp::int(n as i64)
    }

    fn exists(&mut self, keys: &[String]) -> Resp {
        let n = keys.iter().filter(|k| self.live(k).is_some()).count();
        Resp::int(n as i64)
    }

    // --- EXPIRE / TTL ---
    fn expire(&mut self, key: &str, secs: u64) -> Resp {
        match self.live_mut(key) {
            Some(e) => { e.expires_at = Some(Instant::now() + Duration::from_secs(secs)); Resp::int(1) }
            None    => Resp::int(0),
        }
    }
    fn ttl(&mut self, key: &str) -> Resp {
        match self.live(key) {
            None    => Resp::int(-2),
            Some(e) => match e.expires_at {
                None    => Resp::int(-1),
                Some(t) => Resp::int(t.saturating_duration_since(Instant::now()).as_secs() as i64),
            },
        }
    }

    // --- TYPE / KEYS ---
    fn type_cmd(&mut self, key: &str) -> Resp {
        match self.live(key) {
            None    => Resp::bulk("none"),
            Some(e) => Resp::bulk(e.type_name()),
        }
    }

    fn keys(&mut self, pattern: &str) -> Resp {
        let expired: Vec<_> = self.data.iter()
            .filter(|(_, e)| e.is_expired()).map(|(k, _)| k.clone()).collect();
        for k in expired { self.data.remove(&k); }
        Resp::Array(
            self.data.keys().filter(|k| glob(pattern, k)).cloned()
                .map(Resp::bulk).collect()
        )
    }

    // --- FLUSHDB ---
    fn flushdb(&mut self) -> Resp { self.data.clear(); Resp::ok() }

    // --- List operations ---
    fn lpush(&mut self, key: String, vals: &[String]) -> Resp {
        let e = self.data.entry(key).or_insert_with(Entry::list);
        match &mut e.val {
            StoreVal::List(list) => {
                // Redis LPUSH: each element is inserted at head in order,
                // so the last element in vals ends up at the front.
                for v in vals.iter() { list.insert(0, v.clone()); }
                Resp::int(list.len() as i64)
            }
            _ => Resp::err("WRONGTYPE not a list"),
        }
    }

    fn rpush(&mut self, key: String, vals: &[String]) -> Resp {
        let e = self.data.entry(key).or_insert_with(Entry::list);
        match &mut e.val {
            StoreVal::List(list) => {
                for v in vals { list.push(v.clone()); }
                Resp::int(list.len() as i64)
            }
            _ => Resp::err("WRONGTYPE not a list"),
        }
    }

    fn lrange(&mut self, key: &str, start: i64, stop: i64) -> Resp {
        match self.live(key) {
            None => Resp::Array(vec![]),
            Some(e) => match &e.val {
                StoreVal::List(list) => {
                    let len = list.len() as i64;
                    let s = if start < 0 { (len + start).max(0) } else { start.min(len) } as usize;
                    let e = if stop  < 0 { (len + stop).max(-1) } else { stop.min(len - 1) } as usize;
                    if s > e as usize { return Resp::Array(vec![]); }
                    Resp::Array(list[s..=e].iter().map(|v| Resp::bulk(v.clone())).collect())
                }
                _ => Resp::err("WRONGTYPE not a list"),
            },
        }
    }

    fn llen(&mut self, key: &str) -> Resp {
        match self.live(key) {
            None => Resp::int(0),
            Some(e) => match &e.val {
                StoreVal::List(l) => Resp::int(l.len() as i64),
                _ => Resp::err("WRONGTYPE not a list"),
            },
        }
    }

    fn lindex(&mut self, key: &str, idx: i64) -> Resp {
        match self.live(key) {
            None => Resp::null(),
            Some(e) => match &e.val {
                StoreVal::List(list) => {
                    let len = list.len() as i64;
                    let i = if idx < 0 { len + idx } else { idx };
                    if i < 0 || i >= len { return Resp::null(); }
                    Resp::bulk(list[i as usize].clone())
                }
                _ => Resp::err("WRONGTYPE not a list"),
            },
        }
    }

    // --- HASH commands ---

    /// HSET key field value [field value ...]  — returns count of new fields added.
    fn hset(&mut self, key: &str, pairs: &[(String, String)]) -> Resp {
        let e = self.data.entry(key.to_string()).or_insert_with(Entry::hash);
        if e.is_expired() { *e = Entry::hash(); }
        match &mut e.val {
            StoreVal::Hash(h) => {
                let mut added = 0i64;
                for (f, v) in pairs {
                    if h.insert(f.clone(), v.clone()).is_none() { added += 1; }
                }
                Resp::int(added)
            }
            _ => Resp::err("WRONGTYPE not a hash"),
        }
    }

    /// HSETNX key field value — set only if field doesn't exist. Returns 1 if set, 0 if not.
    fn hsetnx(&mut self, key: &str, field: &str, value: &str) -> Resp {
        let e = self.data.entry(key.to_string()).or_insert_with(Entry::hash);
        if e.is_expired() { *e = Entry::hash(); }
        match &mut e.val {
            StoreVal::Hash(h) => {
                if h.contains_key(field) {
                    Resp::int(0)
                } else {
                    h.insert(field.to_string(), value.to_string());
                    Resp::int(1)
                }
            }
            _ => Resp::err("WRONGTYPE not a hash"),
        }
    }

    /// HGET key field
    fn hget(&mut self, key: &str, field: &str) -> Resp {
        match self.live(key) {
            None => Resp::null(),
            Some(e) => match &e.val {
                StoreVal::Hash(h) => h.get(field).map(|v| Resp::bulk(v.clone())).unwrap_or(Resp::null()),
                _ => Resp::err("WRONGTYPE not a hash"),
            },
        }
    }

    /// HMGET key field [field ...] — returns array of values (null for missing).
    fn hmget(&mut self, key: &str, fields: &[String]) -> Resp {
        match self.live(key) {
            None => Resp::Array(fields.iter().map(|_| Resp::null()).collect()),
            Some(e) => match &e.val {
                StoreVal::Hash(h) => Resp::Array(
                    fields.iter().map(|f| h.get(f.as_str()).map(|v| Resp::bulk(v.clone())).unwrap_or(Resp::null())).collect()
                ),
                _ => Resp::err("WRONGTYPE not a hash"),
            },
        }
    }

    /// HGETALL key — alternating field, value
    fn hgetall(&mut self, key: &str) -> Resp {
        match self.live(key) {
            None => Resp::Array(vec![]),
            Some(e) => match &e.val {
                StoreVal::Hash(h) => {
                    let mut items = Vec::with_capacity(h.len() * 2);
                    let mut pairs: Vec<_> = h.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.as_str()); // deterministic order
                    for (f, v) in pairs {
                        items.push(Resp::bulk(f.clone()));
                        items.push(Resp::bulk(v.clone()));
                    }
                    Resp::Array(items)
                }
                _ => Resp::err("WRONGTYPE not a hash"),
            },
        }
    }

    /// HDEL key field [field ...]
    fn hdel(&mut self, key: &str, fields: &[String]) -> Resp {
        match self.live_mut(key) {
            None => Resp::int(0),
            Some(e) => match &mut e.val {
                StoreVal::Hash(h) => {
                    let removed = fields.iter().filter(|f| h.remove(f.as_str()).is_some()).count();
                    Resp::int(removed as i64)
                }
                _ => Resp::err("WRONGTYPE not a hash"),
            },
        }
    }

    /// HLEN key
    fn hlen(&mut self, key: &str) -> Resp {
        match self.live(key) {
            None => Resp::int(0),
            Some(e) => match &e.val {
                StoreVal::Hash(h) => Resp::int(h.len() as i64),
                _ => Resp::err("WRONGTYPE not a hash"),
            },
        }
    }

    /// HEXISTS key field
    fn hexists(&mut self, key: &str, field: &str) -> Resp {
        match self.live(key) {
            None => Resp::int(0),
            Some(e) => match &e.val {
                StoreVal::Hash(h) => Resp::int(h.contains_key(field) as i64),
                _ => Resp::err("WRONGTYPE not a hash"),
            },
        }
    }

    /// HKEYS key
    fn hkeys(&mut self, key: &str) -> Resp {
        match self.live(key) {
            None => Resp::Array(vec![]),
            Some(e) => match &e.val {
                StoreVal::Hash(h) => {
                    let mut keys: Vec<_> = h.keys().cloned().collect();
                    keys.sort();
                    Resp::Array(keys.into_iter().map(Resp::bulk).collect())
                }
                _ => Resp::err("WRONGTYPE not a hash"),
            },
        }
    }

    /// HVALS key
    fn hvals(&mut self, key: &str) -> Resp {
        match self.live(key) {
            None => Resp::Array(vec![]),
            Some(e) => match &e.val {
                StoreVal::Hash(h) => {
                    let mut pairs: Vec<_> = h.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.as_str());
                    Resp::Array(pairs.into_iter().map(|(_, v)| Resp::bulk(v.clone())).collect())
                }
                _ => Resp::err("WRONGTYPE not a hash"),
            },
        }
    }

    /// HINCRBY key field increment
    fn hincrby(&mut self, key: &str, field: &str, by: i64) -> Resp {
        let e = self.data.entry(key.to_string()).or_insert_with(Entry::hash);
        if e.is_expired() { *e = Entry::hash(); }
        match &mut e.val {
            StoreVal::Hash(h) => {
                let cur: i64 = h.get(field).and_then(|v| v.parse().ok()).unwrap_or(0);
                let next = cur + by;
                h.insert(field.to_string(), next.to_string());
                Resp::int(next)
            }
            _ => Resp::err("WRONGTYPE not a hash"),
        }
    }
}

// ── Glob helper ───────────────────────────────────────────────────────────────

fn glob(pat: &str, text: &str) -> bool {
    if pat == "*" { return true; }
    if let Some(s) = pat.strip_prefix('*') {
        if let Some(p) = s.strip_suffix('*') { return text.contains(p); }
        return text.ends_with(s);
    }
    if let Some(p) = pat.strip_suffix('*') { return text.starts_with(p); }
    pat == text
}

// ── Command dispatch ──────────────────────────────────────────────────────────

fn args_from(resp: &Resp) -> Vec<String> {
    match resp {
        Resp::Array(items) => items.iter().filter_map(|v| match v {
            Resp::BulkString(Some(s)) | Resp::SimpleString(s) => Some(s.clone()),
            _ => None,
        }).collect(),
        _ => vec![],
    }
}

fn dispatch(args: &[String], store: &mut Store) -> Resp {
    match dispatch_inner(args, store) {
        Ok(r)  => r,
        Err(r) => r,
    }
}

fn dispatch_inner(args: &[String], store: &mut Store) -> Result<Resp, Resp> {
    if args.is_empty() { return Ok(Resp::err("ERR no command")); }
    let cmd = args[0].to_ascii_uppercase();
    Ok(match cmd.as_str() {
        "PING"    => if args.len() > 1 { Resp::bulk(&args[1]) } else { Resp::SimpleString("PONG".into()) },
        "COMMAND" => Resp::ok(),
        "FLUSHDB" => store.flushdb(),
        "KEYS"    => store.keys(args.get(1).map_or("*", |s| s.as_str())),
        "TYPE"    => store.type_cmd(args.get(1).map_or("", |s| s.as_str())),

        "GET"    => {
            if args.len() < 2 { return Err(Resp::err("ERR wrong number of arguments for 'get' command")); }
            store.get(&args[1])
        }
        "SETNX"  => {
            if args.len() < 3 { return Err(Resp::err("ERR wrong number of arguments for 'setnx' command")); }
            store.setnx(args[1].clone(), args[2].clone())
        }

        "SET" => {
            require_min(args, 3)?;
            let mut ttl: Option<Duration> = None;
            let mut i = 3;
            while i < args.len() {
                match args[i].to_ascii_uppercase().as_str() {
                    "EX" => { i += 1; ttl = Some(Duration::from_secs(parse_u64(args.get(i))?)); }
                    "PX" => { i += 1; ttl = Some(Duration::from_millis(parse_u64(args.get(i))?)); }
                    _    => return Ok(Resp::err("ERR syntax error")),
                }
                i += 1;
            }
            store.set(args[1].clone(), args[2].clone(), ttl)
        }

        "MGET" => { require_min(args, 2)?; store.mget(&args[1..]) }
        "MSET" => {
            if args.len() < 3 || (args.len() - 1) % 2 != 0 {
                return Ok(Resp::err("ERR wrong number of arguments for MSET"));
            }
            let pairs: Vec<(String, String)> = args[1..].chunks(2)
                .map(|c| (c[0].clone(), c[1].clone())).collect();
            store.mset(&pairs)
        }

        "DEL"    => { require_min(args, 2)?; store.del(&args[1..]) }
        "EXISTS" => { require_min(args, 2)?; store.exists(&args[1..]) }
        "EXPIRE" => { require(args, 3)?; store.expire(&args[1], parse_u64(args.get(2))?) }
        "TTL"    => { require(args, 2)?; store.ttl(&args[1]) }

        "INCR"   => { require(args, 2)?; store.incr_by(&args[1],  1) }
        "DECR"   => { require(args, 2)?; store.incr_by(&args[1], -1) }
        "INCRBY" => { require(args, 3)?; store.incr_by(&args[1], parse_i64(args.get(2))?) }
        "DECRBY" => { require(args, 3)?; store.incr_by(&args[1], -parse_i64(args.get(2))?) }

        "LPUSH"  => { require_min(args, 3)?; store.lpush(args[1].clone(), &args[2..]) }
        "RPUSH"  => { require_min(args, 3)?; store.rpush(args[1].clone(), &args[2..]) }
        "LRANGE" => {
            require(args, 4)?;
            let s = parse_i64(args.get(2))?;
            let e = parse_i64(args.get(3))?;
            store.lrange(&args[1], s, e)
        }
        "LLEN"   => { require(args, 2)?; store.llen(&args[1]) }
        "LINDEX" => { require(args, 3)?; store.lindex(&args[1], parse_i64(args.get(2))?) }

        // ── Hash commands ──────────────────────────────────────────────────────
        "HSET" => {
            require_min(args, 4)?;
            if (args.len() - 2) % 2 != 0 {
                return Err(Resp::err("ERR wrong number of arguments for 'hset' command"));
            }
            let pairs: Vec<(String, String)> = args[2..].chunks(2)
                .map(|c| (c[0].clone(), c[1].clone()))
                .collect();
            store.hset(&args[1], &pairs)
        }
        "HSETNX" => {
            require(args, 4)?;
            store.hsetnx(&args[1], &args[2], &args[3])
        }
        "HGET"    => { require(args, 3)?; store.hget(&args[1], &args[2]) }
        "HMGET"   => { require_min(args, 3)?; store.hmget(&args[1], &args[2..].to_vec()) }
        "HGETALL" => { require(args, 2)?; store.hgetall(&args[1]) }
        "HDEL"    => { require_min(args, 3)?; store.hdel(&args[1], &args[2..].to_vec()) }
        "HLEN"    => { require(args, 2)?; store.hlen(&args[1]) }
        "HEXISTS" => { require(args, 3)?; store.hexists(&args[1], &args[2]) }
        "HKEYS"   => { require(args, 2)?; store.hkeys(&args[1]) }
        "HVALS"   => { require(args, 2)?; store.hvals(&args[1]) }
        "HINCRBY" => { require(args, 4)?; store.hincrby(&args[1], &args[2], parse_i64(args.get(3))?) }

        _ => Resp::err(format!("ERR unknown command '{cmd}'")),
    })
}

// ── Parse helpers ─────────────────────────────────────────────────────────────

fn require(args: &[String], n: usize) -> Result<(), Resp> {
    if args.len() != n { Err(Resp::err(format!("ERR wrong number of arguments for '{}'", args[0]))) }
    else { Ok(()) }
}
fn require_min(args: &[String], n: usize) -> Result<(), Resp> {
    if args.len() < n { Err(Resp::err(format!("ERR wrong number of arguments for '{}'", args[0]))) }
    else { Ok(()) }
}
fn parse_u64(v: Option<&String>) -> Result<u64, Resp> {
    v.and_then(|s| s.parse().ok())
     .ok_or_else(|| Resp::err("ERR value is not an integer or out of range"))
}
fn parse_i64(v: Option<&String>) -> Result<i64, Resp> {
    v.and_then(|s| s.parse().ok())
     .ok_or_else(|| Resp::err("ERR value is not an integer or out of range"))
}

// ── Server ────────────────────────────────────────────────────────────────────

pub struct MiniRedis {
    pub addr: SocketAddr,
    pub connection_count: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
}

impl MiniRedis {
    /// Start listening. Returns `(server_handle, thread_handle)`.
    pub fn start(addr: &str) -> Result<(Self, JoinHandle<()>), String> {
        let listener = TcpListener::bind(addr).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let actual = listener.local_addr().map_err(|e| e.to_string())?;

        let store  = Arc::new(Mutex::new(Store::default()));
        let conns  = Arc::new(AtomicUsize::new(0));
        let shut   = Arc::new(AtomicBool::new(false));

        let (conns2, shut2, store2) = (conns.clone(), shut.clone(), store.clone());
        let handle = thread::spawn(move || {
            loop {
                if shut2.load(Ordering::Relaxed) { break; }
                match listener.accept() {
                    Ok((stream, _)) => {
                        conns2.fetch_add(1, Ordering::Relaxed);
                        let s = store2.clone();
                        thread::spawn(move || { let _ = serve(stream, s); });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(e) => { eprintln!("miniredis accept: {e}"); thread::sleep(Duration::from_millis(50)); }
                }
            }
        });

        Ok((MiniRedis { addr: actual, connection_count: conns, shutdown: shut }, handle))
    }

    pub fn stop(&self) { self.shutdown.store(true, Ordering::Relaxed); }
    pub fn connections(&self) -> usize { self.connection_count.load(Ordering::Relaxed) }
}

fn serve(stream: TcpStream, store: Arc<Mutex<Store>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        let msg = match parse_resp(&mut reader) {
            Ok(v)  => v,
            Err(_) => break,
        };
        let resp = {
            let args = args_from(&msg);
            let mut g = store.lock().expect("store lock");
            dispatch(&args, &mut g)
        };
        writer.write_all(&resp.serialize())?;
        writer.flush()?;
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> Store { Store::default() }

    #[test] fn set_get() {
        let mut s = st();
        assert_eq!(s.set("k".into(), "v".into(), None), Resp::ok());
        assert_eq!(s.get("k"), Resp::bulk("v"));
    }
    #[test] fn missing_get_null() { assert_eq!(st().get("x"), Resp::null()); }
    #[test] fn del_returns_count() {
        let mut s = st();
        s.set("a".into(), "1".into(), None);
        s.set("b".into(), "2".into(), None);
        assert_eq!(s.del(&["a".into(), "z".into()]), Resp::int(1));
    }
    #[test] fn mget_mset() {
        let mut s = st();
        s.mset(&[("x".into(), "1".into()), ("y".into(), "2".into())]);
        let r = s.mget(&["x".into(), "y".into(), "z".into()]);
        assert_eq!(r, Resp::Array(vec![Resp::bulk("1"), Resp::bulk("2"), Resp::null()]));
    }
    #[test] fn incr_decr() {
        let mut s = st();
        s.set("n".into(), "10".into(), None);
        assert_eq!(s.incr_by("n",  5), Resp::int(15));
        assert_eq!(s.incr_by("n", -3), Resp::int(12));
    }
    #[test] fn incr_new_key() {
        let mut s = st();
        assert_eq!(s.incr_by("c", 1), Resp::int(1));
    }
    #[test] fn setnx_only_if_missing() {
        let mut s = st();
        assert_eq!(s.setnx("k".into(), "a".into()), Resp::int(1));
        assert_eq!(s.setnx("k".into(), "b".into()), Resp::int(0));
        assert_eq!(s.get("k"), Resp::bulk("a"));
    }
    #[test] fn expire_ttl() {
        let mut s = st();
        s.set("k".into(), "v".into(), None);
        assert_eq!(s.expire("k", 60), Resp::int(1));
        match s.ttl("k") { Resp::Integer(n) => assert!(n > 0), _ => panic!() }
    }
    #[test] fn ttl_no_expiry() {
        let mut s = st();
        s.set("k".into(), "v".into(), None);
        assert_eq!(s.ttl("k"), Resp::int(-1));
    }
    #[test] fn ttl_missing() { assert_eq!(st().ttl("x"), Resp::int(-2)); }
    #[test] fn type_string_list() {
        let mut s = st();
        s.set("str".into(), "v".into(), None);
        s.lpush("lst".into(), &["a".into()]);
        assert_eq!(s.type_cmd("str"), Resp::bulk("string"));
        assert_eq!(s.type_cmd("lst"), Resp::bulk("list"));
        assert_eq!(s.type_cmd("none"), Resp::bulk("none"));
    }
    #[test] fn flushdb() {
        let mut s = st();
        s.set("a".into(), "1".into(), None);
        s.flushdb();
        assert_eq!(s.get("a"), Resp::null());
    }
    #[test] fn lpush_rpush_lrange() {
        let mut s = st();
        s.lpush("l".into(), &["b".into(), "a".into()]);
        s.rpush("l".into(), &["c".into()]);
        // list should be: a, b, c
        assert_eq!(
            s.lrange("l", 0, -1),
            Resp::Array(vec![Resp::bulk("a"), Resp::bulk("b"), Resp::bulk("c")])
        );
    }
    #[test] fn llen() {
        let mut s = st();
        s.rpush("l".into(), &["x".into(), "y".into()]);
        assert_eq!(s.llen("l"), Resp::int(2));
    }
    #[test] fn lindex() {
        let mut s = st();
        s.rpush("l".into(), &["a".into(), "b".into(), "c".into()]);
        assert_eq!(s.lindex("l",  0), Resp::bulk("a"));
        assert_eq!(s.lindex("l", -1), Resp::bulk("c"));
        assert_eq!(s.lindex("l",  9), Resp::null());
    }
    #[test] fn glob_patterns() {
        assert!(glob("*", "anything"));
        assert!(glob("user:*", "user:1"));
        assert!(!glob("user:*", "session:1"));
        assert!(glob("*:1", "user:1"));
        assert!(glob("exact", "exact"));
        assert!(!glob("exact", "other"));
    }
    #[test] fn server_start_stop() {
        let (s, h) = MiniRedis::start("127.0.0.1:0").unwrap();
        assert_ne!(s.addr.port(), 0);
        s.stop();
        h.join().unwrap();
    }

    // ── Hash tests ────────────────────────────────────────────────────────

    #[test] fn hset_hget_basic() {
        let mut s = st();
        assert_eq!(s.hset("h", &[("f".into(), "v".into())]), Resp::int(1));
        assert_eq!(s.hget("h", "f"), Resp::bulk("v"));
        assert_eq!(s.hget("h", "missing"), Resp::null());
    }

    #[test] fn hset_multi_fields() {
        let mut s = st();
        let pairs = vec![("a".into(), "1".into()), ("b".into(), "2".into())];
        assert_eq!(s.hset("h", &pairs), Resp::int(2));
        // Overwrite doesn't count as new
        let update = vec![("a".into(), "10".into()), ("c".into(), "3".into())];
        assert_eq!(s.hset("h", &update), Resp::int(1)); // only c is new
        assert_eq!(s.hget("h", "a"), Resp::bulk("10"));
    }

    #[test] fn hsetnx_only_if_missing() {
        let mut s = st();
        assert_eq!(s.hsetnx("h", "f", "first"),  Resp::int(1));
        assert_eq!(s.hsetnx("h", "f", "second"), Resp::int(0));
        assert_eq!(s.hget("h", "f"), Resp::bulk("first"));
    }

    #[test] fn hgetall_sorted() {
        let mut s = st();
        s.hset("h", &[("b".into(), "2".into()), ("a".into(), "1".into())]);
        let r = s.hgetall("h");
        assert_eq!(r, Resp::Array(vec![
            Resp::bulk("a"), Resp::bulk("1"),
            Resp::bulk("b"), Resp::bulk("2"),
        ]));
    }

    #[test] fn hgetall_missing_key() {
        assert_eq!(st().hgetall("nokey"), Resp::Array(vec![]));
    }

    #[test] fn hdel_fields() {
        let mut s = st();
        s.hset("h", &[("a".into(), "1".into()), ("b".into(), "2".into())]);
        assert_eq!(s.hdel("h", &["a".into(), "z".into()]), Resp::int(1));
        assert_eq!(s.hget("h", "a"), Resp::null());
        assert_eq!(s.hget("h", "b"), Resp::bulk("2"));
    }

    #[test] fn hlen_count() {
        let mut s = st();
        assert_eq!(s.hlen("h"), Resp::int(0));
        s.hset("h", &[("a".into(), "1".into()), ("b".into(), "2".into())]);
        assert_eq!(s.hlen("h"), Resp::int(2));
    }

    #[test] fn hexists() {
        let mut s = st();
        s.hset("h", &[("f".into(), "v".into())]);
        assert_eq!(s.hexists("h", "f"),       Resp::int(1));
        assert_eq!(s.hexists("h", "missing"), Resp::int(0));
        assert_eq!(s.hexists("none", "f"),    Resp::int(0));
    }

    #[test] fn hkeys_hvals() {
        let mut s = st();
        s.hset("h", &[("b".into(), "2".into()), ("a".into(), "1".into())]);
        assert_eq!(s.hkeys("h"), Resp::Array(vec![Resp::bulk("a"), Resp::bulk("b")]));
        assert_eq!(s.hvals("h"), Resp::Array(vec![Resp::bulk("1"), Resp::bulk("2")]));
    }

    #[test] fn hmget_partial() {
        let mut s = st();
        s.hset("h", &[("a".into(), "1".into())]);
        let r = s.hmget("h", &["a".into(), "z".into()]);
        assert_eq!(r, Resp::Array(vec![Resp::bulk("1"), Resp::null()]));
    }

    #[test] fn hincrby() {
        let mut s = st();
        assert_eq!(s.hincrby("h", "count",  5),  Resp::int(5));
        assert_eq!(s.hincrby("h", "count",  3),  Resp::int(8));
        assert_eq!(s.hincrby("h", "count", -2),  Resp::int(6));
    }

    #[test] fn hash_type_reported() {
        let mut s = st();
        s.hset("h", &[("f".into(), "v".into())]);
        assert_eq!(s.type_cmd("h"), Resp::bulk("hash"));
    }

    #[test] fn dispatch_hset_hget() {
        let mut s = st();
        let set_args = vec!["HSET", "myhash", "field1", "value1"]
            .into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(dispatch(&set_args, &mut s), Resp::int(1));
        let get_args = vec!["HGET", "myhash", "field1"]
            .into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(dispatch(&get_args, &mut s), Resp::bulk("value1"));
    }
}
