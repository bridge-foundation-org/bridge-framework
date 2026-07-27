//! Bridge structured errors — cause chains, error codes, internal/external separation.
//!
//! Inspired by Encore commits 1491 (add details to errors),
//! 1561 (add-details-to-errors), 1584 (hide-error-internal-message-in-response).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use std::fmt;

// ── Error codes ───────────────────────────────────────────────────────────────

/// Standardised error codes — mirrors Encore's `errs` package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// Unspecified or unknown error.
    Unknown,
    /// Caller provided invalid input.
    InvalidArgument,
    /// The operation was not found.
    NotFound,
    /// Caller does not have permission.
    PermissionDenied,
    /// Caller is not authenticated.
    Unauthenticated,
    /// Rate-limit exceeded.
    ResourceExhausted,
    /// Pre-condition not met (e.g. concurrent modification).
    FailedPrecondition,
    /// Operation cancelled by caller.
    Cancelled,
    /// Business logic conflict (409).
    AlreadyExists,
    /// Internal server error.
    Internal,
    /// Feature not implemented.
    Unimplemented,
    /// Upstream service unavailable.
    Unavailable,
    /// Request deadline exceeded.
    DeadlineExceeded,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Unknown             => "unknown",
            Code::InvalidArgument     => "invalid_argument",
            Code::NotFound            => "not_found",
            Code::PermissionDenied    => "permission_denied",
            Code::Unauthenticated     => "unauthenticated",
            Code::ResourceExhausted   => "resource_exhausted",
            Code::FailedPrecondition  => "failed_precondition",
            Code::Cancelled           => "cancelled",
            Code::AlreadyExists       => "already_exists",
            Code::Internal            => "internal",
            Code::Unimplemented       => "unimplemented",
            Code::Unavailable         => "unavailable",
            Code::DeadlineExceeded    => "deadline_exceeded",
        }
    }

    /// HTTP status code equivalent.
    pub fn http_status(self) -> u16 {
        match self {
            Code::Unknown             => 500,
            Code::InvalidArgument     => 400,
            Code::NotFound            => 404,
            Code::PermissionDenied    => 403,
            Code::Unauthenticated     => 401,
            Code::ResourceExhausted   => 429,
            Code::FailedPrecondition  => 412,
            Code::Cancelled           => 499,
            Code::AlreadyExists       => 409,
            Code::Internal            => 500,
            Code::Unimplemented       => 501,
            Code::Unavailable         => 503,
            Code::DeadlineExceeded    => 504,
        }
    }

    pub fn from_http_status(status: u16) -> Self {
        match status {
            400 => Code::InvalidArgument,
            401 => Code::Unauthenticated,
            403 => Code::PermissionDenied,
            404 => Code::NotFound,
            409 => Code::AlreadyExists,
            412 => Code::FailedPrecondition,
            429 => Code::ResourceExhausted,
            499 => Code::Cancelled,
            501 => Code::Unimplemented,
            503 => Code::Unavailable,
            504 => Code::DeadlineExceeded,
            _   => Code::Internal,
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ── Error detail ──────────────────────────────────────────────────────────────

/// Structured key-value detail attached to an error.
#[derive(Debug, Clone)]
pub struct Detail {
    pub key:   String,
    pub value: String,
}

impl Detail {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Detail { key: key.into(), value: value.into() }
    }
}

// ── BridgeError ───────────────────────────────────────────────────────────────

/// The core error type for Bridge.
///
/// Holds:
/// - a **public** message safe to return to callers
/// - an optional **internal** message only logged server-side
/// - structured details for programmatic handling
/// - an optional cause chain
#[derive(Debug, Clone)]
pub struct BridgeError {
    pub code:     Code,
    /// Message safe to send to external callers.
    pub message:  String,
    /// Internal detail — never sent to callers in production.
    pub internal: Option<String>,
    /// Structured key-value details.
    pub details:  Vec<Detail>,
    /// Wrapped cause.
    pub cause:    Option<Box<BridgeError>>,
}

impl BridgeError {
    // ── Constructors ──────────────────────────────────────────────────────

    pub fn new(code: Code, message: impl Into<String>) -> Self {
        BridgeError {
            code,
            message:  message.into(),
            internal: None,
            details:  Vec::new(),
            cause:    None,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(Code::Internal, "internal server error")
            .with_internal(msg)
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(Code::NotFound, msg)
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::new(Code::InvalidArgument, msg)
    }

    pub fn unauthenticated(msg: impl Into<String>) -> Self {
        Self::new(Code::Unauthenticated, msg)
    }

    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self::new(Code::PermissionDenied, msg)
    }

    // ── Builder methods ───────────────────────────────────────────────────

    pub fn with_internal(mut self, msg: impl Into<String>) -> Self {
        self.internal = Some(msg.into());
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push(Detail::new(key, value));
        self
    }

    pub fn with_cause(mut self, cause: BridgeError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    pub fn wrap_std(code: Code, public_msg: impl Into<String>, err: impl std::error::Error) -> Self {
        Self::new(code, public_msg)
            .with_internal(err.to_string())
    }

    // ── Output ────────────────────────────────────────────────────────────

    /// JSON for external callers — omits `internal` field.
    pub fn to_public_json(&self) -> String {
        let details: String = self.details.iter()
            .map(|d| format!(",{{\"key\":\"{}\",\"value\":\"{}\"}}", d.key, d.value))
            .collect();
        format!(
            r#"{{"code":"{code}","message":"{msg}","details":[{details}]}}"#,
            code    = self.code.as_str(),
            msg     = self.message.replace('"', "\\\""),
            details = details.trim_start_matches(','),
        )
    }

    /// Full JSON including internal details — for logging only.
    pub fn to_internal_json(&self) -> String {
        let internal = self.internal.as_deref()
            .map(|s| format!(",\"internal\":\"{}\"", s.replace('"', "\\\"")))
            .unwrap_or_default();
        let cause = self.cause.as_ref()
            .map(|c| format!(",\"cause\":{}", c.to_internal_json()))
            .unwrap_or_default();
        let base = self.to_public_json();
        // Insert internal/cause before final `}`
        let trimmed = base.trim_end_matches('}');
        format!("{trimmed}{internal}{cause}}}")
    }

    /// Wire format: `ERR <code>: <message>`
    pub fn to_wire(&self) -> String {
        format!("ERR {}: {}", self.code.as_str(), self.message)
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)?;
        if let Some(cause) = &self.cause {
            write!(f, " (caused by: {cause})")?;
        }
        Ok(())
    }
}

impl std::error::Error for BridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_deref().map(|e| e as &dyn std::error::Error)
    }
}

/// Short-hand `Result<T, BridgeError>`.
pub type Result<T> = std::result::Result<T, BridgeError>;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_http_status() {
        assert_eq!(Code::NotFound.http_status(), 404);
        assert_eq!(Code::Unauthenticated.http_status(), 401);
        assert_eq!(Code::Internal.http_status(), 500);
        assert_eq!(Code::InvalidArgument.http_status(), 400);
    }

    #[test]
    fn code_round_trip_from_http() {
        assert_eq!(Code::from_http_status(404), Code::NotFound);
        assert_eq!(Code::from_http_status(401), Code::Unauthenticated);
        assert_eq!(Code::from_http_status(999), Code::Internal);
    }

    #[test]
    fn public_json_excludes_internal() {
        let err = BridgeError::internal("secret DB connection string")
            .with_detail("field", "email");
        let json = err.to_public_json();
        assert!(!json.contains("secret DB connection string"),
            "internal message leaked: {json}");
        assert!(json.contains("internal server error"));
        assert!(json.contains("field"));
    }

    #[test]
    fn internal_json_includes_all() {
        let err = BridgeError::new(Code::NotFound, "user not found")
            .with_internal("query returned 0 rows for user_id=42")
            .with_detail("user_id", "42");
        let json = err.to_internal_json();
        assert!(json.contains("query returned 0 rows"));
        assert!(json.contains("user_id"));
    }

    #[test]
    fn cause_chain_display() {
        let root  = BridgeError::new(Code::Unavailable, "database offline");
        let outer = BridgeError::new(Code::Internal, "failed to load user")
            .with_cause(root);
        let text = outer.to_string();
        assert!(text.contains("failed to load user"));
        assert!(text.contains("database offline"));
    }

    #[test]
    fn wire_format() {
        let err = BridgeError::not_found("endpoint not found");
        assert_eq!(err.to_wire(), "ERR not_found: endpoint not found");
    }

    #[test]
    fn builder_chaining() {
        let err = BridgeError::new(Code::InvalidArgument, "bad input")
            .with_internal("field 'email' failed regex")
            .with_detail("field", "email")
            .with_detail("rule", "email_format");
        assert_eq!(err.details.len(), 2);
        assert!(err.internal.is_some());
    }
}
