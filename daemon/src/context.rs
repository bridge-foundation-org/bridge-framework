//! Request Context Management - Propagate context across service boundaries
//!
//! RequestContext maintains request-scoped information like correlation IDs,
//! user identity, and trace metadata that should be propagated across
//! service boundaries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Generate a UUID (placeholder - in real code would use uuid crate)
fn generate_uuid() -> String {
    // Simplified UUID generation for testing
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        rand_u32(),
        rand_u16(),
        rand_u16(),
        rand_u16(),
        rand_u64() & 0xFFFFFFFFFFFF
    )
}

fn rand_u32() -> u32 {
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u32)
        .wrapping_mul(1103515245)
        .wrapping_add(12345)
}

fn rand_u16() -> u16 {
    (rand_u32() >> 16) as u16
}

fn rand_u64() -> u64 {
    let a = rand_u32() as u64;
    let b = rand_u32() as u64;
    (a << 32) | b
}

/// Represents the deadline for a request
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct Deadline {
    /// Unix timestamp in seconds
    seconds: u64,
}

impl Deadline {
    /// Create a new deadline from duration
    pub fn from_now(duration: Duration) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Deadline {
            seconds: now + duration.as_secs(),
        }
    }

    /// Check if deadline has exceeded
    pub fn exceeded(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now >= self.seconds
    }

    /// Get remaining time
    pub fn remaining(&self) -> Duration {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now >= self.seconds {
            Duration::from_secs(0)
        } else {
            Duration::from_secs(self.seconds - now)
        }
    }
}

/// Request context - request-scoped information
#[derive(Clone, Debug)]
pub struct RequestContext {
    /// Unique request ID
    request_id: String,
    /// Correlation ID (same across service calls)
    correlation_id: String,
    /// Parent span ID for distributed tracing
    parent_span_id: Option<String>,
    /// Current span ID
    span_id: String,
    /// User ID (from auth)
    user_id: Option<String>,
    /// Service name that handled the request
    service: String,
    /// Request deadline
    deadline: Option<Deadline>,
    /// Custom metadata
    metadata: HashMap<String, String>,
}

impl RequestContext {
    /// Create a new request context with generated IDs
    pub fn new(service: impl Into<String>) -> Self {
        RequestContext {
            request_id: generate_uuid(),
            correlation_id: generate_uuid(),
            parent_span_id: None,
            span_id: generate_uuid(),
            user_id: None,
            service: service.into(),
            deadline: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a new context from headers (for propagation)
    pub fn from_headers(
        service: impl Into<String>,
        headers: &HashMap<String, String>,
    ) -> Self {
        let correlation_id = headers
            .get("x-correlation-id")
            .or_else(|| headers.get("X-Correlation-ID"))
            .cloned()
            .unwrap_or_else(generate_uuid);

        let parent_span_id = headers
            .get("x-parent-span-id")
            .or_else(|| headers.get("X-Parent-Span-ID"))
            .cloned();

        RequestContext {
            request_id: generate_uuid(),
            correlation_id,
            parent_span_id,
            span_id: generate_uuid(),
            user_id: None,
            service: service.into(),
            deadline: None,
            metadata: HashMap::new(),
        }
    }

    /// Get request ID
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Get correlation ID
    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    /// Get parent span ID
    pub fn parent_span_id(&self) -> Option<&str> {
        self.parent_span_id.as_deref()
    }

    /// Get current span ID
    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    /// Set user ID
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Get user ID
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Set deadline
    pub fn with_deadline(mut self, deadline: Deadline) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Get deadline
    pub fn deadline(&self) -> Option<Deadline> {
        self.deadline
    }

    /// Check if context is expired
    pub fn is_expired(&self) -> bool {
        if let Some(deadline) = self.deadline {
            deadline.exceeded()
        } else {
            false
        }
    }

    /// Add metadata
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get metadata
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|v| v.as_str())
    }

    /// Convert to propagation headers for cross-service calls
    pub fn to_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("X-Correlation-ID".to_string(), self.correlation_id.clone());
        headers.insert("X-Request-ID".to_string(), self.request_id.clone());
        headers.insert("X-Span-ID".to_string(), self.span_id.clone());
        headers.insert(
            "X-Parent-Span-ID".to_string(),
            self.parent_span_id.clone().unwrap_or_default(),
        );
        headers.insert("X-Service".to_string(), self.service.clone());

        if let Some(user_id) = &self.user_id {
            headers.insert("X-User-ID".to_string(), user_id.clone());
        }

        // Add custom metadata with X- prefix
        for (key, value) in &self.metadata {
            headers.insert(format!("X-{}", key), value.clone());
        }

        headers
    }

    /// Child context (for nested spans in same trace)
    pub fn child(&self) -> RequestContext {
        RequestContext {
            request_id: self.request_id.clone(),
            correlation_id: self.correlation_id.clone(),
            parent_span_id: Some(self.span_id.clone()),
            span_id: generate_uuid(),
            user_id: self.user_id.clone(),
            service: self.service.clone(),
            deadline: self.deadline,
            metadata: self.metadata.clone(),
        }
    }

    /// Cross-service context (propagates most info, generates new request/span)
    pub fn cross_service(&self, service: impl Into<String>) -> RequestContext {
        RequestContext {
            request_id: generate_uuid(),
            correlation_id: self.correlation_id.clone(),
            parent_span_id: Some(self.span_id.clone()),
            span_id: generate_uuid(),
            user_id: self.user_id.clone(),
            service: service.into(),
            deadline: self.deadline,
            metadata: self.metadata.clone(),
        }
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        RequestContext::new("unknown")
    }
}

/// Thread-local request context storage
thread_local! {
    static CURRENT_CONTEXT: Arc<Mutex<Option<RequestContext>>> = Arc::new(Mutex::new(None));
}

/// Set the current request context for this thread
pub fn set_context(ctx: RequestContext) {
    CURRENT_CONTEXT.with(|current| {
        if let Ok(mut ctx_guard) = current.lock() {
            *ctx_guard = Some(ctx);
        }
    });
}

/// Get the current request context
pub fn get_context() -> Option<RequestContext> {
    CURRENT_CONTEXT.with(|current| {
        if let Ok(ctx_guard) = current.lock() {
            ctx_guard.clone()
        } else {
            None
        }
    })
}

/// Clear the current request context
pub fn clear_context() {
    CURRENT_CONTEXT.with(|current| {
        if let Ok(mut ctx_guard) = current.lock() {
            *ctx_guard = None;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deadline_from_now() {
        let deadline = Deadline::from_now(Duration::from_secs(10));
        assert!(!deadline.exceeded());
    }

    #[test]
    fn test_deadline_remaining() {
        let deadline = Deadline::from_now(Duration::from_secs(100));
        let remaining = deadline.remaining();
        assert!(remaining.as_secs() > 90);
    }

    #[test]
    fn test_request_context_new() {
        let ctx = RequestContext::new("users");
        assert!(!ctx.request_id.is_empty());
        assert!(!ctx.correlation_id.is_empty());
        assert!(!ctx.span_id.is_empty());
        assert_eq!(ctx.service, "users");
        assert_eq!(ctx.user_id, None);
    }

    #[test]
    fn test_request_context_from_headers() {
        let mut headers = HashMap::new();
        headers.insert(
            "X-Correlation-ID".to_string(),
            "corr-123".to_string(),
        );
        headers.insert("X-Parent-Span-ID".to_string(), "parent-456".to_string());

        let ctx = RequestContext::from_headers("posts", &headers);
        assert_eq!(ctx.correlation_id(), "corr-123");
        assert_eq!(ctx.parent_span_id(), Some("parent-456"));
        assert_eq!(ctx.service, "posts");
    }

    #[test]
    fn test_request_context_with_user_id() {
        let ctx = RequestContext::new("users").with_user_id("user-789");
        assert_eq!(ctx.user_id(), Some("user-789"));
    }

    #[test]
    fn test_request_context_with_deadline() {
        let deadline = Deadline::from_now(Duration::from_secs(30));
        let ctx = RequestContext::new("users").with_deadline(deadline);
        assert!(ctx.deadline.is_some());
        assert!(!ctx.is_expired());
    }

    #[test]
    fn test_request_context_metadata() {
        let ctx = RequestContext::new("users")
            .metadata("region", "us-east-1")
            .metadata("env", "prod");

        assert_eq!(ctx.get_metadata("region"), Some("us-east-1"));
        assert_eq!(ctx.get_metadata("env"), Some("prod"));
        assert_eq!(ctx.get_metadata("nonexistent"), None);
    }

    #[test]
    fn test_request_context_to_headers() {
        let ctx = RequestContext::new("users")
            .with_user_id("user-123")
            .metadata("region", "us-west-2");

        let headers = ctx.to_headers();
        assert!(headers.contains_key("X-Correlation-ID"));
        assert!(headers.contains_key("X-Request-ID"));
        assert!(headers.contains_key("X-Span-ID"));
        assert_eq!(headers.get("X-Service"), Some(&"users".to_string()));
        assert_eq!(headers.get("X-User-ID"), Some(&"user-123".to_string()));
    }

    #[test]
    fn test_request_context_child() {
        let parent = RequestContext::new("users");
        let parent_span = parent.span_id.clone();
        let child = parent.child();

        assert_eq!(child.correlation_id, parent.correlation_id);
        assert_eq!(child.request_id, parent.request_id);
        assert_eq!(child.parent_span_id(), Some(parent_span.as_str()));
        assert_ne!(child.span_id, parent.span_id);
    }

    #[test]
    fn test_request_context_cross_service() {
        let original = RequestContext::new("users");
        let original_span = original.span_id.clone();
        let cross = original.cross_service("posts");

        assert_eq!(cross.correlation_id, original.correlation_id);
        assert_eq!(cross.parent_span_id(), Some(original_span.as_str()));
        assert_ne!(cross.request_id, original.request_id);
        assert_eq!(cross.service, "posts");
    }

    #[test]
    fn test_request_context_default() {
        let ctx = RequestContext::default();
        assert_eq!(ctx.service, "unknown");
        assert!(!ctx.request_id.is_empty());
    }

    #[test]
    fn test_thread_local_set_and_get() {
        clear_context();
        assert!(get_context().is_none());

        let ctx = RequestContext::new("users");
        set_context(ctx.clone());

        let retrieved = get_context();
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.unwrap().correlation_id(),
            ctx.correlation_id()
        );
    }

    #[test]
    fn test_thread_local_clear() {
        let ctx = RequestContext::new("users");
        set_context(ctx);
        assert!(get_context().is_some());

        clear_context();
        assert!(get_context().is_none());
    }

    #[test]
    fn test_context_propagation_chain() {
        // Simulate a request chain: users -> posts -> comments
        let original = RequestContext::new("users");
        let corr_id = original.correlation_id().to_string();

        let to_posts = original.cross_service("posts");
        assert_eq!(to_posts.correlation_id(), &corr_id);

        let to_comments = to_posts.cross_service("comments");
        assert_eq!(to_comments.correlation_id(), &corr_id);

        // But request IDs should be different
        assert_ne!(original.request_id(), to_posts.request_id());
        assert_ne!(to_posts.request_id(), to_comments.request_id());
    }

    #[test]
    fn test_deadline_exceeded() {
        let deadline = Deadline {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                - 1,
        };
        assert!(deadline.exceeded());
    }

    #[test]
    fn test_context_preserves_metadata() {
        let original = RequestContext::new("users").metadata("key", "value");
        let child = original.child();

        assert_eq!(child.get_metadata("key"), Some("value"));
    }

    #[test]
    fn test_context_user_id_preserved_cross_service() {
        let original = RequestContext::new("users").with_user_id("user-456");
        let cross = original.cross_service("posts");

        assert_eq!(cross.user_id(), Some("user-456"));
    }
}
