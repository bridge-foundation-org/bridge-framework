//! Middleware system — composable request/response interceptors.
//!
//! ## Design
//!
//! A `Middleware` is a named hook-pair: `before` runs before the handler
//! receives the request; `after` runs after the response is produced.
//! Middlewares are stored in a `MiddlewareRegistry` and executed as a chain.
//!
//! ```text
//! request → [before₁] → [before₂] → handler → [after₂] → [after₁] → response

#![allow(dead_code)]
//! ```
//!
//! ### Scoping
//!
//! - **Global** — applied to every request.
//! - **Service** — applied to every endpoint of a named service.
//! - **Endpoint** — applied to one specific `METHOD /path` pattern.
//!
//! ### Context
//!
//! `MiddlewareContext` is threaded through the chain and carries mutable
//! metadata (extra headers, request tags, early-exit status).  Any middleware
//! can write to it; the HTTP server reads the final result to produce the
//! real HTTP response.
//!
//! ### Early exit
//!
//! A `before` hook can call `ctx.reject(status, body)` to short-circuit the
//! entire chain and return an error response immediately — the handler and all
//! remaining `before`/`after` hooks are skipped.

use std::collections::HashMap;

// ── Context ───────────────────────────────────────────────────────────────────

/// Mutable context passed through every middleware in the chain.
#[derive(Debug, Clone)]
pub struct MiddlewareContext {
    /// HTTP method of the incoming request.
    pub method: String,
    /// Request path (without query string).
    pub path: String,
    /// Request ID assigned by the HTTP server.
    pub request_id: String,
    /// Extra response headers injected by middleware.
    pub extra_headers: HashMap<String, String>,
    /// Arbitrary string tags attached by middleware (for tracing/logging).
    pub tags: Vec<String>,
    /// When `Some`, the chain is short-circuited with this (status, body).
    pub rejection: Option<(u16, String)>,
}

impl MiddlewareContext {
    pub fn new(method: &str, path: &str, request_id: &str) -> Self {
        Self {
            method:        method.to_string(),
            path:          path.to_string(),
            request_id:    request_id.to_string(),
            extra_headers: HashMap::new(),
            tags:          Vec::new(),
            rejection:     None,
        }
    }

    /// Short-circuit the chain with `status` and a JSON error body.
    pub fn reject(&mut self, status: u16, body: impl Into<String>) {
        self.rejection = Some((status, body.into()));
    }

    /// Returns true if a middleware has already rejected this request.
    pub fn is_rejected(&self) -> bool {
        self.rejection.is_some()
    }

    /// Attach a tag for observability.
    pub fn tag(&mut self, t: impl Into<String>) {
        self.tags.push(t.into());
    }

    /// Set an extra response header.
    pub fn set_header(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.extra_headers.insert(key.into(), value.into());
    }
}

// ── Hook type ─────────────────────────────────────────────────────────────────

/// A middleware hook: `fn(&mut MiddlewareContext)`.
/// Using a boxed trait object lets us store closures with captured state.
pub type Hook = Box<dyn Fn(&mut MiddlewareContext) + Send + Sync + 'static>;

// ── Scope ─────────────────────────────────────────────────────────────────────

/// Where a middleware applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Applied to all requests.
    Global,
    /// Applied to all endpoints of a specific service name.
    Service(String),
    /// Applied to one method+path pair, e.g. `("GET", "/users")`.
    Endpoint { method: String, path: String },
}

impl Scope {
    /// Returns true if this scope matches the given method+path.
    pub fn matches(&self, method: &str, path: &str) -> bool {
        match self {
            Scope::Global => true,
            Scope::Service(name) => {
                // Match if path starts with "/<service_name>" (case-insensitive)
                let prefix = format!("/{}", name.to_lowercase());
                path.to_lowercase().starts_with(&prefix)
            }
            Scope::Endpoint { method: m, path: p } => {
                m.eq_ignore_ascii_case(method) && p == path
            }
        }
    }
}

// ── Middleware entry ──────────────────────────────────────────────────────────

/// A named, scoped pair of optional hooks.
pub struct MiddlewareEntry {
    pub name:   String,
    pub scope:  Scope,
    /// Runs before the handler. Called in registration order.
    pub before: Option<Hook>,
    /// Runs after the handler.  Called in reverse registration order.
    pub after:  Option<Hook>,
}

impl std::fmt::Debug for MiddlewareEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiddlewareEntry")
            .field("name", &self.name)
            .field("scope", &self.scope)
            .field("before", &self.before.is_some())
            .field("after",  &self.after.is_some())
            .finish()
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Fluent builder for constructing a `MiddlewareEntry`.
pub struct MiddlewareBuilder {
    name:   String,
    scope:  Scope,
    before: Option<Hook>,
    after:  Option<Hook>,
}

impl MiddlewareBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), scope: Scope::Global, before: None, after: None }
    }

    pub fn scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    pub fn before<F>(mut self, f: F) -> Self
    where F: Fn(&mut MiddlewareContext) + Send + Sync + 'static {
        self.before = Some(Box::new(f));
        self
    }

    pub fn after<F>(mut self, f: F) -> Self
    where F: Fn(&mut MiddlewareContext) + Send + Sync + 'static {
        self.after = Some(Box::new(f));
        self
    }

    pub fn build(self) -> MiddlewareEntry {
        MiddlewareEntry { name: self.name, scope: self.scope, before: self.before, after: self.after }
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Stores all registered middlewares and executes them as a chain.
#[derive(Debug, Default)]
pub struct MiddlewareRegistry {
    entries: Vec<MiddlewareEntry>,
}

impl MiddlewareRegistry {
    pub fn new() -> Self { Self::default() }

    /// Register a middleware entry.  Returns its index.
    pub fn register(&mut self, entry: MiddlewareEntry) -> usize {
        self.entries.push(entry);
        self.entries.len() - 1
    }

    /// Remove a middleware by name.  Returns true if one was found.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.name != name);
        self.entries.len() < before
    }

    /// Names of all registered middlewares.
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    /// How many middlewares match a given method+path.
    pub fn count_matching(&self, method: &str, path: &str) -> usize {
        self.entries.iter().filter(|e| e.scope.matches(method, path)).count()
    }

    // ── Chain execution ───────────────────────────────────────────────────────

    /// Run all `before` hooks that match `ctx.method`/`ctx.path`.
    /// Stops on the first rejection.
    pub fn run_before(&self, ctx: &mut MiddlewareContext) {
        for entry in &self.entries {
            if ctx.is_rejected() { break; }
            if entry.scope.matches(&ctx.method, &ctx.path) {
                if let Some(hook) = &entry.before {
                    hook(ctx);
                }
            }
        }
    }

    /// Run all `after` hooks that match `ctx.method`/`ctx.path`,
    /// in reverse registration order.
    pub fn run_after(&self, ctx: &mut MiddlewareContext) {
        for entry in self.entries.iter().rev() {
            if entry.scope.matches(&ctx.method, &ctx.path) {
                if let Some(hook) = &entry.after {
                    hook(ctx);
                }
            }
        }
    }

    /// Convenience: run full before→after cycle without a real handler.
    /// Used for testing; `handler_status` simulates the handler's HTTP status.
    pub fn run_full(&self, ctx: &mut MiddlewareContext) {
        self.run_before(ctx);
        self.run_after(ctx);
    }

    /// Serialize to JSON for the `/api/v1/middleware` endpoint.
    pub fn to_json(&self) -> String {
        let items: Vec<String> = self.entries.iter().map(|e| {
            let scope_str = match &e.scope {
                Scope::Global => "global".to_string(),
                Scope::Service(s) => format!("service:{s}"),
                Scope::Endpoint { method, path } => format!("{method}:{path}"),
            };
            format!(
                r#"{{"name":"{name}","scope":"{scope}","before":{before},"after":{after}}}"#,
                name   = e.name,
                scope  = scope_str,
                before = e.before.is_some(),
                after  = e.after.is_some(),
            )
        }).collect();
        format!("[{}]", items.join(","))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(method: &str, path: &str) -> MiddlewareContext {
        MiddlewareContext::new(method, path, "req-test")
    }

    // ── Scope matching ────────────────────────────────────────────────────────

    #[test]
    fn global_scope_matches_anything() {
        let s = Scope::Global;
        assert!(s.matches("GET", "/anything"));
        assert!(s.matches("DELETE", "/api/v1/deep/path"));
    }

    #[test]
    fn service_scope_matches_prefix() {
        let s = Scope::Service("users".into());
        assert!(s.matches("GET",  "/users"));
        assert!(s.matches("POST", "/users/create"));
        assert!(s.matches("GET",  "/Users/123")); // case-insensitive
        assert!(!s.matches("GET", "/posts"));
    }

    #[test]
    fn endpoint_scope_exact_match() {
        let s = Scope::Endpoint { method: "GET".into(), path: "/ping".into() };
        assert!(s.matches("GET",  "/ping"));
        assert!(s.matches("get",  "/ping")); // method case-insensitive
        assert!(!s.matches("POST", "/ping"));
        assert!(!s.matches("GET",  "/pong"));
    }

    // ── Builder ───────────────────────────────────────────────────────────────

    #[test]
    fn builder_creates_entry() {
        let entry = MiddlewareBuilder::new("logger")
            .scope(Scope::Global)
            .before(|ctx| ctx.tag("logged"))
            .after(|ctx|  ctx.set_header("X-Logged", "true"))
            .build();
        assert_eq!(entry.name, "logger");
        assert!(entry.before.is_some());
        assert!(entry.after.is_some());
    }

    // ── Registry ──────────────────────────────────────────────────────────────

    #[test]
    fn register_and_names() {
        let mut reg = MiddlewareRegistry::new();
        reg.register(MiddlewareBuilder::new("a").build());
        reg.register(MiddlewareBuilder::new("b").build());
        let names = reg.names();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn remove_by_name() {
        let mut reg = MiddlewareRegistry::new();
        reg.register(MiddlewareBuilder::new("x").build());
        reg.register(MiddlewareBuilder::new("y").build());
        assert!(reg.remove("x"));
        assert_eq!(reg.names(), vec!["y"]);
        assert!(!reg.remove("x")); // already gone
    }

    #[test]
    fn count_matching() {
        let mut reg = MiddlewareRegistry::new();
        reg.register(MiddlewareBuilder::new("global").scope(Scope::Global).build());
        reg.register(MiddlewareBuilder::new("users_only")
            .scope(Scope::Service("users".into())).build());
        assert_eq!(reg.count_matching("GET", "/health"), 1); // only global
        assert_eq!(reg.count_matching("GET", "/users"),  2); // global + users_only
    }

    // ── Before hooks ─────────────────────────────────────────────────────────

    #[test]
    fn before_hooks_run_in_order() {
        let mut reg = MiddlewareRegistry::new();
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        let o1 = std::sync::Arc::clone(&order);
        reg.register(MiddlewareBuilder::new("first")
            .before(move |_| { o1.lock().unwrap().push("first".into()); })
            .build());

        let o2 = std::sync::Arc::clone(&order);
        reg.register(MiddlewareBuilder::new("second")
            .before(move |_| { o2.lock().unwrap().push("second".into()); })
            .build());

        let mut c = ctx("GET", "/anything");
        reg.run_before(&mut c);
        assert_eq!(*order.lock().unwrap(), vec!["first", "second"]);
    }

    #[test]
    fn before_hook_can_tag_context() {
        let mut reg = MiddlewareRegistry::new();
        reg.register(MiddlewareBuilder::new("tagger")
            .before(|ctx| ctx.tag("hello"))
            .build());
        let mut c = ctx("GET", "/x");
        reg.run_before(&mut c);
        assert_eq!(c.tags, vec!["hello"]);
    }

    #[test]
    fn before_hook_can_set_header() {
        let mut reg = MiddlewareRegistry::new();
        reg.register(MiddlewareBuilder::new("header_injector")
            .before(|ctx| ctx.set_header("X-Foo", "bar"))
            .build());
        let mut c = ctx("GET", "/x");
        reg.run_before(&mut c);
        assert_eq!(c.extra_headers.get("X-Foo").map(|s| s.as_str()), Some("bar"));
    }

    // ── After hooks ──────────────────────────────────────────────────────────

    #[test]
    fn after_hooks_run_in_reverse_order() {
        let mut reg = MiddlewareRegistry::new();
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        let o1 = std::sync::Arc::clone(&order);
        reg.register(MiddlewareBuilder::new("outer")
            .after(move |_| { o1.lock().unwrap().push("outer".into()); })
            .build());

        let o2 = std::sync::Arc::clone(&order);
        reg.register(MiddlewareBuilder::new("inner")
            .after(move |_| { o2.lock().unwrap().push("inner".into()); })
            .build());

        let mut c = ctx("POST", "/x");
        reg.run_after(&mut c);
        // inner registered last → runs first in after-chain
        assert_eq!(*order.lock().unwrap(), vec!["inner", "outer"]);
    }

    // ── Rejection / short-circuit ─────────────────────────────────────────────

    #[test]
    fn rejection_stops_chain() {
        let mut reg = MiddlewareRegistry::new();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(false));

        reg.register(MiddlewareBuilder::new("rejecter")
            .before(|ctx| ctx.reject(403, r#"{"error":"forbidden"}"#))
            .build());

        let ran2 = std::sync::Arc::clone(&ran);
        reg.register(MiddlewareBuilder::new("should_not_run")
            .before(move |_| { *ran2.lock().unwrap() = true; })
            .build());

        let mut c = ctx("GET", "/secret");
        reg.run_before(&mut c);
        assert!(c.is_rejected());
        assert_eq!(c.rejection.as_ref().unwrap().0, 403);
        assert!(!*ran.lock().unwrap(), "second middleware should not have run");
    }

    #[test]
    fn rejection_has_correct_body() {
        let mut reg = MiddlewareRegistry::new();
        reg.register(MiddlewareBuilder::new("guard")
            .before(|ctx| ctx.reject(401, r#"{"error":"unauthenticated"}"#))
            .build());
        let mut c = ctx("GET", "/secret");
        reg.run_before(&mut c);
        let (status, body) = c.rejection.unwrap();
        assert_eq!(status, 401);
        assert!(body.contains("unauthenticated"));
    }

    // ── Scope filtering ───────────────────────────────────────────────────────

    #[test]
    fn service_scoped_middleware_skipped_on_other_paths() {
        let mut reg = MiddlewareRegistry::new();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(false));
        let ran2 = std::sync::Arc::clone(&ran);
        reg.register(MiddlewareBuilder::new("users_only")
            .scope(Scope::Service("users".into()))
            .before(move |_| { *ran2.lock().unwrap() = true; })
            .build());
        let mut c = ctx("GET", "/health");
        reg.run_before(&mut c);
        assert!(!*ran.lock().unwrap());
    }

    #[test]
    fn endpoint_scoped_middleware_only_runs_on_exact_match() {
        let mut reg = MiddlewareRegistry::new();
        let count = std::sync::Arc::new(std::sync::Mutex::new(0u32));
        let c2 = std::sync::Arc::clone(&count);
        reg.register(MiddlewareBuilder::new("exact")
            .scope(Scope::Endpoint { method: "POST".into(), path: "/items".into() })
            .before(move |_| { *c2.lock().unwrap() += 1; })
            .build());

        let mut c = ctx("POST", "/items");
        reg.run_before(&mut c);
        assert_eq!(*count.lock().unwrap(), 1);

        let mut c = ctx("GET", "/items");
        reg.run_before(&mut c);
        assert_eq!(*count.lock().unwrap(), 1); // not incremented

        let mut c = ctx("POST", "/other");
        reg.run_before(&mut c);
        assert_eq!(*count.lock().unwrap(), 1); // not incremented
    }

    // ── JSON serialization ────────────────────────────────────────────────────

    #[test]
    fn to_json_contains_names_and_scopes() {
        let mut reg = MiddlewareRegistry::new();
        reg.register(MiddlewareBuilder::new("logger").scope(Scope::Global)
            .before(|_| {}).build());
        reg.register(MiddlewareBuilder::new("ratelimit")
            .scope(Scope::Service("api".into()))
            .before(|_| {}).after(|_| {}).build());
        let json = reg.to_json();
        assert!(json.contains("logger"));
        assert!(json.contains("global"));
        assert!(json.contains("ratelimit"));
        assert!(json.contains("service:api"));
    }

    // ── Full cycle ────────────────────────────────────────────────────────────

    #[test]
    fn full_before_after_cycle() {
        let mut reg = MiddlewareRegistry::new();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        let l1 = std::sync::Arc::clone(&log);
        let l2 = std::sync::Arc::clone(&log);
        reg.register(MiddlewareBuilder::new("wrap")
            .before(move |ctx| {
                l1.lock().unwrap().push(format!("before:{}", ctx.path));
                ctx.set_header("X-Before", "1");
            })
            .after(move |ctx| {
                l2.lock().unwrap().push(format!("after:{}", ctx.path));
                ctx.set_header("X-After", "1");
            })
            .build());

        let mut c = ctx("GET", "/ping");
        reg.run_full(&mut c);
        let entries = log.lock().unwrap().clone();
        assert_eq!(entries, vec!["before:/ping", "after:/ping"]);
        assert!(c.extra_headers.contains_key("X-Before"));
        assert!(c.extra_headers.contains_key("X-After"));
    }
}
