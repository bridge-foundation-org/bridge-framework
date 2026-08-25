//! Bridge Cache — Encore-style keyspaces with TTL, LRU eviction, and
//! pattern-based invalidation.
//!
//! Inspired by Encore commits 1707 (cache clusters), 1975/2202 (Redis
//! MGET/MSET), 2069 (full caching API), 2073-2074 (in-memory cache config,
//! legacy config conversion).
//!
//! A *keyspace* is a named cache with its own config (max entries, default
//! TTL) — the unit applications declare in code. Entries live in memory;
//! `hit` / `miss` counters feed the metrics surface.
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Keyspace config ──────────────────────────────────────────────────────────

/// Per-keyspace configuration (Encore `RedisClusterConfig` semantics).
#[derive(Debug, Clone)]
pub struct KeyspaceConfig {
    /// Evict when the keyspace holds more than this many live entries.
    pub max_entries: usize,
    /// Default TTL applied on set when the caller does not pass one.
    /// `0` = no expiry.
    pub default_ttl_ms: u64,
}

impl Default for KeyspaceConfig {
    fn default() -> Self {
        KeyspaceConfig {
            max_entries: 10_000,
            default_ttl_ms: 300_000, // 5 minutes
        }
    }
}

// ── Entry ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Entry {
    value: String,
    expires_at: Option<u64>,
    /// Last-access clock (ms); LRU victim selection reads this.
    last_access: u64,
}

impl Entry {
    fn is_expired(&self, now: u64) -> bool {
        matches!(self.expires_at, Some(t) if t <= now)
    }

    fn to_json(&self, key: &str, now: u64) -> String {
        let ttl_left = self.expires_at.map(|t| t.saturating_sub(now)).unwrap_or(0);
        format!(
            r#"{{"key":"{}","value":{},"ttl_ms_left":{}}}"#,
            key, self.value, ttl_left
        )
    }
}

// ── Stats ────────────────────────────────────────────────────────────────────

/// Hit/miss counters for one keyspace.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyspaceStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

// ── Registry ─────────────────────────────────────────────────────────────────

/// In-process cache registry — suitable for local development.
/// Swap for a Redis-backed implementation in production via the same surface.
#[derive(Default)]
pub struct CacheRegistry {
    /// Logical operation clock for LRU ordering — immune to same-millisecond
    /// wall-clock ties (victim selection must be deterministic).
    op_seq: u64,
    keyspace_order: Vec<String>,
    keyspaces: HashMap<String, Keyspace>,
}

struct Keyspace {
    cfg: KeyspaceConfig,
    map: HashMap<String, Entry>,
    stats: KeyspaceStats,
}

impl CacheRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_seq(&mut self) -> u64 {
        self.op_seq += 1;
        self.op_seq
    }

    /// Declare a keyspace (idempotent — re-declaring keeps existing data but
    /// applies any new config limits).
    pub fn ensure_keyspace(&mut self, name: &str, cfg: KeyspaceConfig) {
        if let Some(ks) = self.keyspaces.get_mut(name) {
            ks.cfg = cfg;
        } else {
            self.keyspace_order.push(name.to_string());
            self.keyspaces.insert(
                name.to_string(),
                Keyspace {
                    cfg,
                    map: HashMap::new(),
                    stats: KeyspaceStats::default(),
                },
            );
        }
    }

    pub fn has_keyspace(&self, name: &str) -> bool {
        self.keyspaces.contains_key(name)
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    // ── Core operations ──────────────────────────────────────────────────────

    /// Set `key` → JSON `value`, honoring per-call or default TTL.
    /// Returns the number of entries evicted to enforce `max_entries`.
    pub fn set(&mut self, ks: &str, key: &str, value: &str, ttl_ms: Option<u64>) -> usize {
        let now = Self::now_ms();
        let seq = self.next_seq();
        let space = self
            .keyspaces
            .entry(ks.to_string())
            .or_insert_with(|| Keyspace {
                cfg: KeyspaceConfig::default(),
                map: HashMap::new(),
                stats: KeyspaceStats::default(),
            });

        let effective_ttl = ttl_ms.unwrap_or(space.cfg.default_ttl_ms);
        let expires_at = if effective_ttl == 0 {
            None
        } else {
            Some(now.saturating_add(effective_ttl))
        };
        space.map.insert(
            key.to_string(),
            Entry {
                value: value.to_string(),
                expires_at,
                last_access: seq,
            },
        );

        // Enforce capacity: drop expired first, then least-recently-used.
        let mut evicted = 0usize;
        while space.map.len() > space.cfg.max_entries {
            let victims: Vec<String> = space
                .map
                .iter()
                .filter(|(_, e)| e.is_expired(now))
                .map(|(k, _)| k.clone())
                .collect();
            let victim = victims.first().cloned().or_else(|| {
                space
                    .map
                    .iter()
                    .min_by_key(|(_, e)| e.last_access)
                    .map(|(k, _)| k.clone())
            });
            match victim {
                Some(v) => {
                    space.map.remove(&v);
                    space.stats.evictions += 1;
                    evicted += 1;
                }
                None => break,
            }
        }
        evicted
    }

    /// Get a live (non-expired) value; bumps LRU recency and hit stats.
    /// Absent and expired keys both count as misses (expired entries are
    /// removed lazily on access).
    pub fn get(&mut self, ks: &str, key: &str) -> Option<String> {
        let now = Self::now_ms();
        let seq = self.next_seq();
        let space = self.keyspaces.get_mut(ks)?;
        let Some(entry) = space.map.get_mut(key) else {
            space.stats.misses += 1;
            return None;
        };
        if entry.is_expired(now) {
            space.map.remove(key);
            space.stats.misses += 1;
            return None;
        }
        entry.last_access = seq;
        space.stats.hits += 1;
        Some(entry.value.clone())
    }

    /// Like [`get`], but returns the full entry document including the
    /// remaining TTL: `{"key":...,"value":...,"ttl_ms_left":N}` — or
    /// `"ttl_ms_left":null` for entries with no expiry.
    pub fn get_json(&mut self, ks: &str, key: &str) -> Option<String> {
        let value = self.get(ks, key)?;
        let now = Self::now_ms();
        let ttl_left = self
            .keyspaces
            .get(ks)
            .and_then(|s| s.map.get(key))
            .and_then(|e| e.expires_at)
            .map(|t| t.saturating_sub(now).to_string())
            .unwrap_or_else(|| "null".to_string());
        Some(format!(
            r#"{{"key":"{key}","value":{value},"ttl_ms_left":{ttl_left}}}"#
        ))
    }

    /// Delete a single key. Returns true when a live entry was removed.
    pub fn del(&mut self, ks: &str, key: &str) -> bool {
        let now = Self::now_ms();
        self.keyspaces
            .get_mut(ks)
            .map(|s| matches!(s.map.remove(key), Some(e) if !e.is_expired(now)))
            .unwrap_or(false)
    }

    /// Invalidate every key matching a glob (`*` wildcard, prefix/suffix
    /// patterns cover the common cases). Returns how many live entries died.
    pub fn invalidate_pattern(&mut self, ks: &str, pattern: &str) -> usize {
        let now = Self::now_ms();
        let Some(space) = self.keyspaces.get_mut(ks) else {
            return 0;
        };
        let dead: Vec<String> = space
            .map
            .keys()
            .filter(|k| glob_match(pattern, k))
            .cloned()
            .collect();
        let mut n = 0;
        for k in dead {
            if !space.map[&k].is_expired(now) {
                n += 1;
            }
            space.map.remove(&k);
        }
        n
    }

    /// Drop all live entries in a keyspace; returns how many died.
    pub fn invalidate_all(&mut self, ks: &str) -> usize {
        let now = Self::now_ms();
        self.keyspaces
            .get_mut(ks)
            .map(|s| {
                let n = s.map.values().filter(|e| !e.is_expired(now)).count();
                s.map.clear();
                n
            })
            .unwrap_or(0)
    }

    /// Multi-get across one keyspace (commit 1975). Missing/expired keys come
    /// back as `null`.
    pub fn mget(&mut self, ks: &str, keys: &[String]) -> Vec<Option<String>> {
        keys.iter().map(|k| self.get(ks, k)).collect()
    }

    /// Multi-set within one keyspace (commit 2202). One shared optional TTL.
    pub fn mset(&mut self, ks: &str, pairs: &[(String, String)], ttl_ms: Option<u64>) {
        for (k, v) in pairs {
            self.set(ks, k, v, ttl_ms);
        }
    }

    // ── Introspection ────────────────────────────────────────────────────────

    fn live_count(space: &Keyspace, now: u64) -> usize {
        space.map.values().filter(|e| !e.is_expired(now)).count()
    }

    /// One-line JSON summary per keyspace.
    pub fn list_json(&self) -> String {
        let now = Self::now_ms();
        let mut names: Vec<&String> = self.keyspaces.keys().collect();
        names.sort();
        let items: Vec<String> = names
            .iter()
            .map(|name| {
                let s = &self.keyspaces[*name];
                format!(
                    r#"{{"name":"{n}","entries":{e},"max_entries":{m},"default_ttl_ms":{t},"hits":{h},"misses":{mi},"evictions":{ev}}}"#,
                    n = name,
                    e = Self::live_count(s, now),
                    m = s.cfg.max_entries,
                    t = s.cfg.default_ttl_ms,
                    h = s.stats.hits,
                    mi = s.stats.misses,
                    ev = s.stats.evictions,
                )
            })
            .collect();
        format!(r#"{{"keyspaces":[{items}]}}"#, items = items.join(","))
    }

    /// Detailed JSON for one keyspace's config + stats.
    pub fn keyspace_json(&self, name: &str) -> Option<String> {
        let now = Self::now_ms();
        let s = self.keyspaces.get(name)?;
        Some(format!(
            r#"{{"name":"{n}","entries":{e},"max_entries":{m},"default_ttl_ms":{t},"hits":{h},"misses":{mi},"evictions":{ev}}}"#,
            n = name,
            e = Self::live_count(s, now),
            m = s.cfg.max_entries,
            t = s.cfg.default_ttl_ms,
            h = s.stats.hits,
            mi = s.stats.misses,
            ev = s.stats.evictions,
        ))
    }

    /// All live entries of a keyspace as JSON array items.
    pub fn entries_json(&self, name: &str) -> Option<String> {
        let now = Self::now_ms();
        let s = self.keyspaces.get(name)?;
        let mut keys: Vec<&String> = s.map.keys().collect();
        keys.sort();
        let items: Vec<String> = keys
            .iter()
            .filter_map(|k| {
                let e = &s.map[*k];
                if e.is_expired(now) {
                    None
                } else {
                    Some(e.to_json(k, now))
                }
            })
            .collect();
        Some(format!(
            r#"{{"entries":[{items}]}}"#,
            items = items.join(",")
        ))
    }

    /// Live entries grouped per keyspace (debug surface for `/entries`).
    pub fn entries_all_json(&self) -> String {
        let now = Self::now_ms();
        let mut names: Vec<&String> = self.keyspaces.keys().collect();
        names.sort();
        let groups: Vec<String> = names
            .iter()
            .map(|name| {
                let s = &self.keyspaces[*name];
                let mut keys: Vec<&String> = s.map.keys().collect();
                keys.sort();
                let items: Vec<String> = keys
                    .iter()
                    .filter_map(|k| {
                        let e = &s.map[*k];
                        if e.is_expired(now) {
                            None
                        } else {
                            Some(e.to_json(k, now))
                        }
                    })
                    .collect();
                format!(
                    r#"{{"keyspace":"{n}","entries":[{items}]}}"#,
                    n = name,
                    items = items.join(",")
                )
            })
            .collect();
        format!(r#"{{"keyspaces":[{groups}]}}"#, groups = groups.join(","))
    }

    /// Aggregate status line for `/api/v1/cache`.
    pub fn status_json(&self) -> String {
        let now = Self::now_ms();
        let keyspaces = self.keyspaces.len();
        let entries: usize = self
            .keyspaces
            .values()
            .map(|s| Self::live_count(s, now))
            .sum();
        let hits: u64 = self.keyspaces.values().map(|s| s.stats.hits).sum();
        let misses: u64 = self.keyspaces.values().map(|s| s.stats.misses).sum();
        format!(
            r#"{{"keyspaces":{ks},"entries":{en},"hits":{h},"misses":{mi}}}"#,
            ks = keyspaces,
            en = entries,
            h = hits,
            mi = misses,
        )
    }
}

/// Minimal glob with `*` wildcards (any run of characters). `?` matches
/// exactly one character. No escaping — cache keys are app-controlled.
fn glob_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == text;
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_rec(&p, 0, &t, 0)
}

fn glob_rec(p: &[char], mut pi: usize, t: &[char], mut ti: usize) -> bool {
    while pi < p.len() {
        match p[pi] {
            '*' => {
                // Collapse consecutive stars; try every suffix position.
                while pi + 1 < p.len() && p[pi + 1] == '*' {
                    pi += 1;
                }
                if pi + 1 == p.len() {
                    return true;
                }
                for k in ti..=t.len() {
                    if glob_rec(p, pi + 1, t, k) {
                        return true;
                    }
                }
                return false;
            }
            '?' => {
                if ti >= t.len() {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
            c => {
                if ti >= t.len() || t[ti] != c {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }
    ti == t.len()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_roundtrip_and_miss_on_unknown() {
        let mut c = CacheRegistry::new();
        c.ensure_keyspace("users", KeyspaceConfig::default());
        assert_eq!(c.set("users", "u1", r#"{"id":1}"#, None), 0);
        assert_eq!(c.get("users", "u1").as_deref(), Some(r#"{"id":1}"#));
        assert!(c.get("users", "nope").is_none());
    }

    #[test]
    fn ttl_expiry_counts_as_miss_and_cleans_up() {
        let mut c = CacheRegistry::new();
        c.ensure_keyspace(
            "short",
            KeyspaceConfig {
                max_entries: 100,
                default_ttl_ms: 0,
            },
        );
        c.set("short", "k", "v", Some(1)); // 1ms — will expire
        std::thread::sleep(std::time::Duration::from_millis(15));
        assert!(c.get("short", "k").is_none(), "must be expired");
        assert_eq!(
            c.status_json(),
            r#"{"keyspaces":1,"entries":0,"hits":0,"misses":1}"#
        );
    }

    #[test]
    fn lru_eviction_enforces_max_entries() {
        let mut c = CacheRegistry::new();
        c.ensure_keyspace(
            "tiny",
            KeyspaceConfig {
                max_entries: 2,
                default_ttl_ms: 0,
            },
        );
        c.set("tiny", "a", "1", None);
        c.set("tiny", "b", "2", None);
        assert!(c.get("tiny", "a").is_some(), "touch a so b becomes LRU");
        c.set("tiny", "c", "3", None);
        assert!(c.get("tiny", "b").is_none(), "b must be LRU-evicted");
        assert!(c.get("tiny", "a").is_some());
        assert!(c.get("tiny", "c").is_some());
        assert_eq!(
            c.keyspace_json("tiny").unwrap(),
            r#"{"name":"tiny","entries":2,"max_entries":2,"default_ttl_ms":0,"hits":3,"misses":1,"evictions":1}"#
        );
    }

    #[test]
    fn invalidate_pattern_hits_wildcards_only() {
        let mut c = CacheRegistry::new();
        c.ensure_keyspace("sess", KeyspaceConfig::default());
        for k in ["user:1", "user:2", "order:9"] {
            c.set("sess", k, "x", None);
        }
        assert_eq!(c.invalidate_pattern("sess", "user:*"), 2);
        assert!(c.get("sess", "user:1").is_none());
        assert!(c.get("sess", "order:9").is_some());

        // Exact-match pattern (no wildcard).
        assert_eq!(c.invalidate_pattern("sess", "order:9"), 1);

        // Unknown pattern invalidates nothing.
        assert_eq!(c.invalidate_pattern("sess", "ghost*"), 0);
        // Question-mark wildcard.
        c.set("sess", "ab", "x", None);
        c.set("sess", "abc", "x", None);
        assert_eq!(c.invalidate_pattern("sess", "a?"), 1);
    }

    #[test]
    fn invalidate_all_clears_live_entries() {
        let mut c = CacheRegistry::new();
        c.ensure_keyspace("bulk", KeyspaceConfig::default());
        for i in 0..5 {
            c.set("bulk", &format!("k{i}"), "v", None);
        }
        assert_eq!(c.invalidate_all("bulk"), 5);
        assert_eq!(c.invalidate_all("bulk"), 0, "second sweep finds nothing");
        assert!(c.entries_json("bulk").unwrap().contains(r#""entries":[]"#));
    }

    #[test]
    fn mget_mset_batch_semantics() {
        let mut c = CacheRegistry::new();
        c.ensure_keyspace("batch", KeyspaceConfig::default());
        c.mset(
            "batch",
            &[("k1".into(), "v1".into()), ("k2".into(), "v2".into())],
            None,
        );
        let got = c.mget(
            "batch",
            &["k1".to_string(), "missing".to_string(), "k2".to_string()],
        );
        assert_eq!(got[0].as_deref(), Some("v1"));
        assert!(got[1].is_none(), "missing key → null");
        assert_eq!(got[2].as_deref(), Some("v2"));
    }

    #[test]
    fn unknown_keyspace_operations_are_safe() {
        let mut c = CacheRegistry::new();
        assert!(c.get("ghost", "k").is_none());
        assert!(!c.del("ghost", "k"));
        assert_eq!(c.invalidate_pattern("ghost", "*"), 0);
        assert_eq!(c.invalidate_all("ghost"), 0);
        assert!(c.keyspace_json("ghost").is_none());
        assert!(c.entries_json("ghost").is_none());
    }

    #[test]
    fn implicit_keyspace_creation_on_set() {
        let mut c = CacheRegistry::new();
        c.set("ad-hoc", "k", "v", None);
        assert!(c.has_keyspace("ad-hoc"), "set materializes the keyspace");
    }

    #[test]
    fn ensure_keyspace_is_idempotent_keeps_data() {
        let mut c = CacheRegistry::new();
        c.ensure_keyspace("ks", KeyspaceConfig::default());
        c.set("ks", "k", "v", None);
        c.ensure_keyspace(
            "ks",
            KeyspaceConfig {
                max_entries: 5,
                default_ttl_ms: 60_000,
            },
        );
        assert_eq!(c.get("ks", "k").as_deref(), Some("v"));
        assert!(
            c.keyspace_json("ks")
                .unwrap()
                .contains(r#""max_entries":5"#),
            "config must update"
        );
    }

    #[test]
    fn list_and_status_json_shapes() {
        let mut c = CacheRegistry::new();
        assert_eq!(
            c.status_json(),
            r#"{"keyspaces":0,"entries":0,"hits":0,"misses":0}"#
        );
        c.ensure_keyspace(
            "a",
            KeyspaceConfig {
                max_entries: 7,
                default_ttl_ms: 1000,
            },
        );
        let list = c.list_json();
        assert!(list.contains(r#""name":"a""#));
        assert!(list.contains(r#""max_entries":7"#));
        let status = c.status_json();
        assert!(status.contains(r#""keyspaces":1"#));
    }

    #[test]
    fn glob_match_basics() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("user:*", "user:42"));
        assert!(!glob_match("user:*", "admin:42"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*mid*", "some-middle-thing"));
        assert!(!glob_match("abc", "abd"));
    }
}
