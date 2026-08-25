//! Bridge infra config — runtime environment variables, service discovery,
//! database config, and TLS status (Encore `infra.Config` surface).
//!
//! Inspired by Encore commits 1797 (infra config), 1809 (config in traces),
//! 2000-2033 (runtime config plumbing).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use std::collections::BTreeMap;

/// Runtime infrastructure configuration for the app.
#[derive(Debug, Clone, Default)]
pub struct InfraConfig {
    /// Declared env vars (name → value). Values are plain; secrets live
    /// in the secrets registry, not here.
    pub env_vars: BTreeMap<String, String>,
    /// Discovered services: name → comma-free address list.
    pub services: Vec<ServiceEndpoint>,
    /// Per-database config (name → connection settings).
    pub databases: BTreeMap<String, DatabaseConfig>,
    /// TLS termination status for the gateway.
    pub tls: Option<TlsStatus>,
}

#[derive(Debug, Clone)]
pub struct ServiceEndpoint {
    pub name: String,
    pub addr: String,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub engine: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct TlsStatus {
    pub enabled: bool,
    pub cert_path: Option<String>,
}

impl InfraConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set an env var. Empty value removes it.
    pub fn set_env_var(&mut self, name: &str, value: &str) {
        if value.is_empty() {
            self.env_vars.remove(name);
        } else {
            self.env_vars.insert(name.to_string(), value.to_string());
        }
    }

    /// Register/replace a service endpoint.
    pub fn register_service(&mut self, name: &str, addr: &str) -> bool {
        if name.is_empty() || addr.is_empty() || !addr.contains(':') && addr != "unix" {
            return false;
        }
        if let Some(e) = self.services.iter_mut().find(|e| e.name == name) {
            e.addr = addr.to_string();
            return false;
        }
        self.services.push(ServiceEndpoint {
            name: name.to_string(),
            addr: addr.to_string(),
        });
        true
    }

    /// Upsert a database config. Validates port > 0 and known engines.
    pub fn upsert_database(
        &mut self,
        name: &str,
        engine: &str,
        host: &str,
        port: u16,
    ) -> Result<(), String> {
        if name.is_empty() || host.is_empty() {
            return Err("database name and host required".to_string());
        }
        if !matches!(engine, "postgres" | "mysql" | "sqlite") {
            return Err(format!(
                "unsupported engine {engine} (postgres|mysql|sqlite)"
            ));
        }
        if port == 0 {
            return Err("port must be 1-65535".to_string());
        }
        self.databases.insert(
            name.to_string(),
            DatabaseConfig {
                engine: engine.to_string(),
                host: host.to_string(),
                port,
            },
        );
        Ok(())
    }

    pub fn set_tls(&mut self, enabled: bool, cert_path: Option<String>) {
        self.tls = Some(TlsStatus { enabled, cert_path });
    }

    pub fn service_json(&self) -> String {
        let items: Vec<String> = self
            .services
            .iter()
            .map(|e| format!(r#"{{"name":"{}","addr":"{}"}}"#, e.name, e.addr))
            .collect();
        format!(r#"{{"services":[{items}]}}"#, items = items.join(","))
    }

    pub fn databases_json(&self) -> String {
        let items: Vec<String> = self
            .databases
            .iter()
            .map(|(n, d)| {
                format!(
                    r#"{{"name":"{}","engine":"{}","host":"{}","port":{}}}"#,
                    n, d.engine, d.host, d.port
                )
            })
            .collect();
        format!(r#"{{"databases":[{items}]}}"#, items = items.join(","))
    }

    pub fn tls_json(&self) -> String {
        match &self.tls {
            Some(t) => format!(
                r#"{{"enabled":{},"cert":{}}}"#,
                t.enabled,
                t.cert_path
                    .as_deref()
                    .map(|c| format!(r#""{c}""#))
                    .unwrap_or_else(|| "null".into()),
            ),
            None => r#"{"configured":false}"#.to_string(),
        }
    }

    /// Full snapshot — env vars sorted (BTreeMap), then discovery/db/tls.
    pub fn to_json(&self) -> String {
        let env_items: Vec<String> = self
            .env_vars
            .iter()
            .map(|(k, v)| format!(r#""{k}":"{v}""#))
            .collect();
        format!(
            r#"{{"env_vars":{{{}}},"services":[{}],"databases":[{}],"tls":{}}}"#,
            env_items.join(","),
            self.service_items(),
            self.database_items(),
            self.tls_json(),
        )
    }

    fn service_items(&self) -> String {
        self.services
            .iter()
            .map(|e| format!(r#"{{"name":"{}","addr":"{}"}}"#, e.name, e.addr))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn database_items(&self) -> String {
        self.databases
            .iter()
            .map(|(n, d)| {
                format!(
                    r#"{{"name":"{}","engine":"{}","host":"{}","port":{}}}"#,
                    n, d.engine, d.host, d.port
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_vars_set_remove_and_sorted_output() {
        let mut c = InfraConfig::new();
        c.set_env_var("Z_VAR", "1");
        c.set_env_var("A_VAR", "2");
        let json = c.to_json();
        assert!(
            json.find("A_VAR").unwrap() < json.find("Z_VAR").unwrap(),
            "sorted"
        );
        c.set_env_var("Z_VAR", ""); // empty removes
        let after = c.to_json();
        assert!(!after.contains("Z_VAR"));
        assert!(after.contains("A_VAR"));
    }

    #[test]
    fn service_registration_and_replacement() {
        let mut c = InfraConfig::new();
        assert!(c.register_service("auth", "127.0.0.1:9001"), "first is new");
        assert!(
            !c.register_service("auth", "127.0.0.1:9002"),
            "second updates"
        );
        assert!(!c.register_service("", "x:1"));
        assert!(!c.register_service("bad", ""));
        assert!(c.service_json().contains("9002"));
        assert_eq!(c.services.len(), 1);
    }

    #[test]
    fn database_upsert_validation() {
        let mut c = InfraConfig::new();
        assert!(c
            .upsert_database("db", "postgres", "localhost", 5432)
            .is_ok());
        assert!(
            c.upsert_database("db", "oracle", "h", 1).is_err(),
            "unknown engine"
        );
        assert!(
            c.upsert_database("db", "postgres", "h", 0).is_err(),
            "port 0"
        );
        assert!(
            c.upsert_database("", "postgres", "h", 1).is_err(),
            "no name"
        );
        assert!(c.databases.contains_key("db"));
        assert!(c.databases_json().contains("5432"));
    }

    #[test]
    fn tls_status_shape() {
        let mut c = InfraConfig::new();
        assert_eq!(c.tls_json(), r#"{"configured":false}"#);
        c.set_tls(true, Some("/certs/a.pem".into()));
        assert_eq!(c.tls_json(), r#"{"enabled":true,"cert":"/certs/a.pem"}"#);
    }

    #[test]
    fn full_snapshot_json_shape() {
        let mut c = InfraConfig::new();
        c.register_service("svc", "h:1");
        let j = c.to_json();
        assert!(j.starts_with(r#"{"env_vars":{"#));
        assert!(j.contains(r#""services":[{"name":"svc","addr":"h:1"}]"#));
        assert!(j.contains(r#""databases":[]"#));
        assert!(j.contains(r#""tls":{"configured":false}"#));
    }
}
