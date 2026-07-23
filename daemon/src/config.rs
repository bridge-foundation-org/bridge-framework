//! `bridge.toml` project configuration — parsed at daemon startup.
//!
//! ## File format
//!
//! ```toml
//! [project]
//! name    = "my-app"
//! version = "0.1.0"
//!
//! [daemon]
//! http_addr  = "127.0.0.1:8787"
//! tcp_addr   = "127.0.0.1:7878"
//! redis_addr = "127.0.0.1:6399"
//! mode       = "full"           # lite | full | ultra | off
//!
//! [watch]
//! enabled  = true
//! poll_ms  = 500
//! dirs     = [".", "services"]
//! files    = ["app.bridge"]
//!
//! [middleware]
//! # Each entry registers one middleware
//! [[middleware.rules]]
//! name   = "logger"
//! scope  = "global"
//! before = "log"
//!
//! [[middleware.rules]]
//! name   = "cors-header"
//! scope  = "global"
//! after  = "header:X-Powered-By:bridge"
//!
//! [ratelimit]
//! [[ratelimit.rules]]
//! method      = "POST"
//! path        = "/api/v1/compile"
//! capacity    = 60
//! refill_rate = 1.0
//!
//! [[ratelimit.rules]]
//! method      = "*"
//! path        = "*"
//! capacity    = 1000
//! refill_rate = 100.0
//! ```
//!
//! All sections are optional.  Missing keys fall back to defaults.
//!
//! ## Design
//!
//! Pure `std` only — no `serde` or `toml` crate.  We hand-parse the file
//! with a minimal line-oriented TOML subset that covers everything above.
//! This keeps the project dependency-free.

// ── Config types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ProjectConfig {
    pub name:    String,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub http_addr:  String,
    pub tcp_addr:   String,
    pub redis_addr: String,
    pub mode:       String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            http_addr:  "127.0.0.1:8787".into(),
            tcp_addr:   "127.0.0.1:7878".into(),
            redis_addr: "127.0.0.1:6399".into(),
            mode:       "full".into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WatchConfig {
    pub enabled: bool,
    pub poll_ms: u64,
    pub dirs:    Vec<String>,
    pub files:   Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MiddlewareRule {
    pub name:   String,
    pub scope:  String,
    pub before: Option<String>,
    pub after:  Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RateLimitRule {
    pub method:      String,
    pub path:        String,
    pub capacity:    u64,
    pub refill_rate: f64,
}

/// The fully-parsed `bridge.toml`.
#[derive(Debug, Clone, Default)]
pub struct BridgeConfig {
    pub project:    ProjectConfig,
    pub daemon:     DaemonConfig,
    pub watch:      WatchConfig,
    pub middleware: Vec<MiddlewareRule>,
    pub ratelimit:  Vec<RateLimitRule>,
}

impl BridgeConfig {
    /// Try to load config from `bridge.toml` in the given directory.
    /// Returns `None` if the file does not exist, `Err` on parse error.
    pub fn load_from_dir(dir: &str) -> Result<Option<Self>, String> {
        let path = format!("{dir}/bridge.toml");
        match std::fs::read_to_string(&path) {
            Ok(contents) => Ok(Some(Self::parse(&contents)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read {path}: {e}")),
        }
    }

    /// Try to load config from explicit path.
    pub fn load(path: &str) -> Result<Option<Self>, String> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(Some(Self::parse(&contents)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read {path}: {e}")),
        }
    }

    /// Parse TOML content into a `BridgeConfig`.
    pub fn parse(content: &str) -> Result<Self, String> {
        let mut cfg = BridgeConfig::default();

        // Defaults
        cfg.daemon  = DaemonConfig::default();
        cfg.watch   = WatchConfig { enabled: true, poll_ms: 500, dirs: vec![], files: vec![] };

        let mut section     = String::new();
        let mut cur_mw:  Option<MiddlewareRule> = None;
        let mut cur_rl:  Option<RateLimitRule>  = None;

        for (lineno, raw_line) in content.lines().enumerate() {
            let lineno = lineno + 1;
            let line   = raw_line.trim();

            // Skip blank lines and comments
            if line.is_empty() || line.starts_with('#') { continue; }

            // Section header: [section] or [[section.sub]]
            if line.starts_with('[') {
                // Flush pending rule before switching section
                if let Some(mw) = cur_mw.take() {
                    if !mw.name.is_empty() { cfg.middleware.push(mw); }
                }
                if let Some(rl) = cur_rl.take() {
                    if rl.capacity > 0 { cfg.ratelimit.push(rl); }
                }

                let s = line.trim_matches(|c| c == '[' || c == ']').trim();
                section = s.to_string();

                // Array-of-tables entry
                if section == "middleware.rules" {
                    cur_mw = Some(MiddlewareRule::default());
                } else if section == "ratelimit.rules" {
                    cur_rl = Some(RateLimitRule { method: "*".into(), path: "*".into(), ..Default::default() });
                }
                continue;
            }

            // Key = value
            let (key, val) = match parse_kv(line) {
                Some(kv) => kv,
                None => return Err(format!("line {lineno}: cannot parse: {line:?}")),
            };

            match section.as_str() {
                "project" => match key {
                    "name"    => cfg.project.name    = val.to_string(),
                    "version" => cfg.project.version = val.to_string(),
                    other => return Err(format!("line {lineno}: unknown key [project].{other}")),
                },
                "daemon" => match key {
                    "http_addr"  => cfg.daemon.http_addr  = val.to_string(),
                    "tcp_addr"   => cfg.daemon.tcp_addr   = val.to_string(),
                    "redis_addr" => cfg.daemon.redis_addr = val.to_string(),
                    "mode"       => cfg.daemon.mode       = val.to_string(),
                    other => return Err(format!("line {lineno}: unknown key [daemon].{other}")),
                },
                "watch" => match key {
                    "enabled" => cfg.watch.enabled = parse_bool(val, lineno)?,
                    "poll_ms" => cfg.watch.poll_ms  = parse_u64(val, lineno)?,
                    "dirs"    => cfg.watch.dirs     = parse_str_array(val),
                    "files"   => cfg.watch.files    = parse_str_array(val),
                    other => return Err(format!("line {lineno}: unknown key [watch].{other}")),
                },
                "middleware.rules" => {
                    let mw = cur_mw.get_or_insert_with(MiddlewareRule::default);
                    match key {
                        "name"   => mw.name   = val.to_string(),
                        "scope"  => mw.scope  = val.to_string(),
                        "before" => mw.before = Some(val.to_string()),
                        "after"  => mw.after  = Some(val.to_string()),
                        other => return Err(format!("line {lineno}: unknown key [[middleware.rules]].{other}")),
                    }
                }
                "ratelimit.rules" => {
                    let rl = cur_rl.get_or_insert_with(|| RateLimitRule {
                        method: "*".into(), path: "*".into(), ..Default::default()
                    });
                    match key {
                        "method"      => rl.method      = val.to_uppercase(),
                        "path"        => rl.path        = val.to_string(),
                        "capacity"    => rl.capacity    = parse_u64(val, lineno)?,
                        "refill_rate" => rl.refill_rate = parse_f64(val, lineno)?,
                        other => return Err(format!("line {lineno}: unknown key [[ratelimit.rules]].{other}")),
                    }
                }
                // Ignore unknown top-level sections silently
                _ => {}
            }
        }

        // Flush last pending rules
        if let Some(mw) = cur_mw {
            if !mw.name.is_empty() { cfg.middleware.push(mw); }
        }
        if let Some(rl) = cur_rl {
            if rl.capacity > 0 { cfg.ratelimit.push(rl); }
        }

        Ok(cfg)
    }

    /// Generate a default `bridge.toml` for a new project with the given name.
    pub fn default_toml(project_name: &str) -> String {
        format!(
r#"# bridge.toml — Bridge Framework project configuration
# https://github.com/bridge-framework

[project]
name    = "{name}"
version = "0.1.0"

[daemon]
http_addr  = "127.0.0.1:8787"
tcp_addr   = "127.0.0.1:7878"
redis_addr = "127.0.0.1:6399"
mode       = "full"            # lite | full | ultra | off

[watch]
enabled = true
poll_ms = 500
dirs    = ["."]                # directories to scan for .bridge files
files   = ["app.bridge"]       # explicit files to watch

# ── Middleware ─────────────────────────────────────────────────────────────────
# Each [[middleware.rules]] entry registers one middleware hook.
# Supported before specs: log | reject:<status>:<message>
# Supported after  specs: log | header:<key>:<value>

[[middleware.rules]]
name   = "powered-by"
scope  = "global"
after  = "header:X-Powered-By:bridge"

# ── Rate limiting ──────────────────────────────────────────────────────────────
# Each [[ratelimit.rules]] entry creates one token-bucket rule.
# method / path support "*" as wildcard.

[[ratelimit.rules]]
method      = "POST"
path        = "/api/v1/compile"
capacity    = 60
refill_rate = 1.0

[[ratelimit.rules]]
method      = "*"
path        = "*"
capacity    = 1000
refill_rate = 100.0
"#,
            name = project_name
        )
    }
}

// ── Apply config to daemon state ──────────────────────────────────────────────

/// Apply a parsed `BridgeConfig` to the daemon's shared state.
/// Call this once at startup after state is created.
pub fn apply(cfg: &BridgeConfig, state: &crate::state::SharedState) {
    let mut g = state.lock().expect("state lock poisoned");

    // Daemon mode
    if let Ok(mode) = protocol::DaemonMode::parse(&cfg.daemon.mode) {
        g.mode = mode;
    }

    // App name
    if !cfg.project.name.is_empty() {
        g.app_name = cfg.project.name.clone();
    }

    // Watch settings
    if cfg.watch.enabled {
        g.watcher.poll_ms = cfg.watch.poll_ms.max(100); // floor at 100ms
        for dir in &cfg.watch.dirs {
            g.watcher.watch_dir(dir);
        }
        for file in &cfg.watch.files {
            g.watcher.watch_file(file);
        }
        g.watcher.running = true;
    }

    // Middleware rules
    for mw_rule in &cfg.middleware {
        use crate::middleware::{MiddlewareBuilder, Scope};

        let scope = match crate::http::parse_scope(&mw_rule.scope) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[bridge] config: middleware {}: bad scope — {e}", mw_rule.name);
                continue;
            }
        };

        let mut builder = MiddlewareBuilder::new(&mw_rule.name).scope(scope);

        if let Some(spec) = &mw_rule.before {
            match crate::http::build_hook_before(spec) {
                Ok(hook) => builder = builder.before(hook),
                Err(e)   => { eprintln!("[bridge] config: middleware {}: before hook — {e}", mw_rule.name); continue; }
            }
        }
        if let Some(spec) = &mw_rule.after {
            match crate::http::build_hook_after(spec) {
                Ok(hook) => builder = builder.after(hook),
                Err(e)   => { eprintln!("[bridge] config: middleware {}: after hook — {e}", mw_rule.name); continue; }
            }
        }

        g.middleware.remove(&mw_rule.name);
        g.middleware.register(builder.build());
    }

    // Rate-limit rules
    for rl_rule in &cfg.ratelimit {
        use crate::ratelimit::BucketKey;
        let key = BucketKey::new(&rl_rule.method, &rl_rule.path);
        g.rate_limiter.add_rule(key, rl_rule.capacity, rl_rule.refill_rate);
    }
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

/// Parse `key = "value"` or `key = value`, returning `(key, unquoted_value)`.
fn parse_kv(line: &str) -> Option<(&str, &str)> {
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    let raw = line[eq+1..].trim();
    // Strip inline comment after value (outside of strings)
    let raw = if raw.starts_with('"') {
        // Quoted string — find closing quote
        let inner = &raw[1..];
        let end   = inner.find('"')?;
        &inner[..end]
    } else {
        // Unquoted — strip trailing comment
        raw.split('#').next().unwrap_or(raw).trim()
    };
    Some((key, raw))
}

/// Parse a TOML inline string array: `["a", "b", "c"]` → `vec!["a","b","c"]`.
fn parse_str_array(s: &str) -> Vec<String> {
    let inner = s.trim().trim_start_matches('[').trim_end_matches(']');
    inner.split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_bool(s: &str, lineno: usize) -> Result<bool, String> {
    match s.trim() {
        "true"  => Ok(true),
        "false" => Ok(false),
        other   => Err(format!("line {lineno}: expected true/false, got {other:?}")),
    }
}

fn parse_u64(s: &str, lineno: usize) -> Result<u64, String> {
    s.trim().parse::<u64>()
        .map_err(|_| format!("line {lineno}: expected integer, got {s:?}"))
}

fn parse_f64(s: &str, lineno: usize) -> Result<f64, String> {
    s.trim().parse::<f64>()
        .map_err(|_| format!("line {lineno}: expected float, got {s:?}"))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> BridgeConfig { BridgeConfig::parse(s).expect("parse failed") }

    // ── Blank / comments ──────────────────────────────────────────────────────

    #[test]
    fn empty_content_produces_defaults() {
        let cfg = parse("");
        assert_eq!(cfg.daemon.http_addr, "127.0.0.1:8787");
        assert_eq!(cfg.daemon.mode,      "full");
        assert!(cfg.watch.enabled);
        assert_eq!(cfg.watch.poll_ms, 500);
    }

    #[test]
    fn comment_only_file() {
        parse("# this is a comment\n# another comment\n");
    }

    // ── [project] ─────────────────────────────────────────────────────────────

    #[test]
    fn project_section_parsed() {
        let cfg = parse("[project]\nname = \"myapp\"\nversion = \"1.2.3\"\n");
        assert_eq!(cfg.project.name,    "myapp");
        assert_eq!(cfg.project.version, "1.2.3");
    }

    // ── [daemon] ─────────────────────────────────────────────────────────────

    #[test]
    fn daemon_section_parsed() {
        let cfg = parse(
            "[daemon]\nhttp_addr = \"0.0.0.0:9090\"\ntcp_addr = \"0.0.0.0:9091\"\nmode = \"lite\"\n"
        );
        assert_eq!(cfg.daemon.http_addr, "0.0.0.0:9090");
        assert_eq!(cfg.daemon.tcp_addr,  "0.0.0.0:9091");
        assert_eq!(cfg.daemon.mode,      "lite");
    }

    // ── [watch] ───────────────────────────────────────────────────────────────

    #[test]
    fn watch_section_parsed() {
        let cfg = parse(
            "[watch]\nenabled = false\npoll_ms = 250\ndirs = [\".\", \"services\"]\nfiles = [\"app.bridge\"]\n"
        );
        assert!(!cfg.watch.enabled);
        assert_eq!(cfg.watch.poll_ms, 250);
        assert_eq!(cfg.watch.dirs,  vec![".", "services"]);
        assert_eq!(cfg.watch.files, vec!["app.bridge"]);
    }

    // ── [[middleware.rules]] ──────────────────────────────────────────────────

    #[test]
    fn middleware_rules_parsed() {
        let src = "[[middleware.rules]]\nname = \"logger\"\nscope = \"global\"\nbefore = \"log\"\n";
        let cfg = parse(src);
        assert_eq!(cfg.middleware.len(), 1);
        assert_eq!(cfg.middleware[0].name,   "logger");
        assert_eq!(cfg.middleware[0].scope,  "global");
        assert_eq!(cfg.middleware[0].before, Some("log".to_string()));
        assert_eq!(cfg.middleware[0].after,  None);
    }

    #[test]
    fn multiple_middleware_rules() {
        let src = concat!(
            "[[middleware.rules]]\nname = \"a\"\nscope = \"global\"\nbefore = \"log\"\n",
            "[[middleware.rules]]\nname = \"b\"\nscope = \"service:users\"\nafter = \"header:X-Foo:bar\"\n",
        );
        let cfg = parse(src);
        assert_eq!(cfg.middleware.len(), 2);
        assert_eq!(cfg.middleware[1].name, "b");
    }

    // ── [[ratelimit.rules]] ───────────────────────────────────────────────────

    #[test]
    fn ratelimit_rules_parsed() {
        let src = "[[ratelimit.rules]]\nmethod = \"POST\"\npath = \"/submit\"\ncapacity = 10\nrefill_rate = 1.0\n";
        let cfg = parse(src);
        assert_eq!(cfg.ratelimit.len(), 1);
        assert_eq!(cfg.ratelimit[0].method,   "POST");
        assert_eq!(cfg.ratelimit[0].path,     "/submit");
        assert_eq!(cfg.ratelimit[0].capacity, 10);
        assert!((cfg.ratelimit[0].refill_rate - 1.0).abs() < 1e-9);
    }

    #[test]
    fn ratelimit_zero_capacity_excluded() {
        // A rule with capacity = 0 should not be added (invalid)
        let src = "[[ratelimit.rules]]\nmethod = \"GET\"\npath = \"/x\"\ncapacity = 0\nrefill_rate = 1.0\n";
        let cfg = parse(src);
        assert_eq!(cfg.ratelimit.len(), 0);
    }

    #[test]
    fn multiple_ratelimit_rules() {
        let src = concat!(
            "[[ratelimit.rules]]\nmethod = \"POST\"\npath = \"/a\"\ncapacity = 5\nrefill_rate = 1.0\n",
            "[[ratelimit.rules]]\nmethod = \"*\"\npath = \"*\"\ncapacity = 1000\nrefill_rate = 100.0\n",
        );
        let cfg = parse(src);
        assert_eq!(cfg.ratelimit.len(), 2);
    }

    // ── Full file round-trip ──────────────────────────────────────────────────

    #[test]
    fn full_default_toml_parses() {
        let toml = BridgeConfig::default_toml("testapp");
        let cfg  = BridgeConfig::parse(&toml).expect("default toml should parse");
        assert_eq!(cfg.project.name, "testapp");
        assert_eq!(cfg.daemon.mode,  "full");
        assert!(cfg.watch.enabled);
        assert!(!cfg.middleware.is_empty(), "default toml should have middleware");
        assert!(!cfg.ratelimit.is_empty(),  "default toml should have ratelimit rules");
    }

    // ── Error cases ───────────────────────────────────────────────────────────

    #[test]
    fn unknown_key_in_daemon_section_is_error() {
        let src = "[daemon]\nunknown_key = \"value\"\n";
        assert!(BridgeConfig::parse(src).is_err());
    }

    #[test]
    fn bad_bool_is_error() {
        let src = "[watch]\nenabled = yes\n";
        assert!(BridgeConfig::parse(src).is_err());
    }

    #[test]
    fn bad_integer_is_error() {
        let src = "[watch]\npoll_ms = notanumber\n";
        assert!(BridgeConfig::parse(src).is_err());
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    #[test]
    fn parse_kv_quoted() {
        let (k, v) = parse_kv("name = \"my-app\"").unwrap();
        assert_eq!(k, "name");
        assert_eq!(v, "my-app");
    }

    #[test]
    fn parse_kv_unquoted() {
        let (k, v) = parse_kv("poll_ms = 500").unwrap();
        assert_eq!(k, "poll_ms");
        assert_eq!(v, "500");
    }

    #[test]
    fn parse_kv_strips_inline_comment() {
        let (k, v) = parse_kv("mode = full # a comment").unwrap();
        assert_eq!(k, "mode");
        assert_eq!(v, "full");
    }

    #[test]
    fn parse_str_array_basic() {
        let v = parse_str_array("[\".\", \"services\"]");
        assert_eq!(v, vec![".", "services"]);
    }

    #[test]
    fn parse_str_array_empty() {
        let v = parse_str_array("[]");
        assert!(v.is_empty());
    }

    // ── load_from_dir ─────────────────────────────────────────────────────────

    #[test]
    fn load_from_nonexistent_dir_returns_none() {
        assert!(BridgeConfig::load_from_dir("/no/such/dir/xyz123").unwrap().is_none());
    }

    #[test]
    fn load_from_dir_reads_file() {
        let dir = std::env::temp_dir().join("bridge_config_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bridge.toml");
        std::fs::write(&path, "[project]\nname = \"loaded-app\"\n").unwrap();
        let cfg = BridgeConfig::load_from_dir(&dir.to_string_lossy())
            .unwrap().unwrap();
        assert_eq!(cfg.project.name, "loaded-app");
        let _ = std::fs::remove_file(path);
    }
}
