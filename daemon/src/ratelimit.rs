//! Rate limiting — per-endpoint token-bucket throttling.
//!
//! ## Algorithm: Token Bucket
//!
//! Each `BucketKey` (method + path pattern) has its own `TokenBucket`:
//! - Starts full at `capacity` tokens.
//! - Tokens refill at `refill_rate` tokens/second.
//! - Each request consumes 1 token.
//! - If no tokens remain → 429 Too Many Requests.
//!
//! Refill is lazy (computed on each `try_consume` call from elapsed time).
//!
//! ## Integration with the middleware layer
//!
//! `RateLimiter::as_middleware()` returns a `MiddlewareEntry` that can be
//! registered directly into the daemon's `MiddlewareRegistry`.  The entry
//! uses a `before` hook that calls `try_consume` and rejects with 429 when
//! the bucket is empty.
//!
//! ## Response headers (RFC 6585 / draft-ietf-httpapi-ratelimit-headers)
//!
//! Injected via the middleware `after` hook:
//! - `X-RateLimit-Limit`     — bucket capacity
//! - `X-RateLimit-Remaining` — tokens left after this request
//! - `X-RateLimit-Reset`     — Unix timestamp when bucket will be full again
//! - `Retry-After`           — seconds to wait (only on 429 responses)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ── Token Bucket ──────────────────────────────────────────────────────────────

/// A single token-bucket for one endpoint.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Maximum token capacity (= max burst size).
    pub capacity: u64,
    /// Tokens added per second.
    pub refill_rate: f64,
    /// Current token count (fractional for smooth refill).
    tokens: f64,
    /// Last time tokens were refilled.
    last_refill: Instant,
}

impl TokenBucket {
    /// Create a new bucket, starting at full capacity.
    pub fn new(capacity: u64, refill_rate: f64) -> Self {
        Self {
            capacity,
            refill_rate,
            tokens: capacity as f64,
            last_refill: Instant::now(),
        }
    }

    /// Attempt to consume one token. Returns `Ok(remaining)` on success,
    /// `Err(retry_after_secs)` when the bucket is empty.
    pub fn try_consume(&mut self) -> Result<u64, u64> {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(self.tokens as u64)
        } else {
            // How long until 1 token refills
            let wait = ((1.0 - self.tokens) / self.refill_rate).ceil() as u64;
            Err(wait)
        }
    }

    /// Peek at remaining tokens without consuming.
    #[allow(dead_code)]
    pub fn remaining(&mut self) -> u64 {
        self.refill();
        self.tokens as u64
    }

    /// Unix timestamp (seconds) when the bucket will be full again.
    pub fn reset_at(&mut self) -> u64 {
        self.refill();
        let deficit = (self.capacity as f64 - self.tokens).max(0.0);
        let secs_until_full = (deficit / self.refill_rate).ceil() as u64;
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            + secs_until_full
    }

    /// Lazy token refill based on elapsed time.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity as f64);
        self.last_refill = now;
    }
}

// ── Bucket key ────────────────────────────────────────────────────────────────

/// Identifies a rate-limit rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BucketKey {
    /// HTTP method, e.g. "GET".  Use `"*"` for any method.
    pub method: String,
    /// Exact path, e.g. "/api/v1/users".  Use `"*"` for any path.
    pub path: String,
}

impl BucketKey {
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
        }
    }

    /// Returns true if this key matches the given method + path.
    /// `"*"` in either field is a wildcard.
    #[allow(dead_code)]
    pub fn matches(&self, method: &str, path: &str) -> bool {
        let m = self.method == "*" || self.method.eq_ignore_ascii_case(method);
        let p = self.path == "*" || self.path == path;
        m && p
    }
}

// ── Rate limiter ──────────────────────────────────────────────────────────────

/// Stores all per-endpoint token buckets.
#[derive(Debug, Default)]
pub struct RateLimiter {
    buckets: HashMap<BucketKey, TokenBucket>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a rate-limit rule.  Replaces any existing rule for the same key.
    pub fn add_rule(&mut self, key: BucketKey, capacity: u64, refill_rate: f64) {
        self.buckets
            .insert(key, TokenBucket::new(capacity, refill_rate));
    }

    /// Remove a rule.
    pub fn remove_rule(&mut self, key: &BucketKey) -> bool {
        self.buckets.remove(key).is_some()
    }

    /// List all rules as JSON.
    pub fn to_json(&self) -> String {
        let items: Vec<String> = self.buckets.iter().map(|(k, b)| {
            format!(
                r#"{{"method":"{m}","path":"{p}","capacity":{cap},"refill_rate":{rate},"remaining":{rem}}}"#,
                m   = k.method,
                p   = k.path,
                cap = b.capacity,
                rate = b.refill_rate,
                rem  = b.tokens as u64,
            )
        }).collect();
        format!("[{}]", items.join(","))
    }

    /// Try to consume a token for `method`+`path`.
    ///
    /// Finds the **most specific** matching bucket (exact match > wildcard).
    /// Returns:
    /// - `None` — no rule configured for this endpoint (pass through)
    /// - `Some(Ok((capacity, remaining, reset_at)))` — request allowed
    /// - `Some(Err(retry_after_secs))` — rate limit exceeded
    pub fn check(&mut self, method: &str, path: &str) -> Option<Result<(u64, u64, u64), u64>> {
        // Prefer exact method+path match over wildcards
        let key = self.find_best_key(method, path)?;
        let bucket = self.buckets.get_mut(&key)?;
        let cap = bucket.capacity;
        let reset = bucket.reset_at();
        match bucket.try_consume() {
            Ok(remaining) => Some(Ok((cap, remaining, reset))),
            Err(retry) => Some(Err(retry)),
        }
    }

    /// Find the best matching key: exact > method wildcard > path wildcard > global wildcard.
    fn find_best_key(&self, method: &str, path: &str) -> Option<BucketKey> {
        // 1. Exact match
        let exact = BucketKey::new(method.to_uppercase(), path);
        if self.buckets.contains_key(&exact) {
            return Some(exact);
        }

        // 2. Any method, exact path
        let any_method = BucketKey::new("*", path);
        if self.buckets.contains_key(&any_method) {
            return Some(any_method);
        }

        // 3. Exact method, any path
        let any_path = BucketKey::new(method.to_uppercase(), "*");
        if self.buckets.contains_key(&any_path) {
            return Some(any_path);
        }

        // 4. Global wildcard
        let global = BucketKey::new("*", "*");
        if self.buckets.contains_key(&global) {
            return Some(global);
        }

        None
    }

    /// Build a `MiddlewareEntry` wrapping this rate limiter.
    ///
    /// The entry is globally scoped; the bucket lookup narrows it to the
    /// specific endpoint.  Pass in `Arc<Mutex<RateLimiter>>` so the hook
    /// closure shares state with the registry.
    #[allow(dead_code)]
    pub fn as_middleware(
        limiter: Arc<Mutex<RateLimiter>>,
        name: impl Into<String>,
    ) -> crate::middleware::MiddlewareEntry {
        use crate::middleware::{MiddlewareBuilder, Scope};

        let name = name.into();

        let lim_before = Arc::clone(&limiter);
        let lim_after = Arc::clone(&limiter);

        MiddlewareBuilder::new(&name)
            .scope(Scope::Global)
            .before(move |ctx| {
                let mut lim = lim_before.lock().unwrap();
                match lim.check(&ctx.method, &ctx.path) {
                    None => {} // no rule — pass through
                    Some(Ok((cap, remaining, reset))) => {
                        // Allowed — stash headers for the after hook
                        ctx.tag(format!("rl:ok:{cap}:{remaining}:{reset}"));
                    }
                    Some(Err(retry)) => {
                        ctx.reject(
                            429,
                            format!(r#"{{"error":"rate limit exceeded","retry_after":{retry}}}"#),
                        );
                        ctx.tag(format!("rl:exceeded:{retry}"));
                    }
                }
            })
            .after(move |ctx| {
                // Inject rate-limit response headers based on tags set in before hook.
                // Clone tags to avoid holding an immutable borrow while calling set_header.
                let tags: Vec<String> = ctx.tags.clone();
                for tag in &tags {
                    if let Some(rest) = tag.strip_prefix("rl:ok:") {
                        let parts: Vec<&str> = rest.splitn(3, ':').collect();
                        if parts.len() == 3 {
                            ctx.set_header("X-RateLimit-Limit", parts[0]);
                            ctx.set_header("X-RateLimit-Remaining", parts[1]);
                            ctx.set_header("X-RateLimit-Reset", parts[2]);
                        }
                        let _ = &lim_after; // keep Arc alive
                        break;
                    }
                    if let Some(rest) = tag.strip_prefix("rl:exceeded:") {
                        ctx.set_header("Retry-After", rest);
                        ctx.set_header("X-RateLimit-Remaining", "0");
                        break;
                    }
                }
            })
            .build()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ── TokenBucket ───────────────────────────────────────────────────────────

    #[test]
    fn new_bucket_starts_full() {
        let mut b = TokenBucket::new(10, 1.0);
        assert_eq!(b.remaining(), 10);
    }

    #[test]
    fn consume_decrements_tokens() {
        let mut b = TokenBucket::new(5, 1.0);
        assert!(b.try_consume().is_ok());
        assert_eq!(b.remaining(), 4);
    }

    #[test]
    fn consume_returns_remaining() {
        let mut b = TokenBucket::new(3, 1.0);
        assert_eq!(b.try_consume().unwrap(), 2);
        assert_eq!(b.try_consume().unwrap(), 1);
        assert_eq!(b.try_consume().unwrap(), 0);
    }

    #[test]
    fn empty_bucket_returns_err() {
        let mut b = TokenBucket::new(1, 1.0);
        b.try_consume().unwrap(); // drain the one token
        let r = b.try_consume();
        assert!(r.is_err(), "expected Err when empty");
        let retry = r.unwrap_err();
        assert!(retry >= 1, "retry_after should be >= 1 second");
    }

    #[test]
    fn bucket_refills_over_time() {
        // Use a very fast refill rate so we don't need to actually sleep
        let mut b = TokenBucket::new(10, 1000.0); // 1000 tokens/sec
                                                  // drain all
        for _ in 0..10 {
            b.try_consume().unwrap();
        }
        assert_eq!(b.remaining(), 0);
        // Sleep just 2ms — should get ~2 tokens back at 1000 tok/s
        std::thread::sleep(Duration::from_millis(2));
        assert!(b.remaining() >= 1, "expected at least 1 token after refill");
    }

    #[test]
    fn bucket_never_exceeds_capacity() {
        let mut b = TokenBucket::new(5, 1000.0);
        std::thread::sleep(Duration::from_millis(20));
        b.refill(); // would add 20 tokens without cap
        assert_eq!(b.remaining(), 5);
    }

    #[test]
    fn reset_at_is_future_timestamp() {
        let mut b = TokenBucket::new(10, 1.0);
        // drain partially
        b.try_consume().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let reset = b.reset_at();
        assert!(reset >= now, "reset should be >= now");
    }

    // ── BucketKey matching ────────────────────────────────────────────────────

    #[test]
    fn exact_key_matches() {
        let k = BucketKey::new("GET", "/ping");
        assert!(k.matches("GET", "/ping"));
        assert!(k.matches("get", "/ping")); // case-insensitive method
        assert!(!k.matches("POST", "/ping"));
        assert!(!k.matches("GET", "/pong"));
    }

    #[test]
    fn wildcard_method_matches_any() {
        let k = BucketKey::new("*", "/ping");
        assert!(k.matches("GET", "/ping"));
        assert!(k.matches("DELETE", "/ping"));
        assert!(!k.matches("GET", "/other"));
    }

    #[test]
    fn wildcard_path_matches_any() {
        let k = BucketKey::new("POST", "*");
        assert!(k.matches("POST", "/anything"));
        assert!(k.matches("POST", "/a/b/c"));
        assert!(!k.matches("GET", "/anything"));
    }

    #[test]
    fn global_wildcard_matches_everything() {
        let k = BucketKey::new("*", "*");
        assert!(k.matches("GET", "/foo"));
        assert!(k.matches("DELETE", "/bar/baz"));
    }

    // ── RateLimiter ───────────────────────────────────────────────────────────

    #[test]
    fn no_rule_returns_none() {
        let mut lim = RateLimiter::new();
        assert!(lim.check("GET", "/unrestricted").is_none());
    }

    #[test]
    fn rule_allows_up_to_capacity() {
        let mut lim = RateLimiter::new();
        lim.add_rule(BucketKey::new("GET", "/limited"), 3, 1.0);
        for _ in 0..3 {
            assert!(lim.check("GET", "/limited").unwrap().is_ok());
        }
        assert!(lim.check("GET", "/limited").unwrap().is_err());
    }

    #[test]
    fn check_returns_capacity_remaining_reset() {
        let mut lim = RateLimiter::new();
        lim.add_rule(BucketKey::new("POST", "/items"), 5, 1.0);
        let (cap, rem, reset) = lim.check("POST", "/items").unwrap().unwrap();
        assert_eq!(cap, 5);
        assert_eq!(rem, 4);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(reset >= now);
    }

    #[test]
    fn exact_match_preferred_over_wildcard() {
        let mut lim = RateLimiter::new();
        // Global rule: 100 tokens
        lim.add_rule(BucketKey::new("*", "*"), 100, 1.0);
        // Specific rule: 2 tokens
        lim.add_rule(BucketKey::new("GET", "/strict"), 2, 1.0);

        // /strict should use the 2-token rule
        assert!(lim.check("GET", "/strict").unwrap().is_ok());
        assert!(lim.check("GET", "/strict").unwrap().is_ok());
        assert!(lim.check("GET", "/strict").unwrap().is_err());

        // other paths should use the global 100-token rule
        assert!(lim.check("GET", "/other").unwrap().is_ok());
    }

    #[test]
    fn remove_rule() {
        let mut lim = RateLimiter::new();
        let key = BucketKey::new("GET", "/x");
        lim.add_rule(key.clone(), 5, 1.0);
        assert!(lim.remove_rule(&key));
        assert!(lim.check("GET", "/x").is_none());
        assert!(!lim.remove_rule(&key)); // already gone
    }

    #[test]
    fn to_json_contains_rules() {
        let mut lim = RateLimiter::new();
        lim.add_rule(BucketKey::new("GET", "/api"), 10, 2.0);
        let j = lim.to_json();
        assert!(j.contains("GET"), "got: {j}");
        assert!(j.contains("/api"), "got: {j}");
        assert!(j.contains("\"capacity\":10"), "got: {j}");
    }

    // ── Middleware integration ─────────────────────────────────────────────────

    #[test]
    fn middleware_allows_within_limit() {
        use crate::middleware::{MiddlewareContext, MiddlewareRegistry};

        let lim = Arc::new(Mutex::new(RateLimiter::new()));
        lim.lock()
            .unwrap()
            .add_rule(BucketKey::new("GET", "/ok"), 5, 1.0);

        let entry = RateLimiter::as_middleware(Arc::clone(&lim), "rl");
        let mut reg = MiddlewareRegistry::new();
        reg.register(entry);

        let mut ctx = MiddlewareContext::new("GET", "/ok", "req-1");
        reg.run_before(&mut ctx);
        assert!(!ctx.is_rejected(), "should pass within limit");
        reg.run_after(&mut ctx);
        assert!(
            ctx.extra_headers.contains_key("X-RateLimit-Limit"),
            "missing limit header"
        );
        assert!(
            ctx.extra_headers.contains_key("X-RateLimit-Remaining"),
            "missing remaining header"
        );
        assert!(
            ctx.extra_headers.contains_key("X-RateLimit-Reset"),
            "missing reset header"
        );
    }

    #[test]
    fn middleware_rejects_when_over_limit() {
        use crate::middleware::{MiddlewareContext, MiddlewareRegistry};

        let lim = Arc::new(Mutex::new(RateLimiter::new()));
        lim.lock()
            .unwrap()
            .add_rule(BucketKey::new("POST", "/submit"), 1, 0.1);

        let entry = RateLimiter::as_middleware(Arc::clone(&lim), "rl");
        let mut reg = MiddlewareRegistry::new();
        reg.register(entry);

        // First request passes
        let mut ctx1 = MiddlewareContext::new("POST", "/submit", "req-1");
        reg.run_before(&mut ctx1);
        assert!(!ctx1.is_rejected());

        // Second request is rejected
        let mut ctx2 = MiddlewareContext::new("POST", "/submit", "req-2");
        reg.run_before(&mut ctx2);
        assert!(ctx2.is_rejected(), "should be rate-limited");
        let (status, body) = ctx2.rejection.unwrap();
        assert_eq!(status, 429);
        assert!(body.contains("rate limit exceeded"), "got: {body}");
    }

    #[test]
    fn middleware_passes_unmatched_endpoints() {
        use crate::middleware::{MiddlewareContext, MiddlewareRegistry};

        let lim = Arc::new(Mutex::new(RateLimiter::new()));
        // No rule for /health
        let entry = RateLimiter::as_middleware(Arc::clone(&lim), "rl");
        let mut reg = MiddlewareRegistry::new();
        reg.register(entry);

        let mut ctx = MiddlewareContext::new("GET", "/health", "req-1");
        reg.run_before(&mut ctx);
        assert!(!ctx.is_rejected(), "unmatched endpoint should pass through");
    }

    #[test]
    fn retry_after_header_on_429() {
        use crate::middleware::{MiddlewareContext, MiddlewareRegistry};

        let lim = Arc::new(Mutex::new(RateLimiter::new()));
        lim.lock()
            .unwrap()
            .add_rule(BucketKey::new("*", "*"), 1, 0.5);

        let entry = RateLimiter::as_middleware(Arc::clone(&lim), "rl");
        let mut reg = MiddlewareRegistry::new();
        reg.register(entry);

        let mut ctx1 = MiddlewareContext::new("GET", "/any", "r1");
        reg.run_before(&mut ctx1);
        reg.run_after(&mut ctx1); // drain the one token

        let mut ctx2 = MiddlewareContext::new("DELETE", "/any", "r2");
        reg.run_before(&mut ctx2);
        reg.run_after(&mut ctx2);
        assert!(ctx2.is_rejected());
        assert!(
            ctx2.extra_headers.contains_key("Retry-After"),
            "missing Retry-After header"
        );
        assert!(ctx2.extra_headers.contains_key("X-RateLimit-Remaining"));
        assert_eq!(
            ctx2.extra_headers
                .get("X-RateLimit-Remaining")
                .map(|s| s.as_str()),
            Some("0")
        );
    }
}
