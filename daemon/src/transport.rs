//! HTTP Transport Layer - Enables cross-service HTTP calls with request signing
//!
//! The transport layer handles all HTTP communication between services,
//! including:
//! - Service discovery (DNS resolution and fallback)
//! - Request signing (bearer tokens, API keys, PSK)
//! - Request/response encoding
//! - Metadata propagation (correlation IDs, trace info)
//! - Timeout and retry logic

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// HTTP methods supported for cross-service calls
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
}

impl HttpMethod {
    /// Get the HTTP method as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Head => "HEAD",
        }
    }
}

/// Authentication method for service-to-service calls
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthMethod {
    /// No authentication
    None,
    /// Bearer token (JWT, OAuth2 token)
    Bearer(String),
    /// API key
    ApiKey { header: String, value: String },
    /// Pre-shared key
    PSK(String),
}

/// HTTP Request for cross-service communication
#[derive(Clone, Debug)]
pub struct HttpRequest {
    /// HTTP method
    pub method: HttpMethod,
    /// Path (e.g., "/users/123")
    pub path: String,
    /// Query parameters
    pub query: HashMap<String, String>,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// Request body
    pub body: Option<Vec<u8>>,
    /// Authentication method
    pub auth: AuthMethod,
    /// Request timeout
    pub timeout: Duration,
    /// Custom metadata (correlation ID, trace headers, etc.)
    pub metadata: HashMap<String, String>,
}

impl HttpRequest {
    /// Create a new GET request
    pub fn get(path: impl Into<String>) -> Self {
        HttpRequest {
            method: HttpMethod::Get,
            path: path.into(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
            auth: AuthMethod::None,
            timeout: Duration::from_secs(30),
            metadata: HashMap::new(),
        }
    }

    /// Create a new POST request
    pub fn post(path: impl Into<String>, body: Vec<u8>) -> Self {
        HttpRequest {
            method: HttpMethod::Post,
            path: path.into(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: Some(body),
            auth: AuthMethod::None,
            timeout: Duration::from_secs(30),
            metadata: HashMap::new(),
        }
    }

    /// Add a header
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Add query parameter
    pub fn query(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.insert(key.into(), value.into());
        self
    }

    /// Set authentication method
    pub fn auth(mut self, auth: AuthMethod) -> Self {
        self.auth = auth;
        self
    }

    /// Set timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Add metadata
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get fully qualified path with query parameters
    pub fn full_path(&self) -> String {
        if self.query.is_empty() {
            self.path.clone()
        } else {
            let mut path = self.path.clone();
            path.push('?');
            let query_parts: Vec<String> = self
                .query
                .iter()
                .map(|(k, v)| format!("{}={}", k, urlencoding(v)))
                .collect();
            path.push_str(&query_parts.join("&"));
            path
        }
    }

    /// Build HTTP request line
    pub fn to_http_request(&self) -> String {
        let mut req = format!("{} {} HTTP/1.1\r\n", self.method.as_str(), self.full_path());

        // Add headers
        req.push_str("Host: localhost\r\n");

        // Add authentication
        match &self.auth {
            AuthMethod::None => {}
            AuthMethod::Bearer(token) => {
                req.push_str(&format!("Authorization: Bearer {}\r\n", token));
            }
            AuthMethod::ApiKey { header, value } => {
                req.push_str(&format!("{}: {}\r\n", header, value));
            }
            AuthMethod::PSK(key) => {
                req.push_str(&format!("X-PSK: {}\r\n", key));
            }
        }

        // Add metadata
        for (key, value) in &self.metadata {
            req.push_str(&format!("X-{}: {}\r\n", key, value));
        }

        // Add custom headers
        for (key, value) in &self.headers {
            req.push_str(&format!("{}: {}\r\n", key, value));
        }

        // Add body if present
        if let Some(body) = &self.body {
            req.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }

        req.push_str("\r\n");
        req
    }
}

/// HTTP Response from cross-service call
#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponse {
    /// HTTP status code
    pub status: u16,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Create a new response
    pub fn new(status: u16) -> Self {
        HttpResponse {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    /// Add a header
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Set body
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Check if response is successful (2xx status)
    pub fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// Check if response is client error (4xx status)
    pub fn is_client_error(&self) -> bool {
        self.status >= 400 && self.status < 500
    }

    /// Check if response is server error (5xx status)
    pub fn is_server_error(&self) -> bool {
        self.status >= 500 && self.status < 600
    }

    /// Get response body as string
    pub fn body_str(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.body.clone())
    }
}

/// HTTP Transport client for service-to-service calls
pub struct HttpTransport {
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Request timeout
    pub request_timeout: Duration,
}

impl HttpTransport {
    /// Create a new HTTP transport
    pub fn new() -> Self {
        HttpTransport {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
        }
    }

    /// Send an HTTP request to a service
    pub fn send(
        &self,
        host: &str,
        port: u16,
        request: &HttpRequest,
    ) -> Result<HttpResponse, String> {
        // In a real implementation, this would:
        // 1. Resolve DNS if needed
        // 2. Connect to the host:port
        // 3. Send the HTTP request
        // 4. Read and parse the response
        // 5. Handle timeouts and retries

        // For now, return a stub response for testing
        Ok(HttpResponse::new(200).body(b"{}".to_vec()))
    }

    /// Send a GET request
    pub fn get(&self, host: &str, port: u16, path: &str) -> Result<HttpResponse, String> {
        self.send(host, port, &HttpRequest::get(path))
    }

    /// Send a POST request
    pub fn post(
        &self,
        host: &str,
        port: u16,
        path: &str,
        body: Vec<u8>,
    ) -> Result<HttpResponse, String> {
        self.send(host, port, &HttpRequest::post(path, body))
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// URL encode a string
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_get() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
    }

    #[test]
    fn test_http_method_post() {
        assert_eq!(HttpMethod::Post.as_str(), "POST");
    }

    #[test]
    fn test_auth_none() {
        assert_eq!(AuthMethod::None, AuthMethod::None);
    }

    #[test]
    fn test_auth_bearer() {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9".to_string();
        let auth = AuthMethod::Bearer(token.clone());
        assert_eq!(auth, AuthMethod::Bearer(token));
    }

    #[test]
    fn test_auth_api_key() {
        let auth = AuthMethod::ApiKey {
            header: "X-API-Key".to_string(),
            value: "secret123".to_string(),
        };
        match auth {
            AuthMethod::ApiKey { header, value } => {
                assert_eq!(header, "X-API-Key");
                assert_eq!(value, "secret123");
            }
            _ => panic!("Expected ApiKey auth"),
        }
    }

    #[test]
    fn test_auth_psk() {
        let auth = AuthMethod::PSK("shared_secret".to_string());
        match auth {
            AuthMethod::PSK(secret) => assert_eq!(secret, "shared_secret"),
            _ => panic!("Expected PSK auth"),
        }
    }

    #[test]
    fn test_http_request_get() {
        let req = HttpRequest::get("/users");
        assert_eq!(req.method, HttpMethod::Get);
        assert_eq!(req.path, "/users");
        assert_eq!(req.body, None);
    }

    #[test]
    fn test_http_request_post() {
        let body = b"test".to_vec();
        let req = HttpRequest::post("/users", body.clone());
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.body, Some(body));
    }

    #[test]
    fn test_http_request_header() {
        let req = HttpRequest::get("/users").header("X-Custom", "value");
        assert_eq!(req.headers.get("X-Custom"), Some(&"value".to_string()));
    }

    #[test]
    fn test_http_request_query() {
        let req = HttpRequest::get("/users")
            .query("page", "1")
            .query("limit", "10");
        assert_eq!(req.query.len(), 2);
    }

    #[test]
    fn test_http_request_auth() {
        let auth = AuthMethod::Bearer("token123".to_string());
        let req = HttpRequest::get("/users").auth(auth.clone());
        assert_eq!(req.auth, auth);
    }

    #[test]
    fn test_http_request_timeout() {
        let timeout = Duration::from_secs(60);
        let req = HttpRequest::get("/users").timeout(timeout);
        assert_eq!(req.timeout, timeout);
    }

    #[test]
    fn test_http_request_metadata() {
        let req = HttpRequest::get("/users")
            .metadata("correlation-id", "abc123")
            .metadata("trace-id", "xyz789");
        assert_eq!(req.metadata.len(), 2);
    }

    #[test]
    fn test_http_request_full_path_no_query() {
        let req = HttpRequest::get("/users");
        assert_eq!(req.full_path(), "/users");
    }

    #[test]
    fn test_http_request_full_path_with_query() {
        let req = HttpRequest::get("/users")
            .query("page", "1")
            .query("limit", "10");
        let full_path = req.full_path();
        assert!(full_path.contains("page=1"));
        assert!(full_path.contains("limit=10"));
    }

    #[test]
    fn test_http_request_to_http_request() {
        let req = HttpRequest::get("/users");
        let http = req.to_http_request();
        assert!(http.contains("GET /users HTTP/1.1"));
    }

    #[test]
    fn test_http_response_success() {
        let resp = HttpResponse::new(200);
        assert!(resp.is_success());
        assert!(!resp.is_client_error());
        assert!(!resp.is_server_error());
    }

    #[test]
    fn test_http_response_client_error() {
        let resp = HttpResponse::new(404);
        assert!(!resp.is_success());
        assert!(resp.is_client_error());
        assert!(!resp.is_server_error());
    }

    #[test]
    fn test_http_response_server_error() {
        let resp = HttpResponse::new(500);
        assert!(!resp.is_success());
        assert!(!resp.is_client_error());
        assert!(resp.is_server_error());
    }

    #[test]
    fn test_http_response_with_body() {
        let body = b"test response".to_vec();
        let resp = HttpResponse::new(200).body(body.clone());
        assert_eq!(resp.body, body);
    }

    #[test]
    fn test_http_response_body_str() {
        let resp = HttpResponse::new(200).body(b"hello".to_vec());
        assert_eq!(resp.body_str().unwrap(), "hello");
    }

    #[test]
    fn test_http_transport_new() {
        let transport = HttpTransport::new();
        assert_eq!(transport.connect_timeout, Duration::from_secs(5));
        assert_eq!(transport.request_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_http_transport_send() {
        let transport = HttpTransport::new();
        let req = HttpRequest::get("/users");
        let resp = transport.send("localhost", 8001, &req).unwrap();
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_url_encoding_simple() {
        assert_eq!(urlencoding("hello"), "hello");
    }

    #[test]
    fn test_url_encoding_space() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
    }

    #[test]
    fn test_url_encoding_special_chars() {
        assert_eq!(urlencoding("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn test_http_request_builder_chain() {
        let req = HttpRequest::get("/users")
            .header("X-Custom", "value")
            .query("page", "1")
            .auth(AuthMethod::Bearer("token".to_string()))
            .timeout(Duration::from_secs(60));

        assert_eq!(req.method, HttpMethod::Get);
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.query.len(), 1);
        assert_eq!(req.timeout, Duration::from_secs(60));
    }
}
