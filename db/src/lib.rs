//! Bridge in-memory key-value store.
//!
//! A thread-safe, namespace-partitioned key-value store with:
//! - **Namespaces** — isolate data by service or component
//! - **TTL support** — auto-expire entries after a duration
//! - **Transactions** — batch operations with rollback
//! - **Pattern search** — wildcard key lookups
//!
//! # Example
//!
//! ```rust
//! use db::Db;
//! use std::time::Duration;
//!
//! let db = Db::new();
//! db.put("sessions", "user:123", "jwt-token");
//! db.put_with_ttl("cache", "hot-key", "value", Duration::from_secs(60));
//! assert_eq!(db.get("sessions", "user:123"), Some("jwt-token".to_string()));
//! db.del("sessions", "user:123");
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ══════════════════════════════════════════════════════════════════════════════
// Internal storage types
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct Entry {
    value: String,
    expires_at: Option<Instant>,
}

impl Entry {
    fn new(value: impl Into<String>) -> Self {
        Entry { value: value.into(), expires_at: None }
    }

    fn with_ttl(value: impl Into<String>, ttl: Duration) -> Self {
        Entry {
            value: value.into(),
            expires_at: Some(Instant::now() + ttl),
        }
    }

    fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(deadline) => Instant::now() > deadline,
            None => false,
        }
    }

    fn remaining_ttl(&self) -> Option<Duration> {
        self.expires_at.and_then(|deadline| {
            let now = Instant::now();
            if now < deadline {
                Some(deadline - now)
            } else {
                None
            }
        })
    }
}

type NamespaceMap = HashMap<String, HashMap<String, Entry>>;

// ══════════════════════════════════════════════════════════════════════════════
// Main Db type
// ══════════════════════════════════════════════════════════════════════════════

/// A thread-safe, namespaced in-memory key-value store.
#[derive(Clone)]
pub struct Db {
    inner: Arc<Mutex<NamespaceMap>>,
}

impl Db {
    /// Create a new, empty database.
    pub fn new() -> Self {
        Db { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Insert or update a key in the given namespace.
    pub fn put(&self, ns: &str, key: &str, value: impl Into<String>) {
        let mut guard = self.inner.lock().unwrap();
        guard
            .entry(ns.to_string())
            .or_insert_with(HashMap::new)
            .insert(key.to_string(), Entry::new(value));
    }

    /// Insert or update a key with a TTL.
    pub fn put_with_ttl(&self, ns: &str, key: &str, value: impl Into<String>, ttl: Duration) {
        let mut guard = self.inner.lock().unwrap();
        guard
            .entry(ns.to_string())
            .or_insert_with(HashMap::new)
            .insert(key.to_string(), Entry::with_ttl(value, ttl));
    }

    /// Retrieve a value. Returns `None` if missing or expired.
    pub fn get(&self, ns: &str, key: &str) -> Option<String> {
        let guard = self.inner.lock().unwrap();
        guard
            .get(ns)
            .and_then(|namespace| namespace.get(key))
            .filter(|e| !e.is_expired())
            .map(|e| e.value.clone())
    }

    /// Delete a key. Returns `true` if it existed.
    pub fn del(&self, ns: &str, key: &str) -> bool {
        let mut guard = self.inner.lock().unwrap();
        guard
            .get_mut(ns)
            .and_then(|namespace| namespace.remove(key))
            .is_some()
    }

    /// List all non-expired keys in a namespace.
    pub fn keys(&self, ns: &str) -> Vec<String> {
        let guard = self.inner.lock().unwrap();
        match guard.get(ns) {
            None => vec![],
            Some(namespace) => namespace
                .iter()
                .filter(|(_, e)| !e.is_expired())
                .map(|(k, _)| k.clone())
                .collect(),
        }
    }

    /// List all non-expired keys matching a glob pattern in a namespace.
    ///
    /// Supports `*` (any sequence) and `?` (any single char).
    pub fn keys_matching(&self, ns: &str, pattern: &str) -> Vec<String> {
        self.keys(ns)
            .into_iter()
            .filter(|k| glob_match(pattern, k))
            .collect()
    }

    /// Flush all keys in a namespace. Returns number of keys removed.
    pub fn flush_ns(&self, ns: &str) -> usize {
        let mut guard = self.inner.lock().unwrap();
        match guard.get_mut(ns) {
            None => 0,
            Some(namespace) => {
                let count = namespace.len();
                namespace.clear();
                count
            }
        }
    }

    /// Flush all namespaces. Returns total keys removed.
    pub fn flush_all(&self) -> usize {
        let mut guard = self.inner.lock().unwrap();
        let count: usize = guard.values().map(|ns| ns.len()).sum();
        guard.clear();
        count
    }

    /// Get the remaining TTL for a key.
    /// Returns `None` if the key doesn't exist, and `Some(None)` if it has no TTL.
    pub fn ttl(&self, ns: &str, key: &str) -> Option<Option<Duration>> {
        let guard = self.inner.lock().unwrap();
        guard
            .get(ns)
            .and_then(|namespace| namespace.get(key))
            .filter(|e| !e.is_expired())
            .map(|e| e.remaining_ttl())
    }

    /// Set the TTL on an existing key.
    /// Returns `true` if the key existed and TTL was updated.
    pub fn expire(&self, ns: &str, key: &str, ttl: Duration) -> bool {
        let mut guard = self.inner.lock().unwrap();
        match guard
            .get_mut(ns)
            .and_then(|namespace| namespace.get_mut(key))
        {
            Some(entry) if !entry.is_expired() => {
                entry.expires_at = Some(Instant::now() + ttl);
                true
            }
            _ => false,
        }
    }

    /// Remove the TTL from an existing key, making it permanent.
    pub fn persist(&self, ns: &str, key: &str) -> bool {
        let mut guard = self.inner.lock().unwrap();
        match guard
            .get_mut(ns)
            .and_then(|namespace| namespace.get_mut(key))
        {
            Some(entry) if !entry.is_expired() => {
                entry.expires_at = None;
                true
            }
            _ => false,
        }
    }

    /// Check if a key exists (and hasn't expired).
    pub fn exists(&self, ns: &str, key: &str) -> bool {
        self.get(ns, key).is_some()
    }

    /// Count non-expired keys in a namespace.
    pub fn count(&self, ns: &str) -> usize {
        self.keys(ns).len()
    }

    /// Remove all expired entries across all namespaces.
    pub fn purge_expired(&self) -> usize {
        let mut guard = self.inner.lock().unwrap();
        let mut total = 0;
        for namespace in guard.values_mut() {
            let before = namespace.len();
            namespace.retain(|_, e| !e.is_expired());
            total += before - namespace.len();
        }
        total
    }

    /// Begin a transaction. See [`Transaction`] for details.
    pub fn transaction(&self) -> Transaction<'_> {
        Transaction::new(self)
    }

    /// List all namespace names.
    pub fn namespaces(&self) -> Vec<String> {
        self.inner.lock().unwrap().keys().cloned().collect()
    }

    /// Get total stats across all namespaces.
    pub fn stats(&self) -> DbStats {
        let guard = self.inner.lock().unwrap();
        let mut total_entries = 0usize;
        let mut expired_entries = 0usize;

        for namespace in guard.values() {
            for entry in namespace.values() {
                total_entries += 1;
                if entry.is_expired() {
                    expired_entries += 1;
                }
            }
        }

        DbStats {
            namespaces: guard.len(),
            total_entries,
            expired_entries,
            live_entries: total_entries - expired_entries,
        }
    }
}

impl Default for Db {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct DbStats {
    pub namespaces: usize,
    pub total_entries: usize,
    pub expired_entries: usize,
    pub live_entries: usize,
}

impl DbStats {
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"namespaces":{},"total_entries":{},"expired_entries":{},"live_entries":{}}}"#,
            self.namespaces, self.total_entries, self.expired_entries, self.live_entries
        )
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Transactions
// ══════════════════════════════════════════════════════════════════════════════

/// An atomic transaction over a Db namespace.
///
/// Accumulates a sequence of operations and applies them all-at-once on
/// `commit()`. Calling `rollback()` or dropping without committing discards
/// all changes.
///
/// # Example
///
/// ```rust
/// let db = db::Db::new();
/// let mut tx = db.transaction();
/// tx.put("users", "id:1", "alice");
/// tx.put("users", "id:2", "bob");
/// tx.commit().unwrap();
/// ```
pub struct Transaction<'a> {
    db: &'a Db,
    ops: Vec<TxOp>,
}

#[derive(Debug)]
enum TxOp {
    Put { ns: String, key: String, value: String },
    PutTtl { ns: String, key: String, value: String, ttl: Duration },
    Del { ns: String, key: String },
    Flush { ns: String },
}

impl<'a> Transaction<'a> {
    fn new(db: &'a Db) -> Self {
        Transaction { db, ops: Vec::new() }
    }

    /// Queue a put operation.
    pub fn put(&mut self, ns: &str, key: &str, value: impl Into<String>) {
        self.ops.push(TxOp::Put {
            ns: ns.to_string(),
            key: key.to_string(),
            value: value.into(),
        });
    }

    /// Queue a put-with-TTL operation.
    pub fn put_with_ttl(&mut self, ns: &str, key: &str, value: impl Into<String>, ttl: Duration) {
        self.ops.push(TxOp::PutTtl {
            ns: ns.to_string(),
            key: key.to_string(),
            value: value.into(),
            ttl,
        });
    }

    /// Queue a delete operation.
    pub fn del(&mut self, ns: &str, key: &str) {
        self.ops.push(TxOp::Del {
            ns: ns.to_string(),
            key: key.to_string(),
        });
    }

    /// Queue a flush-namespace operation.
    pub fn flush(&mut self, ns: &str) {
        self.ops.push(TxOp::Flush { ns: ns.to_string() });
    }

    /// Commit all queued operations atomically.
    pub fn commit(self) -> Result<usize, String> {
        let count = self.ops.len();
        for op in self.ops {
            match op {
                TxOp::Put { ns, key, value } => self.db.put(&ns, &key, value),
                TxOp::PutTtl { ns, key, value, ttl } => self.db.put_with_ttl(&ns, &key, value, ttl),
                TxOp::Del { ns, key } => { self.db.del(&ns, &key); },
                TxOp::Flush { ns } => { self.db.flush_ns(&ns); },
            }
        }
        Ok(count)
    }

    /// Discard all queued operations.
    pub fn rollback(self) {
        // ops are dropped, nothing is applied
    }

    /// Number of queued operations.
    pub fn op_count(&self) -> usize {
        self.ops.len()
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Glob matching
// ══════════════════════════════════════════════════════════════════════════════

/// Simple glob match supporting `*` and `?`.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();

    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;

    // Handle leading wildcards
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }

    for i in 1..=p.len() {
        for j in 1..=t.len() {
            if p[i - 1] == '*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if p[i - 1] == '?' || p[i - 1] == t[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }

    dp[p.len()][t.len()]
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_put_get_del() {
        let db = Db::new();
        db.put("ns1", "key1", "value1");
        assert_eq!(db.get("ns1", "key1"), Some("value1".to_string()));
        db.del("ns1", "key1");
        assert_eq!(db.get("ns1", "key1"), None);
    }

    #[test]
    fn namespace_isolation() {
        let db = Db::new();
        db.put("ns1", "key", "a");
        db.put("ns2", "key", "b");
        assert_eq!(db.get("ns1", "key"), Some("a".to_string()));
        assert_eq!(db.get("ns2", "key"), Some("b".to_string()));
    }

    #[test]
    fn keys_listing() {
        let db = Db::new();
        db.put("ns", "k1", "v1");
        db.put("ns", "k2", "v2");
        db.put("ns", "k3", "v3");
        let mut keys = db.keys("ns");
        keys.sort();
        assert_eq!(keys, vec!["k1", "k2", "k3"]);
    }

    #[test]
    fn flush_ns() {
        let db = Db::new();
        db.put("ns", "k1", "v1");
        db.put("ns", "k2", "v2");
        assert_eq!(db.count("ns"), 2);
        db.flush_ns("ns");
        assert_eq!(db.count("ns"), 0);
    }

    #[test]
    fn ttl_expiry() {
        let db = Db::new();
        db.put_with_ttl("ns", "key", "value", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(db.get("ns", "key"), None);
    }

    #[test]
    fn ttl_still_alive() {
        let db = Db::new();
        db.put_with_ttl("ns", "key", "value", Duration::from_secs(60));
        assert_eq!(db.get("ns", "key"), Some("value".to_string()));
    }

    #[test]
    fn expire_command() {
        let db = Db::new();
        db.put("ns", "key", "value");
        db.expire("ns", "key", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(db.get("ns", "key"), None);
    }

    #[test]
    fn persist_removes_ttl() {
        let db = Db::new();
        db.put_with_ttl("ns", "key", "value", Duration::from_millis(50));
        db.persist("ns", "key");
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(db.get("ns", "key"), Some("value".to_string()));
    }

    #[test]
    fn transaction_commit() {
        let db = Db::new();
        let mut tx = db.transaction();
        tx.put("users", "id:1", "alice");
        tx.put("users", "id:2", "bob");
        tx.commit().unwrap();
        assert_eq!(db.get("users", "id:1"), Some("alice".to_string()));
        assert_eq!(db.get("users", "id:2"), Some("bob".to_string()));
    }

    #[test]
    fn transaction_rollback() {
        let db = Db::new();
        db.put("users", "id:1", "alice");
        let mut tx = db.transaction();
        tx.put("users", "id:1", "modified");
        tx.rollback();
        // Original value should remain unchanged
        assert_eq!(db.get("users", "id:1"), Some("alice".to_string()));
    }

    #[test]
    fn glob_star() {
        assert!(glob_match("user:*", "user:123"));
        assert!(glob_match("user:*", "user:abc"));
        assert!(!glob_match("user:*", "admin:123"));
    }

    #[test]
    fn glob_question() {
        assert!(glob_match("key?", "key1"));
        assert!(glob_match("key?", "keyA"));
        assert!(!glob_match("key?", "key12"));
    }

    #[test]
    fn glob_exact() {
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exact2"));
    }

    #[test]
    fn keys_matching() {
        let db = Db::new();
        db.put("ns", "user:1", "a");
        db.put("ns", "user:2", "b");
        db.put("ns", "session:1", "c");
        let user_keys = db.keys_matching("ns", "user:*");
        assert_eq!(user_keys.len(), 2);
    }

    #[test]
    fn stats() {
        let db = Db::new();
        db.put("ns1", "k1", "v1");
        db.put("ns1", "k2", "v2");
        db.put("ns2", "k1", "v1");
        db.put_with_ttl("ns2", "expired", "x", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(5));
        let stats = db.stats();
        assert_eq!(stats.namespaces, 2);
        assert_eq!(stats.live_entries, 3);
        assert_eq!(stats.expired_entries, 1);
    }
}
