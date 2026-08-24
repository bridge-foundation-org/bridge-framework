//! Bridge authentication — handlers, token validation, session management.
//!
//! Inspired by Encore commits 1426 (tsparser auth-handler),
//! 1560 (propagate-auth-error), 1601 (handle-missing-auth-schema),
//! 1819 (overriding-auth-data-in-ts).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Auth schemes ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scheme {
    Bearer,
    ApiKey,
    None,
}

impl Scheme {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "bearer" => Scheme::Bearer,
            "api_key" | "apikey" | "x-api-key" => Scheme::ApiKey,
            _ => Scheme::None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Scheme::Bearer => "bearer",
            Scheme::ApiKey => "api_key",
            Scheme::None => "none",
        }
    }
}

// ── Auth data ─────────────────────────────────────────────────────────────────

/// Parsed and validated auth data attached to a request.
#[derive(Debug, Clone)]
pub struct AuthData {
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub roles: Vec<String>,
    pub custom: HashMap<String, String>,
    pub expires_at: Option<u64>,
    pub issued_at: u64,
}

impl AuthData {
    pub fn new(user_id: impl Into<String>) -> Self {
        AuthData {
            user_id: Some(user_id.into()),
            email: None,
            roles: Vec::new(),
            custom: HashMap::new(),
            expires_at: None,
            issued_at: now_secs(),
        }
    }

    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }

    pub fn with_expiry(mut self, ttl_secs: u64) -> Self {
        self.expires_at = Some(now_secs() + ttl_secs);
        self
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| now_secs() >= exp)
            .unwrap_or(false)
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    pub fn to_json(&self) -> String {
        let user_id = self.user_id.as_deref().unwrap_or("");
        let email = self.email.as_deref().unwrap_or("");
        let roles = self
            .roles
            .iter()
            .map(|r| format!("\"{}\"", r))
            .collect::<Vec<_>>()
            .join(",");
        let custom: String = self
            .custom
            .iter()
            .map(|(k, v)| format!(",\"{}\":\"{}\"", k, v))
            .collect();
        let exp = self
            .expires_at
            .map(|e| format!(",\"expires_at\":{e}"))
            .unwrap_or_default();
        format!(
            r#"{{"user_id":"{user_id}","email":"{email}","roles":[{roles}],"issued_at":{iat}{exp}{custom}}}"#,
            user_id = user_id,
            email = email,
            roles = roles,
            iat = self.issued_at,
            exp = exp,
            custom = custom,
        )
    }
}

// ── Token store ───────────────────────────────────────────────────────────────

/// Token entry stored in the session registry.
#[derive(Debug, Clone)]
struct TokenEntry {
    scheme: Scheme,
    raw_token: String,
    auth_data: AuthData,
    created_at: u64,
}

/// Thread-safe token/session registry.
#[derive(Clone)]
pub struct AuthRegistry(Arc<Mutex<RegistryInner>>);

struct RegistryInner {
    tokens: HashMap<String, TokenEntry>,   // token → entry
    api_keys: HashMap<String, TokenEntry>, // api_key → entry
}

impl AuthRegistry {
    pub fn new() -> Self {
        AuthRegistry(Arc::new(Mutex::new(RegistryInner {
            tokens: HashMap::new(),
            api_keys: HashMap::new(),
        })))
    }

    /// Register a bearer token with associated auth data.
    pub fn set_bearer(&self, token: impl Into<String>, data: AuthData) {
        let token = token.into();
        let mut inner = self.0.lock().unwrap();
        inner.tokens.insert(
            token.clone(),
            TokenEntry {
                scheme: Scheme::Bearer,
                raw_token: token,
                auth_data: data,
                created_at: now_secs(),
            },
        );
    }

    /// Register an API key with associated auth data.
    pub fn set_api_key(&self, key: impl Into<String>, data: AuthData) {
        let key = key.into();
        let mut inner = self.0.lock().unwrap();
        inner.api_keys.insert(
            key.clone(),
            TokenEntry {
                scheme: Scheme::ApiKey,
                raw_token: key,
                auth_data: data,
                created_at: now_secs(),
            },
        );
    }

    /// Validate a bearer token. Returns `AuthData` or an error string.
    pub fn validate_bearer(&self, token: &str) -> Result<AuthData, String> {
        let inner = self.0.lock().unwrap();
        match inner.tokens.get(token) {
            None => Err("invalid or expired token".to_string()),
            Some(entry) => {
                if entry.auth_data.is_expired() {
                    Err("token has expired".to_string())
                } else {
                    Ok(entry.auth_data.clone())
                }
            }
        }
    }

    /// Validate an API key.
    pub fn validate_api_key(&self, key: &str) -> Result<AuthData, String> {
        let inner = self.0.lock().unwrap();
        match inner.api_keys.get(key) {
            None => Err("invalid API key".to_string()),
            Some(entry) => {
                if entry.auth_data.is_expired() {
                    Err("API key has expired".to_string())
                } else {
                    Ok(entry.auth_data.clone())
                }
            }
        }
    }

    /// Parse `Authorization` header and validate.
    ///
    /// Supports:
    /// - `Bearer <token>`
    /// - `ApiKey <key>`  
    /// - `X-API-Key: <key>` (passed as header value)
    pub fn validate_header(&self, header: &str) -> Result<AuthData, String> {
        let header = header.trim();
        if let Some(token) = header.strip_prefix("Bearer ") {
            return self.validate_bearer(token.trim());
        }
        if let Some(key) = header.strip_prefix("ApiKey ") {
            return self.validate_api_key(key.trim());
        }
        // Try as raw API key
        if self.validate_api_key(header).is_ok() {
            return self.validate_api_key(header);
        }
        Err("unsupported authentication scheme".to_string())
    }

    /// Remove a bearer token.
    pub fn revoke_bearer(&self, token: &str) {
        self.0.lock().unwrap().tokens.remove(token);
    }

    /// Remove an API key.
    pub fn revoke_api_key(&self, key: &str) {
        self.0.lock().unwrap().api_keys.remove(key);
    }

    /// Clear all tokens and keys.
    pub fn clear(&self) {
        let mut inner = self.0.lock().unwrap();
        inner.tokens.clear();
        inner.api_keys.clear();
    }

    /// Status JSON for the `auth-status` CLI command.
    pub fn status_json(&self) -> String {
        let inner = self.0.lock().unwrap();
        let bearer_count = inner.tokens.len();
        let api_key_count = inner.api_keys.len();
        let has_auth = bearer_count > 0 || api_key_count > 0;
        format!(
            r#"{{"authenticated":{has_auth},"bearer_tokens":{bearer_count},"api_keys":{api_key_count}}}"#
        )
    }
}

impl Default for AuthRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Request auth extraction ───────────────────────────────────────────────────

/// Extract token from common HTTP header locations.
pub fn extract_token(headers: &[(String, String)]) -> Option<String> {
    for (key, value) in headers {
        let k = key.to_lowercase();
        if k == "authorization" {
            return Some(value.clone());
        }
        if k == "x-api-key" {
            return Some(value.clone());
        }
    }
    None
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_parse() {
        assert_eq!(Scheme::parse("bearer"), Scheme::Bearer);
        assert_eq!(Scheme::parse("api_key"), Scheme::ApiKey);
        assert_eq!(Scheme::parse("unknown"), Scheme::None);
    }

    #[test]
    fn auth_data_roles() {
        let data = AuthData::new("user-1")
            .with_role("admin")
            .with_role("editor");
        assert!(data.has_role("admin"));
        assert!(data.has_role("editor"));
        assert!(!data.has_role("viewer"));
    }

    #[test]
    fn auth_data_expiry() {
        let expired = AuthData::new("user-1").with_expiry(0); // already expired
                                                              // Sleep 1ms to ensure timestamp passes
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(expired.is_expired());

        let valid = AuthData::new("user-2").with_expiry(3600);
        assert!(!valid.is_expired());
    }

    #[test]
    fn auth_data_json() {
        let data = AuthData::new("u123")
            .with_email("user@example.com")
            .with_role("admin")
            .with_field("org", "acme");
        let json = data.to_json();
        assert!(json.contains("\"user_id\":\"u123\""));
        assert!(json.contains("\"email\":\"user@example.com\""));
        assert!(json.contains("\"admin\""));
        assert!(json.contains("\"org\":\"acme\""));
    }

    #[test]
    fn registry_bearer_valid() {
        let reg = AuthRegistry::new();
        let data = AuthData::new("user-1").with_expiry(3600);
        reg.set_bearer("token-abc", data);
        let result = reg.validate_bearer("token-abc");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn registry_bearer_invalid() {
        let reg = AuthRegistry::new();
        let result = reg.validate_bearer("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn registry_api_key_valid() {
        let reg = AuthRegistry::new();
        reg.set_api_key("key-xyz", AuthData::new("svc-1").with_expiry(3600));
        assert!(reg.validate_api_key("key-xyz").is_ok());
    }

    #[test]
    fn registry_header_bearer() {
        let reg = AuthRegistry::new();
        reg.set_bearer("tok123", AuthData::new("u1").with_expiry(3600));
        let result = reg.validate_header("Bearer tok123");
        assert!(result.is_ok());
    }

    #[test]
    fn registry_revoke() {
        let reg = AuthRegistry::new();
        reg.set_bearer("to-revoke", AuthData::new("u1").with_expiry(3600));
        reg.revoke_bearer("to-revoke");
        assert!(reg.validate_bearer("to-revoke").is_err());
    }

    #[test]
    fn registry_clear() {
        let reg = AuthRegistry::new();
        reg.set_bearer("tok", AuthData::new("u1").with_expiry(3600));
        reg.set_api_key("key", AuthData::new("s1").with_expiry(3600));
        reg.clear();
        let status = reg.status_json();
        assert!(status.contains("\"bearer_tokens\":0"));
        assert!(status.contains("\"api_keys\":0"));
    }

    #[test]
    fn extract_token_authorization_header() {
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Authorization".to_string(), "Bearer my-token".to_string()),
        ];
        assert_eq!(extract_token(&headers), Some("Bearer my-token".to_string()));
    }

    #[test]
    fn extract_token_x_api_key() {
        let headers = vec![("X-API-Key".to_string(), "my-key-123".to_string())];
        assert_eq!(extract_token(&headers), Some("my-key-123".to_string()));
    }
}
