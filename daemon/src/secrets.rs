//! Bridge secrets management — env-based, gzipped payloads, vault stubs.
//!
//! Inspired by Encore commits 1950 (gzip app secrets),
//! 2065-2066 (secrets UX), 2078 (secrets delete command),
//! 2085 (gzip secret data), 2185 (external vault support),
//! 2193-2194 (secret splitting across multiple parts).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ── Secret value ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum SecretSource {
    /// Plain env var or directly set.
    Environment(String),
    /// File-based secret (path to file).
    File(String),
    /// Inline value (for testing/development).
    Inline(String),
    /// Stub for external vault (HashiCorp, AWS Secrets Manager, GCP SM).
    ExternalVault { provider: String, path: String },
}

#[derive(Debug, Clone)]
pub struct Secret {
    pub name:     String,
    pub source:   SecretSource,
    pub redacted: bool,
}

impl Secret {
    /// Resolve the actual secret value. Returns `None` if not available.
    pub fn resolve(&self) -> Option<String> {
        match &self.source {
            SecretSource::Environment(var_name) => {
                std::env::var(var_name).ok()
            }
            SecretSource::File(path) => {
                std::fs::read_to_string(path).ok()
                    .map(|s| s.trim().to_string())
            }
            SecretSource::Inline(value) => Some(value.clone()),
            SecretSource::ExternalVault { .. } => {
                // In production: call vault API. For local dev: env var fallback.
                std::env::var(&self.name.to_uppercase().replace('-', "_")).ok()
            }
        }
    }

    /// Display value (redacted if configured).
    pub fn display_value(&self) -> String {
        if self.redacted {
            return "***".to_string();
        }
        self.resolve().unwrap_or_else(|| "<not set>".to_string())
    }

    pub fn is_set(&self) -> bool {
        self.resolve().is_some()
    }
}

// ── Secrets registry ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SecretsRegistry(Arc<Mutex<RegistryInner>>);

struct RegistryInner {
    secrets: HashMap<String, Secret>,
}

impl SecretsRegistry {
    pub fn new() -> Self {
        SecretsRegistry(Arc::new(Mutex::new(RegistryInner {
            secrets: HashMap::new(),
        })))
    }

    /// Register an env-based secret.
    pub fn register_env(&self, name: &str, env_var: &str) {
        let mut inner = self.0.lock().unwrap();
        inner.secrets.insert(name.to_string(), Secret {
            name:     name.to_string(),
            source:   SecretSource::Environment(env_var.to_string()),
            redacted: true,
        });
    }

    /// Register an inline secret (development/testing only).
    pub fn register_inline(&self, name: &str, value: &str) {
        let mut inner = self.0.lock().unwrap();
        inner.secrets.insert(name.to_string(), Secret {
            name:     name.to_string(),
            source:   SecretSource::Inline(value.to_string()),
            redacted: true,
        });
    }

    /// Register an external vault secret.
    pub fn register_vault(&self, name: &str, provider: &str, path: &str) {
        let mut inner = self.0.lock().unwrap();
        inner.secrets.insert(name.to_string(), Secret {
            name:     name.to_string(),
            source:   SecretSource::ExternalVault {
                provider: provider.to_string(),
                path:     path.to_string(),
            },
            redacted: true,
        });
    }

    /// Get the resolved value of a secret.
    pub fn get(&self, name: &str) -> Option<String> {
        let inner = self.0.lock().unwrap();
        inner.secrets.get(name)?.resolve()
    }

    /// Delete a registered secret.
    pub fn delete(&self, name: &str) -> bool {
        self.0.lock().unwrap().secrets.remove(name).is_some()
    }

    /// List all registered secret names and their status.
    pub fn list_json(&self) -> String {
        let inner = self.0.lock().unwrap();
        let parts: Vec<String> = inner.secrets.values().map(|s| {
            let source_kind = match &s.source {
                SecretSource::Environment(var) => format!("env:{var}"),
                SecretSource::File(path)       => format!("file:{path}"),
                SecretSource::Inline(_)        => "inline".to_string(),
                SecretSource::ExternalVault { provider, .. } => format!("vault:{provider}"),
            };
            format!(
                r#"{{"name":"{name}","source":"{source}","set":{set}}}"#,
                name   = s.name,
                source = source_kind,
                set    = s.is_set(),
            )
        }).collect();
        format!("[{}]", parts.join(","))
    }

    /// Check required secrets are all set. Returns names of missing secrets.
    pub fn check_required(&self, required: &[&str]) -> Vec<String> {
        let inner = self.0.lock().unwrap();
        required.iter()
            .filter(|&&name| {
                inner.secrets.get(name)
                    .map(|s| !s.is_set())
                    .unwrap_or(true) // missing from registry = not set
            })
            .map(|s| s.to_string())
            .collect()
    }

    /// Number of registered secrets.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap().secrets.len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
}

impl Default for SecretsRegistry {
    fn default() -> Self { Self::new() }
}

// ── Simple gzip-like encoding (RLE for secret payloads) ───────────────────────
// Real gzip needs external crate; this is a simple length-prefix codec
// that keeps the "gzip secrets" API surface for future proper impl.

pub mod compress {
    /// Encode bytes as base64-ish (for transport over text protocols).
    /// In production this would be real gzip + base64.
    pub fn encode(data: &[u8]) -> String {
        // Simple hex encoding (no external deps)
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub fn decode(hex: &str) -> Result<Vec<u8>, String> {
        let hex = hex.trim();
        if hex.len() % 2 != 0 {
            return Err("invalid hex length".to_string());
        }
        (0..hex.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&hex[i..i+2], 16)
                    .map_err(|e| e.to_string())
            })
            .collect()
    }

    pub fn encode_str(s: &str) -> String { encode(s.as_bytes()) }

    pub fn decode_str(hex: &str) -> Result<String, String> {
        decode(hex).and_then(|b|
            String::from_utf8(b).map_err(|e| e.to_string())
        )
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_secret_resolves() {
        let reg = SecretsRegistry::new();
        reg.register_inline("db_password", "super-secret-pw");
        assert_eq!(reg.get("db_password"), Some("super-secret-pw".to_string()));
    }

    #[test]
    fn env_secret_resolves() {
        std::env::set_var("TEST_SECRET_VAR", "env-value-123");
        let reg = SecretsRegistry::new();
        reg.register_env("my_token", "TEST_SECRET_VAR");
        assert_eq!(reg.get("my_token"), Some("env-value-123".to_string()));
        std::env::remove_var("TEST_SECRET_VAR");
    }

    #[test]
    fn missing_secret_returns_none() {
        let reg = SecretsRegistry::new();
        assert_eq!(reg.get("nonexistent"), None);
    }

    #[test]
    fn delete_secret() {
        let reg = SecretsRegistry::new();
        reg.register_inline("temp", "value");
        assert!(reg.delete("temp"));
        assert_eq!(reg.get("temp"), None);
        assert!(!reg.delete("temp")); // second delete returns false
    }

    #[test]
    fn list_json_includes_all() {
        let reg = SecretsRegistry::new();
        reg.register_inline("alpha", "a");
        reg.register_env("beta", "SOME_ENV_THAT_DOESNT_EXIST");
        let json = reg.list_json();
        assert!(json.contains("\"name\":\"alpha\""));
        assert!(json.contains("\"set\":true"));
        assert!(json.contains("\"name\":\"beta\""));
        assert!(json.contains("\"set\":false"));
    }

    #[test]
    fn check_required_finds_missing() {
        let reg = SecretsRegistry::new();
        reg.register_inline("set_secret", "value");
        let missing = reg.check_required(&["set_secret", "missing_one", "missing_two"]);
        assert_eq!(missing.len(), 2);
        assert!(missing.contains(&"missing_one".to_string()));
    }

    #[test]
    fn compress_roundtrip() {
        let original = "hello, Bridge secrets!";
        let encoded  = compress::encode_str(original);
        let decoded  = compress::decode_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn compress_invalid_hex() {
        assert!(compress::decode("xyz").is_err());
    }
}
