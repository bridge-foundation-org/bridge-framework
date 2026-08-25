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

    /// Sign a JWT and register it as a live bearer session so the standard
    /// token pipeline accepts it. Returns the encoded token.
    pub fn issue_jwt(&self, claims: JwtClaims, secret: &[u8]) -> String {
        let token = jwt_sign(&claims, secret);
        self.set_bearer(token.clone(), claims.to_auth_data());
        token
    }

    /// Verify a bearer token. JWT-shaped tokens (two dots) MUST pass
    /// cryptographic verification — a failed JWT never falls back to the
    /// session registry (that would let tampered tokens through). Opaque
    /// tokens hit the registry directly.
    pub fn authenticate(&self, token: &str, secret: &[u8]) -> Result<AuthData, String> {
        if token.matches('.').count() == 2 {
            return jwt_verify(token, secret, now_secs()).map(|claims| claims.to_auth_data());
        }
        self.validate_bearer(token)
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

// ── Base64url (RFC 4648 §5, unpadded) ─────────────────────────────────────────

const B64URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Unpadded base64url encode (JWT segment encoding).
pub fn base64url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64URL_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(B64URL_ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64URL_ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL_ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// Unpadded base64url decode. Accepts stray `=` padding on input.
pub fn base64url_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'-' => Ok(62),
            b'_' => Ok(63),
            _ => Err(format!("invalid base64url character {:?}", c as char)),
        }
    }
    let trimmed = s.trim_end_matches('=');
    let bytes = trimmed.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4 + 1);
    for chunk in bytes.chunks(4) {
        let mut n: u32 = 0;
        for &c in chunk {
            n = (n << 6) | val(c)?;
        }
        match chunk.len() {
            4 => {
                out.push((n >> 16) as u8);
                out.push((n >> 8) as u8);
                out.push(n as u8);
            }
            3 => {
                n <<= 6;
                out.push((n >> 16) as u8);
                out.push((n >> 8) as u8);
            }
            2 => {
                n <<= 12;
                out.push((n >> 16) as u8);
            }
            _ => return Err("truncated base64url segment".into()),
        }
    }
    Ok(out)
}

// ── JWT (HS256, RFC 7519 / RFC 7515) ──────────────────────────────────────────

/// Parsed JWT claim set. Known registered claims are typed; every other
/// scalar claim lands in `custom` (values kept as their raw string form).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JwtClaims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    /// Expiry — unix seconds. Verification rejects `now >= exp`.
    pub exp: Option<u64>,
    /// Issued-at — unix seconds.
    pub iat: Option<u64>,
    /// Scope list (from a space-delimited `scope` claim).
    pub scopes: Vec<String>,
    pub custom: std::collections::BTreeMap<String, String>,
}

impl JwtClaims {
    pub fn new(sub: impl Into<String>) -> Self {
        JwtClaims {
            sub: Some(sub.into()),
            ..Default::default()
        }
    }

    pub fn with_issuer(mut self, iss: impl Into<String>) -> Self {
        self.iss = Some(iss.into());
        self
    }

    pub fn with_ttl(mut self, ttl_secs: u64) -> Self {
        self.iat = Some(now_secs());
        self.exp = Some(now_secs() + ttl_secs);
        self
    }

    pub fn with_claim(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }

    /// Deterministic compact JSON (custom claims sorted by key).
    pub fn to_json(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(sub) = &self.sub {
            parts.push(format!(r#""sub":"{}""#, escape_json_string(sub)));
        }
        if let Some(iss) = &self.iss {
            parts.push(format!(r#""iss":"{}""#, escape_json_string(iss)));
        }
        if let Some(iat) = self.iat {
            parts.push(format!(r#""iat":{iat}"#));
        }
        if let Some(exp) = self.exp {
            parts.push(format!(r#""exp":{exp}"#));
        }
        if !self.scopes.is_empty() {
            let joined = self.scopes.join(" ");
            parts.push(format!(r#""scope":"{}""#, escape_json_string(&joined)));
        }
        for (k, v) in &self.custom {
            parts.push(format!(
                r#""{}":"{}""#,
                escape_json_string(k),
                escape_json_string(v)
            ));
        }
        format!("{{{}}}", parts.join(","))
    }

    /// Parse a flat JSON object of claims. Registered names are consumed;
    /// unknown scalars go to `custom`.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let pairs = scan_flat_object(json);
        let mut claims = JwtClaims::default();
        for (key, raw) in pairs {
            match key.as_str() {
                "sub" => claims.sub = Some(raw),
                "iss" => claims.iss = Some(raw),
                "iat" => claims.iat = Some(raw.parse().map_err(|_| "bad iat claim")?),
                "exp" => claims.exp = Some(raw.parse().map_err(|_| "bad exp claim")?),
                "scope" | "scopes" => {
                    claims.scopes = raw.split_whitespace().map(String::from).collect();
                }
                _ => {
                    claims.custom.insert(key, raw);
                }
            }
        }
        Ok(claims)
    }

    /// Convert to pipeline [`AuthData`] (sub → user_id, scope → roles).
    pub fn to_auth_data(&self) -> AuthData {
        let mut data = AuthData::new(self.sub.clone().unwrap_or_default());
        data.issued_at = self.iat.unwrap_or_else(now_secs);
        data.expires_at = self.exp;
        if let Some(email) = self.custom.get("email") {
            data.email = Some(email.clone());
        }
        data.roles = self.scopes.clone();
        data.custom = self
            .custom
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        data
    }
}

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Scan a flat JSON object into (key, raw-value) pairs.
/// String values are unquoted; numbers/bools keep their literal text.
fn scan_flat_object(json: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let body = json.trim();
    let Some(body) = body.strip_prefix('{') else {
        return out;
    };
    let body = body.trim_end_matches('}');
    let mut chars = body.chars().peekable();
    loop {
        // Skip whitespace and separators
        while matches!(chars.peek(), Some(c) if c.is_whitespace() || *c == ',') {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        // Key
        if chars.next() != Some('"') {
            break; // malformed — stop conservatively
        }
        let mut key = String::new();
        loop {
            match chars.next() {
                None | Some(',') => break, // malformed or empty key — stop
                Some('"') => break,
                Some('\\') => {
                    if let Some(esc) = chars.next() {
                        key.push(match esc {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            other => other,
                        });
                    }
                }
                Some(c) => key.push(c),
            }
        }
        // Colon
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        if chars.next() != Some(':') {
            break;
        }
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        // Value
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            loop {
                match chars.next() {
                    None | Some(',') => break,
                    Some('"') => break,
                    Some('\\') => {
                        if let Some(esc) = chars.next() {
                            value.push(match esc {
                                'n' => '\n',
                                't' => '\t',
                                'r' => '\r',
                                other => other,
                            });
                        }
                    }
                    Some(c) => value.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                value.push(c);
                chars.next();
            }
            value = value.trim().to_string();
        }
        out.push((key, value));
    }
    out
}

/// Sign claims as an HS256 JWT (`header.payload.signature`, unpadded b64url).
pub fn jwt_sign(claims: &JwtClaims, secret: &[u8]) -> String {
    let header = base64url_encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = base64url_encode(claims.to_json().as_bytes());
    let signing_input = format!("{header}.{payload}");
    let sig = crate::staticfiles::hmac_sha256(secret, signing_input.as_bytes());
    format!("{signing_input}.{}", base64url_encode(&sig))
}

/// Verify an HS256 JWT: structure, signature, and expiry (`now < exp`).
/// Signature comparison is constant-time.
pub fn jwt_verify(token: &str, secret: &[u8], now: u64) -> Result<JwtClaims, String> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return Err("malformed JWT: expected header.payload.signature".into());
    }
    // Header alg must be HS256 (we refuse none/other algorithms outright).
    let header_bytes = base64url_decode(parts[0])?;
    let header = String::from_utf8(header_bytes).map_err(|_| "invalid header encoding")?;
    if !header.contains(r#""alg":"HS256""#) && !header.contains(r#""alg": "HS256""#) {
        return Err("unsupported JWT algorithm (only HS256 accepted)".into());
    }

    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let expected = crate::staticfiles::hmac_sha256(secret, signing_input.as_bytes());
    let got = base64url_decode(parts[2])?;
    // Constant-time comparison: fold every byte XOR, seed with length diff.
    let mut diff = (got.len() ^ expected.len()) as u8;
    for (a, b) in got.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return Err("signature mismatch".into());
    }

    let payload = String::from_utf8(base64url_decode(parts[1])?)
        .map_err(|_| "invalid UTF-8 in JWT payload")?;
    let claims = JwtClaims::from_json(&payload)?;
    if let Some(exp) = claims.exp {
        if now >= exp {
            return Err("token expired".into());
        }
    }
    Ok(claims)
}

/// Extract a bearer token from an `Authorization` header value.
pub fn bearer_from_header(header: Option<&str>) -> Option<&str> {
    header
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
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

    // ── Base64url ────────────────────────────────────────────────────────────

    #[test]
    fn base64url_known_vectors() {
        // RFC 4648 test vectors (pad with '=' in RFC; we emit unpadded).
        assert_eq!(base64url_encode(b""), "");
        assert_eq!(base64url_encode(b"f"), "Zg");
        assert_eq!(base64url_encode(b"fo"), "Zm8");
        assert_eq!(base64url_encode(b"foo"), "Zm9v");
        assert_eq!(base64url_encode(b"foob"), "Zm9vYg");
        assert_eq!(base64url_encode(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url_encode(b"foobar"), "Zm9vYmFy");
        // URL-safe alphabet: bytes that produce +/ in standard b64 become -_
        assert_eq!(base64url_encode(&[251, 255, 191]), "-_-_");
    }

    #[test]
    fn base64url_roundtrip() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len as u8)
                .map(|i| i.wrapping_mul(37).wrapping_add(len as u8))
                .collect();
            let enc = base64url_encode(&data);
            assert!(!enc.contains('='), "unpadded output, got {enc}");
            assert!(!enc.contains('+') && !enc.contains('/'));
            let dec = base64url_decode(&enc).unwrap();
            assert_eq!(dec, data, "roundtrip failed at len {len}");
        }
    }

    #[test]
    fn base64url_decode_rejects_garbage() {
        assert!(base64url_decode("Zm9*").is_err());
        assert!(base64url_decode("A").is_err()); // 1-char segment can't decode
    }

    // ── JWT ──────────────────────────────────────────────────────────────────

    fn test_claims() -> JwtClaims {
        JwtClaims::new("user-42")
            .with_issuer("bridge-daemon")
            .with_ttl(3600)
            .with_claim("role", "admin")
    }

    #[test]
    fn jwt_roundtrip_sign_verify() {
        let secret = b"super-secret-key";
        let token = jwt_sign(&test_claims(), secret);
        assert_eq!(token.split('.').count(), 3);

        let claims = jwt_verify(&token, secret, now_secs()).unwrap();
        assert_eq!(claims.sub.as_deref(), Some("user-42"));
        assert_eq!(claims.iss.as_deref(), Some("bridge-daemon"));
        assert_eq!(claims.custom.get("role").map(String::as_str), Some("admin"));
        assert!(claims.exp.is_some());
    }

    #[test]
    fn jwt_wrong_secret_fails() {
        let token = jwt_sign(&test_claims(), b"secret-a");
        assert!(jwt_verify(&token, b"secret-b", now_secs()).is_err());
    }

    #[test]
    fn jwt_tampered_payload_fails() {
        let token = jwt_sign(&test_claims(), b"secret");
        let mut parts: Vec<String> = token.split('.').map(String::from).collect();
        // Flip payload content while keeping valid base64url.
        parts[1] = base64url_encode(br#"{"sub":"attacker"}"#);
        let forged = parts.join(".");
        assert!(jwt_verify(&forged, b"secret", now_secs()).is_err());
    }

    #[test]
    fn jwt_expired_token_rejected() {
        let mut claims = JwtClaims::new("u1");
        claims.exp = Some(1000); // long past
        claims.iat = Some(900);
        let token = jwt_sign(&claims, b"k");
        let err = jwt_verify(&token, b"k", 2000).unwrap_err();
        assert!(err.contains("expired"));
        // Same token verified "at issuance time" is fine.
        assert!(jwt_verify(&token, b"k", 999).is_ok());
    }

    #[test]
    fn jwt_alg_none_rejected() {
        // Hand-crafted alg:none token — must never verify.
        let header = base64url_encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = base64url_encode(br#"{"sub":"u1"}"#);
        let token = format!("{header}.{payload}.x");
        let err = jwt_verify(&token, b"k", now_secs()).unwrap_err();
        assert!(err.contains("algorithm"));
    }

    #[test]
    fn jwt_malformed_tokens_rejected() {
        assert!(jwt_verify("", b"k", 0).is_err());
        assert!(jwt_verify("a.b", b"k", 0).is_err());
        assert!(jwt_verify("a.b.c.d", b"k", 0).is_err());
        assert!(jwt_verify("..x", b"k", 0).is_err());
    }

    #[test]
    fn jwt_claims_json_roundtrip() {
        let original = JwtClaims::new("u7")
            .with_issuer("iss-x")
            .with_claim("org", "acme")
            .with_claim("email", "u7@acme.io");
        let original = JwtClaims {
            exp: Some(1234),
            iat: Some(1200),
            ..original
        };
        let json = original.to_json();
        let parsed = JwtClaims::from_json(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn jwt_scope_claim_splits_to_roles() {
        let json = r#"{"sub":"u1","scope":"read write admin"}"#;
        let claims = JwtClaims::from_json(json).unwrap();
        assert_eq!(claims.scopes, vec!["read", "write", "admin"]);
        let data = claims.to_auth_data();
        assert!(data.has_role("read"));
        assert!(data.has_role("admin"));
    }

    #[test]
    fn bearer_header_extraction() {
        assert_eq!(bearer_from_header(Some("Bearer abc")), Some("abc"));
        assert_eq!(bearer_from_header(Some("bearer abc")), None); // case-sensitive per RFC
        assert_eq!(bearer_from_header(Some("Basic abc")), None);
        assert_eq!(bearer_from_header(None), None);
    }

    #[test]
    fn registry_issue_and_validate_jwt_session() {
        let reg = AuthRegistry::new();
        let token = reg.issue_jwt(JwtClaims::new("session-user").with_ttl(600), b"jwt-secret");
        // Round-trips through validate_bearer since issued sessions are stored.
        let data = reg.validate_bearer(&token).unwrap();
        assert_eq!(data.user_id.as_deref(), Some("session-user"));

        // And verifies cryptographically too.
        let claims = jwt_verify(&token, b"jwt-secret", now_secs()).unwrap();
        assert_eq!(claims.sub.as_deref(), Some("session-user"));
    }

    #[test]
    fn authenticate_tampered_jwt_never_falls_back_to_registry() {
        let secret = b"real-secret";
        let reg = AuthRegistry::new();
        let token = reg.issue_jwt(JwtClaims::new("u1").with_ttl(600), secret);

        // Tamper: swap the payload for different claims, keep header+signature.
        let mut seg = token.split('.');
        let header = seg.next().unwrap().to_string();
        let _orig_payload = seg.next().unwrap();
        let signature = seg.next().unwrap().to_string();
        let forged_payload = base64url_encode(br#"{"sub":"attacker","exp":9999999999}"#);
        let tampered = format!("{header}.{forged_payload}.{signature}");

        // Even though the ORIGINAL token sits in the registry, the tampered
        // variant must fail authentication outright (no registry fallback).
        assert!(reg.authenticate(&tampered, secret).is_err());
    }

    #[test]
    fn authenticate_revoked_session_rejected_on_registry_path() {
        let secret = b"s";
        let reg = AuthRegistry::new();
        // Opaque (non-JWT) token: revocation takes effect immediately.
        reg.set_bearer("opaque-tok", AuthData::new("u3").with_expiry(600));
        assert!(reg.authenticate("opaque-tok", secret).is_ok());
        reg.revoke_bearer("opaque-tok");
        assert!(reg.authenticate("opaque-tok", secret).is_err());
    }
}
