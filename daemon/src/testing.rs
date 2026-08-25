//! Testing support surface (Encore `testing` package parity).
//!
//! Provides the daemon-side primitives an app test harness needs:
//! - **Test databases**: uniquely-namespaced instances with tracked
//!   ownership so a single cleanup call tears everything down
//!   (Encore `NewTestDatabase`, migrator/superuser roles).
//! - **Test mode**: flips the daemon into test semantics with a
//!   default log level override (Encore commit 1423, 1885).
//! - **Mocking**: auth bypass with a canned principal (commit 1737,
//!   1819) and canned service responses.
//!
//! Inspired by Encore commits 1273 (NewTestDatabase), 1423 (test log
//! levels), 1737 (auth mocking), 1885 (test mode), 2158/2163
//! (migrator/superuser test roles).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use std::collections::BTreeMap;

// ── Model ─────────────────────────────────────────────────────────────────────

/// One provisioned test database. The namespace is unique per instance
/// so concurrent tests never collide even with the same base name.
#[derive(Debug, Clone)]
pub struct TestDatabase {
    /// Caller-chosen logical name (may repeat across instances).
    pub name: String,
    /// Isolation namespace: `t{seq}_{name}`.
    pub namespace: String,
    /// Migrator/superuser role granted (Encore 2158/2163).
    pub superuser: bool,
}

/// Active mocking configuration.
#[derive(Debug, Clone, Default)]
pub struct Mocks {
    /// When set, auth checks pass as this principal instead of
    /// performing real verification.
    pub auth_principal: Option<String>,
    /// Canned per-service responses (`service name` → raw JSON body).
    pub services: BTreeMap<String, String>,
}

/// Test-mode configuration (Encore `testing.test()`).
#[derive(Debug, Clone)]
pub struct TestMode {
    /// Default log level while testing (keeps output quiet).
    pub log_level: String,
}

/// Registry of everything the test harness provisions.
#[derive(Debug, Clone, Default)]
pub struct TestRegistry {
    /// Live test databases keyed by their assigned namespace.
    pub databases: BTreeMap<String, TestDatabase>,
    /// Monotonic counter for namespace generation (deterministic).
    next_seq: u64,
    /// Mocks active for the current test run.
    pub mocks: Mocks,
    /// Set while a test run is active.
    pub mode: Option<TestMode>,
}

// ── API ───────────────────────────────────────────────────────────────────────

impl TestRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Provision a new isolated test database. Returns the assigned
    /// namespace. `superuser` maps to Encore's migrator/superuser
    /// test roles.
    pub fn new_database(&mut self, name: &str, superuser: bool) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("database name required".into());
        }
        self.next_seq += 1;
        let seq = self.next_seq;
        let namespace = format!("t{seq}_{name}");
        self.databases.insert(
            namespace.clone(),
            TestDatabase {
                name: name.to_string(),
                namespace: namespace.clone(),
                superuser,
            },
        );
        Ok(namespace)
    }

    /// Destroy every live test database. Returns how many were torn down.
    pub fn cleanup_databases(&mut self) -> usize {
        let n = self.databases.len();
        self.databases.clear();
        n
    }

    /// Enter test mode. Empty/unknown levels fall back to `error`
    /// (quiet-by-default, matching Encore's test harness).
    pub fn enter_mode(&mut self, log_level: &str) {
        let lvl = log_level.trim().to_lowercase();
        let lvl = matches!(lvl.as_str(), "trace" | "debug" | "info" | "warn" | "error")
            .then_some(lvl)
            .unwrap_or_else(|| "error".into());
        self.mode = Some(TestMode { log_level: lvl });
    }

    /// Exit test mode. Returns false when it was not active.
    pub fn exit_mode(&mut self) -> bool {
        self.mode.take().is_some()
    }

    /// Enable auth mocking: every auth check passes as `principal`.
    pub fn mock_auth(&mut self, principal: &str) -> Result<(), String> {
        let p = principal.trim();
        if p.is_empty() {
            return Err("principal required".into());
        }
        self.mocks.auth_principal = Some(p.to_string());
        Ok(())
    }

    /// Register a canned service response (raw JSON body verbatim).
    pub fn mock_service(&mut self, service: &str, response: &str) -> Result<(), String> {
        let s = service.trim();
        if s.is_empty() {
            return Err("service name required".into());
        }
        self.mocks
            .services
            .insert(s.to_string(), response.to_string());
        Ok(())
    }

    /// Clear all mocks (auth + services). Returns count cleared.
    pub fn clear_mocks(&mut self) -> usize {
        let mut n = usize::from(self.mocks.auth_principal.is_some());
        n += self.mocks.services.len();
        self.mocks = Mocks::default();
        n
    }

    /// Full snapshot for `GET /api/v1/testing`.
    pub fn to_json(&self) -> String {
        let dbs: Vec<String> = self
            .databases
            .values()
            .map(|d| {
                format!(
                    r#"{{"name":"{}","namespace":"{}","superuser":{}}}"#,
                    d.name, d.namespace, d.superuser
                )
            })
            .collect();
        let svcs: Vec<String> = self
            .mocks
            .services
            .iter()
            .map(|(k, v)| format!(r#""{k}":{v}"#))
            .collect();
        let auth = match &self.mocks.auth_principal {
            Some(p) => format!(r#"{{"enabled":true,"principal":"{p}"}}"#),
            None => r#"{"enabled":false}"#.to_string(),
        };
        let mode = match &self.mode {
            Some(m) => format!(r#"{{"active":true,"log_level":"{}"}}"#, m.log_level),
            None => r#"{"active":false}"#.to_string(),
        };
        format!(
            r#"{{"mode":{mode},"databases":[{}],"mocks":{{"auth":{auth},"services":{{{}}}}}}}"#,
            dbs.join(","),
            svcs.join(","),
        )
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_databases_get_unique_namespaces() {
        let mut t = TestRegistry::new();
        let ns1 = t.new_database("users", true).unwrap();
        let ns2 = t.new_database("users", false).unwrap();
        assert_ne!(ns1, ns2, "same base name must isolate");
        assert!(ns1.starts_with("t1_users"));
        assert!(ns2.starts_with("t2_users"));
        assert!(t.new_database("", true).is_err());
        assert_eq!(t.databases.len(), 2);
    }

    #[test]
    fn test_cleanup_destroys_everything() {
        let mut t = TestRegistry::new();
        t.new_database("a", false).unwrap();
        t.new_database("b", false).unwrap();
        assert_eq!(t.cleanup_databases(), 2);
        assert!(t.databases.is_empty());
        assert_eq!(t.cleanup_databases(), 0);
    }

    #[test]
    fn test_mode_log_level_validation() {
        let mut t = TestRegistry::new();
        t.enter_mode("warn");
        assert_eq!(t.mode.as_ref().unwrap().log_level, "warn");
        t.enter_mode("bogus");
        assert_eq!(t.mode.as_ref().unwrap().log_level, "error", "fallback");
        assert!(t.exit_mode());
        assert!(!t.exit_mode(), "second exit is a no-op");
    }

    #[test]
    fn test_auth_mock_roundtrip_and_validation() {
        let mut t = TestRegistry::new();
        assert!(t.mock_auth("u_123").is_ok());
        assert_eq!(t.mocks.auth_principal.as_deref(), Some("u_123"));
        assert!(t.mock_auth("  ").is_err(), "blank principal rejected");
    }

    #[test]
    fn test_service_mock_canned_responses() {
        let mut t = TestRegistry::new();
        t.mock_service("auth", r#"{"user":"u_1"}"#).unwrap();
        assert!(t.mock_service("", "{}").is_err());
        assert_eq!(
            t.mocks.services.get("auth").map(String::as_str),
            Some(r#"{"user":"u_1"}"#)
        );
    }

    #[test]
    fn test_snapshot_json_shape() {
        let mut t = TestRegistry::new();
        assert!(t
            .to_json()
            .starts_with(r#"{"mode":{"active":false},"databases":[],"mocks":{"auth":{"enabled":false},"services":{}}}"#));
        t.enter_mode("debug");
        t.new_database("db", true).unwrap();
        t.mock_auth("u_9").unwrap();
        let j = t.to_json();
        assert!(j.contains(r#""mode":{"active":true,"log_level":"debug"}"#));
        assert!(j.contains(r#""namespace":"t1_db","superuser":true"#));
        assert!(j.contains(r#""auth":{"enabled":true,"principal":"u_9"}"#));
    }
}
