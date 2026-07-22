//! TCP protocol server — line-oriented command/response.
//!
//! Each TCP connection:
//!   1. Reads exactly one `\n`-terminated command line.
//!   2. Parses it with `protocol::parse_command`.
//!   3. Executes it against shared state.
//!   4. Writes one response line and closes the connection.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
#[allow(unused_imports)]
use std::sync::Mutex;
use std::thread;

use protocol::{Command, Response, parse_command};

use crate::sqldb;
use crate::state::{SharedState, State};

// ── Server ────────────────────────────────────────────────────────────────────

pub fn run_tcp_server(addr: &str, state: SharedState) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("[bridge] TCP listening on {addr}");
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || { let _ = handle(stream, state); });
            }
            Err(e) => eprintln!("[bridge] TCP accept error: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, state: SharedState) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response = process(line.trim(), state);
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

/// Parse and execute one command line; returns the serialized response string.
pub fn process(input: &str, state: SharedState) -> String {
    match parse_command(input) {
        Ok(cmd) => format!("{}\n", exec(cmd, state)),
        Err(e)  => format!("{}\n", Response::Err(e)),
    }
}

fn exec(cmd: Command, state: SharedState) -> Response {
    let mut g = state.lock().expect("state lock poisoned");
    match cmd {
        // ── Core ──────────────────────────────────────────────────────────
        Command::Ping    => Response::Pong,
        Command::Version => Response::Data(protocol::VERSION.to_string()),
        Command::Health  => Response::Data(g.health_json()),
        Command::Help    => Response::Data(HELP_TEXT.to_string()),
        Command::Stop    => {
            g.mode = protocol::DaemonMode::Off;
            Response::Ok("stopped".into())
        }

        // ── Mode ──────────────────────────────────────────────────────────
        Command::GetMode        => Response::Mode(g.mode.clone()),
        Command::SetMode(mode)  => { g.mode = mode.clone(); Response::Ok(format!("mode={mode}")) }

        // ── Compiler pipeline ─────────────────────────────────────────────
        Command::Compile { source } => compile_source(&source, &mut g),
        Command::CompileFile { path } => {
            match std::fs::read_to_string(&path) {
                Ok(src)  => compile_source(&src, &mut g),
                Err(e)   => Response::Err(format!("cannot read '{path}': {e}")),
            }
        }
        Command::ServicesList => match &g.service_registry {
            None    => Response::Err("no services compiled yet — run COMPILE first".into()),
            Some(f) => {
                let names: Vec<String> = f.services.iter().map(|s| format!(r#""{}""#, s.name)).collect();
                Response::Data(format!("[{}]", names.join(",")))
            }
        },
        Command::RoutesList => match &g.service_registry {
            None    => Response::Data("[]".into()),
            Some(f) => {
                let mut routes = Vec::new();
                for svc in &f.services {
                    for ep in &svc.endpoints {
                        routes.push(format!(
                            r#"{{"service":"{}","name":"{}","method":"{}","path":"{}"}}"#,
                            svc.name, ep.name, ep.method.as_str(), ep.path
                        ));
                    }
                }
                Response::Data(format!("[{}]", routes.join(",")))
            }
        },

        // ── KV store ──────────────────────────────────────────────────────
        Command::DbPut { ns, key, value } => { g.store.put(&ns, &key, value); Response::Ok("stored".into()) }
        Command::DbGet { ns, key } => match g.store.get(&ns, &key) {
            Some(v) => Response::Data(v),
            None    => Response::Err("not found".into()),
        },
        Command::DbDel { ns, key } => {
            let removed = g.store.del(&ns, &key);
            Response::Ok(if removed { "deleted" } else { "not found" }.into())
        }
        Command::DbKeys { ns } => {
            let keys = g.store.keys(&ns);
            let json: Vec<String> = keys.iter().map(|k| format!(r#""{k}""#)).collect();
            Response::Data(format!("[{}]", json.join(",")))
        }
        Command::DbFlush { ns } => { g.store.flush_ns(&ns); Response::Ok("flushed".into()) }

        // ── Postgres (drop lock before Docker I/O) ────────────────────────
        Command::PgCreate { name } => { drop(g); pg_result(sqldb::create(&name)) }
        Command::PgStatus         => { drop(g); pg_result(sqldb::status()) }
        Command::PgMigrate { sql }=> { drop(g); pg_result(sqldb::migrate(&sql)) }
        Command::PgDestroy { name }=>{ drop(g); pg_result(sqldb::destroy(&name)) }

        // ── Redis ─────────────────────────────────────────────────────────
        Command::RedisStatus => {
            let addr  = g.redis_addr.clone().unwrap_or_else(|| "not running".into());
            let conns = g.redis_connections_count();
            Response::Data(format!(r#"{{"addr":"{addr}","connections":{conns}}}"#))
        }
        Command::RedisPing  => Response::Data("pong".into()),
        Command::RedisFlush => {
            // Flush via TCP to miniredis if available
            if let Some(addr) = &g.redis_addr.clone() {
                drop(g);
                redis_cmd(addr, "*1\r\n$7\r\nFLUSHDB\r\n")
            } else {
                Response::Err("miniredis not running".into())
            }
        }
        Command::RedisGet { key } => {
            let addr = g.redis_addr.clone();
            drop(g);
            match addr {
                Some(a) => redis_cmd(&a, &format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key)),
                None    => Response::Err("miniredis not running".into()),
            }
        }
        Command::RedisSet { key, value } => {
            let addr = g.redis_addr.clone();
            drop(g);
            match addr {
                Some(a) => redis_cmd(&a, &format!(
                    "*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n",
                    key.len(), key, value.len(), value
                )),
                None => Response::Err("miniredis not running".into()),
            }
        }
        Command::RedisSetEx { key, seconds, value } => {
            let addr = g.redis_addr.clone();
            drop(g);
            let secs_str = seconds.to_string();
            match addr {
                Some(a) => redis_cmd(&a, &format!(
                    "*5\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n$2\r\nEX\r\n${}\r\n{}\r\n",
                    key.len(), key, value.len(), value, secs_str.len(), secs_str
                )),
                None => Response::Err("miniredis not running".into()),
            }
        }
        Command::RedisDel { key } => {
            let addr = g.redis_addr.clone();
            drop(g);
            match addr {
                Some(a) => redis_cmd(&a, &format!("*2\r\n$3\r\nDEL\r\n${}\r\n{}\r\n", key.len(), key)),
                None    => Response::Err("miniredis not running".into()),
            }
        }
        Command::RedisKeys { pattern } => {
            let addr = g.redis_addr.clone();
            drop(g);
            match addr {
                Some(a) => redis_cmd(&a, &format!("*2\r\n$4\r\nKEYS\r\n${}\r\n{}\r\n", pattern.len(), pattern)),
                None    => Response::Err("miniredis not running".into()),
            }
        }
        Command::RedisTtl { key } => {
            let addr = g.redis_addr.clone();
            drop(g);
            match addr {
                Some(a) => redis_cmd(&a, &format!("*2\r\n$3\r\nTTL\r\n${}\r\n{}\r\n", key.len(), key)),
                None    => Response::Err("miniredis not running".into()),
            }
        }
        Command::RedisExpire { key, seconds } => {
            let addr = g.redis_addr.clone();
            drop(g);
            let secs = seconds.to_string();
            match addr {
                Some(a) => redis_cmd(&a, &format!(
                    "*3\r\n$6\r\nEXPIRE\r\n${}\r\n{}\r\n${}\r\n{}\r\n",
                    key.len(), key, secs.len(), secs
                )),
                None    => Response::Err("miniredis not running".into()),
            }
        }

        // ── Auth ──────────────────────────────────────────────────────────
        Command::AuthStatus => {
            let set = g.auth_token.is_some();
            Response::Data(format!(r#"{{"configured":{set}}}"#))
        }
        Command::AuthSet { scheme, token } => { 
            g.auth_token = Some(token); 
            Response::Ok(format!("auth token set (scheme: {})", scheme.as_str())) 
        }
        Command::AuthClear         => { g.auth_token = None; Response::Ok("auth token cleared".into()) }

        // ── Traces ────────────────────────────────────────────────────────
        Command::TraceList { limit, filter: _ } => {
            let traces: Vec<String> = g.traces.iter()
                .take(limit.unwrap_or(usize::MAX))
                .map(|t| t.to_json())
                .collect();
            Response::Data(format!("[{}]", traces.join(",")))
        }
        Command::TraceGet { id } => match g.find_trace(&id) {
            Some(t) => Response::Data(t.to_json()),
            None    => Response::Err(format!("trace '{id}' not found")),
        },
        Command::TraceClear => { g.traces.clear(); Response::Ok("traces cleared".into()) }

        // ── Metrics ───────────────────────────────────────────────────────────
        Command::TraceExport { format: _ } => {
            let json: Vec<String> = g.traces.iter().map(|t| t.to_json()).collect();
            Response::Data(format!("[{}]", json.join(",")))
        }
        Command::MetricsList => Response::Data(g.metrics.to_json()),
        Command::MetricsGet { name } => {
            let count = g.metrics.request_counts.get(&name).copied().unwrap_or(0);
            let errs  = g.metrics.error_counts.get(&name).copied().unwrap_or(0);
            Response::Data(format!(r#"{{"endpoint":"{name}","requests":{count},"errors":{errs}}}"#))
        }
        Command::MetricsClear => { g.metrics = Default::default(); Response::Ok("metrics cleared".into()) }
        Command::MetricsExport { format: _ } => Response::Data(g.metrics.to_json()),

        // ── Pub/Sub ───────────────────────────────────────────────────────
        Command::PubSubPublish { topic, payload } => {
            let msg = crate::pubsub::Message::new(&topic, payload);
            let seq = g.pubsub.publish(msg);
            Response::Ok(format!("published seq={seq} topic={topic}"))
        }
        Command::PubSubSubscribe { topic, subscriber } => {
            g.pubsub.subscribe(&topic, &subscriber, crate::pubsub::SubscriptionConfig::default());
            Response::Ok(format!("subscribed {subscriber} to {topic}"))
        }
        Command::PubSubPull { topic, subscriber } => {
            match g.pubsub.pull(&topic, &subscriber) {
                Some(msg) => Response::Data(msg.to_json()),
                None      => Response::Ok("no messages".into()),
            }
        }
        Command::PubSubAck { msg_id } => {
            if g.pubsub.ack(&msg_id) {
                Response::Ok(format!("acked {msg_id}"))
            } else {
                Response::Err(format!("message '{msg_id}' not found"))
            }
        }
        Command::PubSubNack { msg_id, reason } => {
            g.pubsub.nack(&msg_id, &reason);
            Response::Ok(format!("nacked {msg_id}"))
        }
        Command::PubSubStatus => Response::Data(g.pubsub.status_json()),

        // ── Secrets ───────────────────────────────────────────────────────
        Command::SecretSet { name, value } => {
            g.secrets.register_inline(&name, &value);
            Response::Ok(format!("secret '{name}' set"))
        }
        Command::SecretGet { name } => {
            match g.secrets.get(&name) {
                Some(_) => Response::Ok(format!("secret '{name}' is set")),
                None    => Response::Err(format!("secret '{name}' not found")),
            }
        }
        Command::SecretDelete { name } => {
            if g.secrets.delete(&name) {
                Response::Ok(format!("secret '{name}' deleted"))
            } else {
                Response::Err(format!("secret '{name}' not found"))
            }
        }
        Command::SecretList   => Response::Data(g.secrets.list_json()),
        Command::SecretCheck { names } => {
            let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            let missing = g.secrets.check_required(&refs);
            if missing.is_empty() {
                Response::Ok("all secrets present".into())
            } else {
                Response::Err(format!("missing secrets: {}", missing.join(", ")))
            }
        }

        // ── Streaming ─────────────────────────────────────────────────────
        Command::StreamList   => Response::Data(g.streams.endpoints_json()),
        Command::StreamStatus => {
            let active = g.streams.active_count();
            Response::Data(format!(r#"{{"active_streams":{active}}}"#))
        }
        Command::StreamOpen { path } => {
            match g.streams.open(&path) {
                Some(id) => { g.streams.set_open(&id); Response::Ok(format!("stream opened id={id}")) }
                None     => Response::Err(format!("no stream endpoint at '{path}'")),
            }
        }
        Command::StreamClose { id } => {
            g.streams.close(&id);
            Response::Ok(format!("stream {id} closed"))
        }
    }
}

// ── Compiler helper ───────────────────────────────────────────────────────────

fn compile_source(source: &str, g: &mut State) -> Response {
    match compiler::parse(source) {
        Err(e) => Response::Err(e),
        Ok(file) => {
            let ts = codegen::generate_typescript(&file);
            // Cache in KV store by first service name and as "latest"
            if let Some(first) = file.services.first() {
                g.store.put("codegen", &first.name, ts.clone());
            }
            g.store.put("codegen", "latest", ts.clone());
            g.service_registry = Some(file);
            Response::Data(ts)
        }
    }
}

// ── Redis passthrough helper ──────────────────────────────────────────────────

fn redis_cmd(addr: &str, cmd: &str) -> Response {
    use std::io::Read;
    use std::net::TcpStream;
    let mut stream = match TcpStream::connect(addr) {
        Ok(s)  => s,
        Err(e) => return Response::Err(format!("cannot connect to miniredis: {e}")),
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let _ = stream.write_all(cmd.as_bytes());
    let _ = stream.flush();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    // Convert RESP to Data/Ok/Error
    let trimmed = buf.trim_end_matches(['\r', '\n']);
    if trimmed.starts_with('+') {
        Response::Ok(trimmed[1..].to_string())
    } else if trimmed.starts_with('-') {
        Response::Err(trimmed[1..].to_string())
    } else if trimmed.starts_with(':') {
        Response::Data(trimmed[1..].to_string())
    } else {
        Response::Data(trimmed.to_string())
    }
}

// ── Misc helpers ──────────────────────────────────────────────────────────────

fn pg_result(r: Result<String, String>) -> Response {
    match r {
        Ok(msg)  => Response::Ok(msg),
        Err(msg) => Response::Err(msg),
    }
}

const HELP_TEXT: &str = "\
Bridge Framework daemon commands:\n\
\n\
  Core commands: PING | VERSION | HEALTH | STOP\n\
  Mode commands: MODE GET | MODE SET <lite|full|ultra|off>\n\
  Compiler: COMPILE <source> | COMPILE FILE <path>\n\
  Services: SERVICES LIST | ROUTES LIST\n\
  KV Store: DB PUT <ns> <key> <value> | DB GET <ns> <key> | DB DEL <ns> <key>\n\
           DB KEYS <ns> | DB FLUSH <ns>\n\
  Postgres: PG CREATE <name> | PG STATUS | PG MIGRATE <sql> | PG DESTROY <name>\n\
  Redis:    REDIS STATUS | REDIS PING | REDIS GET <k> | REDIS SET <k> <v>\n\
           REDIS SETEX <k> <secs> <v> | REDIS DEL <k> | REDIS KEYS <pat>\n\
           REDIS TTL <k> | REDIS EXPIRE <k> <secs> | REDIS FLUSH\n\
  Auth:     AUTH STATUS | AUTH SET <scheme> <token> | AUTH CLEAR\n\
  Traces:   TRACE LIST [limit] | TRACE GET <id> | TRACE CLEAR | TRACE EXPORT <fmt>\n\
  Metrics:  METRICS LIST | METRICS GET <name> | METRICS CLEAR | METRICS EXPORT <fmt>";

// ── Unit tests (no network) ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SharedState {
        Arc::new(Mutex::new(State::new(None, None)))
    }

    #[test] fn ping() { assert_eq!(process("PING", state()).trim(), "PONG"); }
    #[test] fn mode_get() { assert!(process("MODE GET", state()).starts_with("MODE ")); }
    #[test] fn mode_set() { assert!(process("MODE SET lite", state()).starts_with("OK ")); }
    #[test] fn version()  { assert!(process("VERSION", state()).starts_with("DATA ")); }
    #[test] fn health()   { assert!(process("HEALTH", state()).starts_with("DATA ")); }

    #[test]
    fn compile_generates_typescript() {
        let s = state();
        let src = "service%20hello%0Aendpoint%20ping%20GET%20/ping";
        let r = process(&format!("COMPILE {src}"), Arc::clone(&s));
        assert!(r.starts_with("DATA "), "got: {r}");
    }

    #[test]
    fn services_list_after_compile() {
        let s = state();
        process("COMPILE service%20hello%0Aendpoint%20ping%20GET%20/ping", Arc::clone(&s));
        let r = process("SERVICES LIST", Arc::clone(&s));
        assert!(r.starts_with("DATA "), "got: {r}");
        assert!(r.contains("hello"), "expected service name 'hello', got: {r}");
    }

    #[test]
    fn routes_list_after_compile() {
        let s = state();
        process("COMPILE service%20hello%0Aendpoint%20ping%20GET%20/ping", Arc::clone(&s));
        let r = process("ROUTES LIST", Arc::clone(&s));
        assert!(r.starts_with("DATA "), "got: {r}");
        // /ping → %2Fping in percent-encoded DATA payload
        assert!(r.contains("ping"), "expected route 'ping', got: {r}");
    }

    #[test]
    fn db_put_get_del() {
        let s = state();
        assert!(process("DB PUT ns key hello%20world", Arc::clone(&s)).starts_with("OK"));
        let r = process("DB GET ns key", Arc::clone(&s));
        assert!(r.contains("hello%20world") || r.contains("hello world"), "got: {r}");
        assert!(process("DB DEL ns key", Arc::clone(&s)).starts_with("OK"));
        assert!(process("DB GET ns key", Arc::clone(&s)).starts_with("ERR"));
    }

    #[test]
    fn auth_set_status_clear() {
        let s = state();
        process("AUTH SET my-secret", Arc::clone(&s));
        let r = process("AUTH STATUS", Arc::clone(&s));
        assert!(r.starts_with("DATA "), "got: {r}");
        assert!(r.contains("true"), "got: {r}");
        process("AUTH CLEAR", Arc::clone(&s));
        let r2 = process("AUTH STATUS", Arc::clone(&s));
        assert!(r2.starts_with("DATA "), "got: {r2}");
        assert!(r2.contains("false"), "got: {r2}");
    }

    #[test]
    fn trace_lifecycle() {
        let s = state();
        {
            let mut g = s.lock().unwrap();
            g.push_trace("GET", "/ping", 200, 3);
        }
        let r = process("TRACE LIST", Arc::clone(&s));
        assert!(r.starts_with("DATA "), "got: {r}");
        assert!(r.contains("GET"), "got: {r}");
        process("TRACE CLEAR", Arc::clone(&s));
        let r2 = process("TRACE LIST", Arc::clone(&s));
        // After clear: DATA %5B%5D (percent-encoded [])
        assert!(r2.starts_with("DATA "), "got: {r2}");
    }
}
