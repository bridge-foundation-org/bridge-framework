//! Bridge DSL compiler — parses `.bridge` source files into a typed AST.
//!
//! # DSL syntax
//!
//! ```bridge
//! # comment
//! service <name>
//!   [auth <scheme>]          # bearer | api_key | none (default: none)
//!   [middleware <name> ...]  # space-separated list
//!
//! endpoint <name> <METHOD> <path>
//!   [auth <scheme>]          # per-endpoint override
//!   [tags <tag> ...]
//! ```
//!
//! ## Path parameters
//! Path segments starting with `:` are typed path parameters.
//!
//! ```bridge
//! service users
//! endpoint get   GET  /users/:id
//! endpoint list  GET  /users
//! endpoint create POST /users
//! endpoint update PUT  /users/:id
//! endpoint delete DELETE /users/:id
//! ```
//!
//! ## Multiple services per file
//! A single `.bridge` file may declare multiple services.
//! A new `service` line starts a new service block.

// ── AST types ────────────────────────────────────────────────────────────────

/// HTTP methods supported by Bridge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_uppercase().as_str() {
            "GET" => Ok(Method::Get),
            "POST" => Ok(Method::Post),
            "PUT" => Ok(Method::Put),
            "PATCH" => Ok(Method::Patch),
            "DELETE" => Ok(Method::Delete),
            "HEAD" => Ok(Method::Head),
            "OPTIONS" => Ok(Method::Options),
            other => Err(format!("unknown HTTP method: {other}")),
        }
    }
}

/// Auth scheme required by a service or endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Auth {
    #[default]
    None,
    Bearer,
    ApiKey,
}

impl Auth {
    fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Ok(Auth::None),
            "bearer" => Ok(Auth::Bearer),
            "api_key" | "apikey" => Ok(Auth::ApiKey),
            other => Err(format!(
                "unknown auth scheme: {other} (use: none|bearer|api_key)"
            )),
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Auth::None => "none",
            Auth::Bearer => "bearer",
            Auth::ApiKey => "api_key",
        }
    }
}

/// A single endpoint within a service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Identifier used for the generated function name.
    pub name: String,
    /// HTTP method.
    pub method: Method,
    /// URL path, may contain `:param` segments.
    pub path: String,
    /// Auth override (falls back to service-level auth).
    pub auth: Option<Auth>,
    /// Arbitrary tags for filtering / grouping.
    pub tags: Vec<String>,
}

impl Endpoint {
    /// Extract `:param` names from the path, in order.
    pub fn path_params(&self) -> Vec<&str> {
        self.path
            .split('/')
            .filter(|seg| seg.starts_with(':'))
            .map(|seg| &seg[1..])
            .collect()
    }

    /// Whether this endpoint has any path parameters.
    pub fn has_path_params(&self) -> bool {
        !self.path_params().is_empty()
    }
}

/// A parsed service block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// Service name (used as namespace in generated clients).
    pub name: String,
    /// Default auth scheme for all endpoints in this service.
    pub auth: Auth,
    /// Middleware names to apply.
    pub middleware: Vec<String>,
    /// Endpoints declared in this service.
    pub endpoints: Vec<Endpoint>,
}

/// A fully parsed Bridge file (may contain multiple services).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BridgeFile {
    pub services: Vec<Service>,
}

impl BridgeFile {
    /// Return a new `BridgeFile` containing only endpoints that have the given tag.
    /// Services with no matching endpoints are omitted entirely.
    pub fn filter_by_tag(&self, tag: &str) -> Self {
        let services = self
            .services
            .iter()
            .filter_map(|svc| {
                let eps: Vec<Endpoint> = svc
                    .endpoints
                    .iter()
                    .filter(|ep| ep.tags.iter().any(|t| t == tag))
                    .cloned()
                    .collect();
                if eps.is_empty() {
                    None
                } else {
                    Some(Service {
                        endpoints: eps,
                        ..svc.clone()
                    })
                }
            })
            .collect();
        BridgeFile { services }
    }

    /// Total endpoint count across all services.
    pub fn endpoint_count(&self) -> usize {
        self.services.iter().map(|s| s.endpoints.len()).sum()
    }

    /// Collect all unique tags used across all endpoints.
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: std::collections::HashSet<String> = std::collections::HashSet::new();
        for svc in &self.services {
            for ep in &svc.endpoints {
                for t in &ep.tags {
                    tags.insert(t.clone());
                }
            }
        }
        let mut v: Vec<String> = tags.into_iter().collect();
        v.sort();
        v
    }
}

// ── Rich error types ─────────────────────────────────────────────────────────

/// A structured parse error with source context and an optional fix hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Numeric error code (E0001 – E0010).
    pub code: &'static str,
    /// 1-based line number where the error occurred.
    pub line: usize,
    /// 1-based column of the offending token (0 if unknown).
    pub column: usize,
    /// Short human-readable description.
    pub message: String,
    /// The source line text (for display with a caret).
    pub snippet: String,
    /// Suggested fix shown after the error.
    pub hint: Option<String>,
}

impl ParseError {
    fn new(
        code: &'static str,
        line: usize,
        message: impl Into<String>,
        snippet: impl Into<String>,
        hint: Option<&'static str>,
    ) -> Self {
        ParseError {
            code,
            line,
            column: 0,
            message: message.into(),
            snippet: snippet.into(),
            hint: hint.map(str::to_string),
        }
    }

    fn new_hint(
        code: &'static str,
        line: usize,
        message: impl Into<String>,
        snippet: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        ParseError {
            code,
            line,
            column: 0,
            message: message.into(),
            snippet: snippet.into(),
            hint: Some(hint.into()),
        }
    }

    fn with_column(mut self, col: usize) -> Self {
        self.column = col;
        self
    }

    /// Format a Rust-compiler-style error message.
    ///
    /// ```text
    /// error[E0001]: unknown HTTP method: FETCH
    ///   --> api.bridge:3:14
    ///    |
    ///  3 | endpoint list FETCH /users
    ///    |               ^^^^^ unknown method
    ///    |
    ///    = hint: valid methods are GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS
    /// ```
    pub fn display(&self, filename: &str) -> String {
        let mut out = format!("error[{}]: {}\n", self.code, self.message);
        if self.column > 0 {
            out.push_str(&format!(
                "  --> {}:{}:{}\n",
                filename, self.line, self.column
            ));
        } else {
            out.push_str(&format!("  --> {}:{}\n", filename, self.line));
        }
        if !self.snippet.is_empty() {
            let lineno_str = self.line.to_string();
            let pad = " ".repeat(lineno_str.len());
            out.push_str(&format!("   {pad}|\n"));
            out.push_str(&format!(" {lineno_str} | {}\n", self.snippet));
            out.push_str(&format!("   {pad}|\n"));
        }
        if let Some(hint) = &self.hint {
            out.push_str(&format!("   = hint: {hint}\n"));
        }
        out
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[{}] line {}: {}",
            self.code, self.line, self.message
        )
    }
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a `.bridge` source string with rich structured errors.
///
/// Returns a `BridgeFile` on success, or a `Vec<ParseError>` (one per
/// problem) on failure.
pub fn parse_with_errors(source: &str) -> Result<BridgeFile, Vec<ParseError>> {
    let lines: Vec<&str> = source.lines().collect();
    let mut errors: Vec<ParseError> = Vec::new();
    let mut file = BridgeFile::default();
    let mut current: Option<Service> = None;

    for (idx, raw) in lines.iter().enumerate() {
        let lineno = idx + 1; // 1-based
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }

        let mut tokens = line.split_whitespace();
        let keyword = tokens.next().unwrap_or("");

        match keyword {
            "service" => {
                if let Some(svc) = current.take() {
                    if let Err(e) = check_service(&svc, lineno, raw) {
                        errors.push(e);
                    }
                    file.services.push(svc);
                }
                match tokens.next() {
                    None => errors.push(ParseError::new(
                        "E0010",
                        lineno,
                        "'service' requires a name",
                        raw.trim(),
                        Some("write: service <name>"),
                    )),
                    Some(name) => {
                        current = Some(Service {
                            name: name.to_string(),
                            auth: Auth::None,
                            middleware: Vec::new(),
                            endpoints: Vec::new(),
                        });
                    }
                }
            }

            "auth" => match tokens.next() {
                None => errors.push(ParseError::new(
                    "E0005",
                    lineno,
                    "'auth' requires a scheme",
                    raw.trim(),
                    Some("valid schemes: none | bearer | api_key"),
                )),
                Some(s) => match Auth::parse(s) {
                    Err(msg) => errors.push(ParseError::new(
                        "E0005",
                        lineno,
                        msg,
                        raw.trim(),
                        Some("valid schemes: none | bearer | api_key"),
                    )),
                    Ok(auth) => match &mut current {
                        Some(svc) => svc.auth = auth,
                        None => errors.push(ParseError::new(
                            "E0008",
                            lineno,
                            "'auth' must appear inside a service block",
                            raw.trim(),
                            Some("add a 'service <name>' line before this"),
                        )),
                    },
                },
            },

            "middleware" => {
                let names: Vec<String> = tokens.map(str::to_string).collect();
                if names.is_empty() {
                    errors.push(ParseError::new(
                        "E0008",
                        lineno,
                        "'middleware' requires at least one name",
                        raw.trim(),
                        Some("write: middleware <name> [name ...]"),
                    ));
                } else {
                    match &mut current {
                        Some(svc) => svc.middleware.extend(names),
                        None => errors.push(ParseError::new(
                            "E0008",
                            lineno,
                            "'middleware' must appear inside a service block",
                            raw.trim(),
                            Some("add a 'service <name>' line before this"),
                        )),
                    }
                }
            }

            "endpoint" => {
                let name = tokens.next();
                let method_str = name.and_then(|_| tokens.next());
                let path = method_str.and_then(|_| tokens.next());

                match (name, method_str, path) {
                    (Some(name), Some(method_str), Some(path)) => match Method::parse(method_str) {
                        Err(_) => errors.push(ParseError::new(
                            "E0001",
                            lineno,
                            format!("unknown HTTP method '{method_str}'"),
                            raw.trim(),
                            Some("valid methods: GET POST PUT PATCH DELETE HEAD OPTIONS"),
                        )),
                        Ok(method) => {
                            if !path.starts_with('/') {
                                errors.push(ParseError::new_hint(
                                    "E0002",
                                    lineno,
                                    format!("path must start with '/' (got '{path}')"),
                                    raw.trim(),
                                    format!("change to '/{path}'"),
                                ));
                            }
                            let mut ep_auth: Option<Auth> = None;
                            let mut tags: Vec<String> = Vec::new();
                            for qual in tokens {
                                if let Some(scheme) = qual.strip_prefix("auth=") {
                                    match Auth::parse(scheme) {
                                        Ok(a) => ep_auth = Some(a),
                                        Err(msg) => errors.push(ParseError::new(
                                            "E0005",
                                            lineno,
                                            msg,
                                            raw.trim(),
                                            Some("valid schemes: none | bearer | api_key"),
                                        )),
                                    }
                                } else if let Some(tag_list) = qual.strip_prefix("tags=") {
                                    tags.extend(tag_list.split(',').map(str::to_string));
                                }
                            }
                            match &mut current {
                                Some(svc) => svc.endpoints.push(Endpoint {
                                    name: name.to_string(),
                                    method,
                                    path: path.to_string(),
                                    auth: ep_auth,
                                    tags,
                                }),
                                None => errors.push(ParseError::new(
                                    "E0007",
                                    lineno,
                                    "'endpoint' must appear inside a service block",
                                    raw.trim(),
                                    Some("add a 'service <name>' line before this"),
                                )),
                            }
                        }
                    },
                    _ => errors.push(ParseError::new(
                        "E0001",
                        lineno,
                        format!(
                            "endpoint '{}' is missing method and/or path",
                            name.unwrap_or("?")
                        ),
                        raw.trim(),
                        Some("write: endpoint <name> <METHOD> <path>"),
                    )),
                }
            }

            other => errors.push(ParseError::new(
                "E0001",
                lineno,
                format!("unrecognised keyword '{other}'"),
                raw.trim(),
                Some("valid keywords: service, endpoint, auth, middleware"),
            )),
        }
    }

    // Flush last service
    if let Some(svc) = current.take() {
        if let Err(e) = check_service(&svc, lines.len(), lines.last().copied().unwrap_or("")) {
            errors.push(e);
        }
        file.services.push(svc);
    }

    if file.services.is_empty() && errors.is_empty() {
        errors.push(ParseError::new(
            "E0003",
            1,
            "no services found — file must contain at least one 'service' block",
            "",
            Some("start with: service <name>"),
        ));
    }

    // Duplicate service names
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for svc in &file.services {
        if let Some(prev) = seen.get(&svc.name) {
            errors.push(ParseError::new(
                "E0003",
                0,
                format!(
                    "duplicate service name '{}' (first defined at line {})",
                    svc.name, prev
                ),
                "",
                None,
            ));
        } else {
            seen.insert(svc.name.clone(), 0);
        }
    }

    if errors.is_empty() {
        Ok(file)
    } else {
        Err(errors)
    }
}

/// Helper to check a completed service block for internal errors.
fn check_service(svc: &Service, _lineno: usize, _raw: &str) -> Result<(), ParseError> {
    if svc.endpoints.is_empty() {
        return Err(ParseError::new_hint(
            "E0009",
            0,
            format!(
                "service '{}' has no endpoints — add at least one 'endpoint' line",
                svc.name
            ),
            "",
            format!("add: endpoint <name> GET /{}", svc.name),
        ));
    }
    let mut seen_names = std::collections::HashSet::new();
    for ep in &svc.endpoints {
        if !seen_names.insert(&ep.name) {
            return Err(ParseError::new(
                "E0004",
                0,
                format!(
                    "service '{}': duplicate endpoint name '{}'",
                    svc.name, ep.name
                ),
                "",
                None,
            ));
        }
    }
    let mut seen_routes = std::collections::HashSet::new();
    for ep in &svc.endpoints {
        let key = format!("{} {}", ep.method.as_str(), ep.path);
        if !seen_routes.insert(key.clone()) {
            return Err(ParseError::new(
                "E0006",
                0,
                format!("service '{}': conflicting route '{}' — two endpoints share the same method and path", svc.name, key),
                "",
                Some("use a different path or method for one of the endpoints"),
            ));
        }
    }
    Ok(())
}

/// Parse a `.bridge` source string.
///
/// Returns a `BridgeFile` with one or more services on success, or a
/// human-readable error string on failure.
pub fn parse(source: &str) -> Result<BridgeFile, String> {
    let mut file = BridgeFile::default();
    let mut current: Option<Service> = None;

    for (lineno, raw) in source.lines().enumerate() {
        let line = raw.trim();
        // skip blanks and comments (# or // style)
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }

        let mut tokens = line.split_whitespace();
        let keyword = tokens.next().unwrap_or("");

        match keyword {
            "service" => {
                // Flush previous service
                if let Some(svc) = current.take() {
                    validate_service(&svc, lineno)?;
                    file.services.push(svc);
                }
                let name = tokens
                    .next()
                    .ok_or_else(|| format!("line {}: 'service' requires a name", lineno + 1))?;
                validate_ident(name, lineno)?;
                current = Some(Service {
                    name: name.to_string(),
                    auth: Auth::None,
                    middleware: Vec::new(),
                    endpoints: Vec::new(),
                });
            }

            "auth" => {
                let scheme = tokens.next().ok_or_else(|| {
                    format!(
                        "line {}: 'auth' requires a scheme (none|bearer|api_key)",
                        lineno + 1
                    )
                })?;
                let auth = Auth::parse(scheme).map_err(|e| format!("line {}: {e}", lineno + 1))?;
                match &mut current {
                    Some(svc) => svc.auth = auth,
                    None => {
                        return Err(format!(
                            "line {}: 'auth' must appear inside a service block",
                            lineno + 1
                        ))
                    }
                }
            }

            "middleware" => {
                let names: Vec<String> = tokens.map(str::to_string).collect();
                if names.is_empty() {
                    return Err(format!(
                        "line {}: 'middleware' requires at least one name",
                        lineno + 1
                    ));
                }
                match &mut current {
                    Some(svc) => svc.middleware.extend(names),
                    None => {
                        return Err(format!(
                            "line {}: 'middleware' must appear inside a service block",
                            lineno + 1
                        ))
                    }
                }
            }

            "endpoint" => {
                let name = tokens
                    .next()
                    .ok_or_else(|| format!("line {}: 'endpoint' requires a name", lineno + 1))?;
                let method_str = tokens.next().ok_or_else(|| {
                    format!("line {}: endpoint '{name}' missing HTTP method", lineno + 1)
                })?;
                let path = tokens.next().ok_or_else(|| {
                    format!("line {}: endpoint '{name}' missing path", lineno + 1)
                })?;

                validate_ident(name, lineno)?;
                let method =
                    Method::parse(method_str).map_err(|e| format!("line {}: {e}", lineno + 1))?;
                if !path.starts_with('/') {
                    return Err(format!(
                        "line {}: endpoint path must start with '/' (got '{path}')",
                        lineno + 1
                    ));
                }

                // optional trailing qualifiers: auth=<scheme> tags=<t1,t2>
                let mut ep_auth: Option<Auth> = None;
                let mut tags: Vec<String> = Vec::new();
                for qual in tokens {
                    if let Some(scheme) = qual.strip_prefix("auth=") {
                        ep_auth = Some(
                            Auth::parse(scheme).map_err(|e| format!("line {}: {e}", lineno + 1))?,
                        );
                    } else if let Some(tag_list) = qual.strip_prefix("tags=") {
                        tags.extend(tag_list.split(',').map(str::to_string));
                    }
                }

                match &mut current {
                    Some(svc) => svc.endpoints.push(Endpoint {
                        name: name.to_string(),
                        method,
                        path: path.to_string(),
                        auth: ep_auth,
                        tags,
                    }),
                    None => {
                        return Err(format!(
                            "line {}: 'endpoint' must appear inside a service block",
                            lineno + 1
                        ))
                    }
                }
            }

            other => {
                return Err(format!(
                    "line {}: unrecognised keyword '{other}'",
                    lineno + 1
                ))
            }
        }
    }

    // Flush last service
    if let Some(svc) = current.take() {
        validate_service(&svc, 0)?;
        file.services.push(svc);
    }

    if file.services.is_empty() {
        return Err(
            "no services found — file must contain at least one 'service' block".to_string(),
        );
    }

    // Check for duplicate service names
    let mut seen_services = std::collections::HashSet::new();
    for svc in &file.services {
        if !seen_services.insert(&svc.name) {
            return Err(format!("duplicate service name '{}'", svc.name));
        }
    }

    Ok(file)
}

/// Convenience wrapper: parse a single-service file and return that service.
///
/// Returns an error if the file contains zero or more than one service.
pub fn compile(source: &str) -> Result<Service, String> {
    let mut file = parse(source)?;
    if file.services.len() > 1 {
        return Err(format!(
            "compile() expects a single-service file, got {}; use parse() instead",
            file.services.len()
        ));
    }
    Ok(file.services.remove(0))
}

// ── Validation helpers ────────────────────────────────────────────────────────

fn validate_ident(name: &str, lineno: usize) -> Result<(), String> {
    if name.is_empty() {
        return Err(format!("line {}: identifier cannot be empty", lineno + 1));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "line {}: identifier '{name}' must contain only alphanumeric characters, '_', or '-'",
            lineno + 1
        ));
    }
    Ok(())
}

fn validate_service(svc: &Service, lineno: usize) -> Result<(), String> {
    if svc.endpoints.is_empty() {
        return Err(format!(
            "service '{}' (near line {}): must have at least one endpoint",
            svc.name,
            lineno + 1
        ));
    }

    // Duplicate endpoint name check
    let mut seen_names = std::collections::HashSet::new();
    for ep in &svc.endpoints {
        if !seen_names.insert(&ep.name) {
            return Err(format!(
                "service '{}': duplicate endpoint name '{}'",
                svc.name, ep.name
            ));
        }
    }

    // Conflicting METHOD+path check (same method + same path = always-shadowed)
    let mut seen_routes = std::collections::HashSet::new();
    for ep in &svc.endpoints {
        let route_key = format!("{} {}", ep.method.as_str(), ep.path);
        if !seen_routes.insert(route_key.clone()) {
            return Err(format!(
                "service '{}': conflicting route '{}' — two endpoints share the same method and path",
                svc.name, route_key
            ));
        }
    }

    // Validate middleware names are valid identifiers
    for mw in &svc.middleware {
        if mw.is_empty()
            || !mw
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(format!(
                "service '{}': invalid middleware name '{}' — must be alphanumeric with _ or -",
                svc.name, mw
            ));
        }
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_service() {
        let src = "service hello\nendpoint ping GET /ping\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.name, "hello");
        assert_eq!(svc.endpoints.len(), 1);
        assert_eq!(svc.endpoints[0].method, Method::Get);
        assert_eq!(svc.endpoints[0].path, "/ping");
    }

    #[test]
    fn path_params() {
        let src = "service users\nendpoint get GET /users/:id\nendpoint delete DELETE /users/:id\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.endpoints[0].path_params(), vec!["id"]);
        assert!(svc.endpoints[0].has_path_params());
    }

    #[test]
    fn multi_segment_path_params() {
        let src = "service orders\nendpoint item GET /orders/:orderId/items/:itemId\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.endpoints[0].path_params(), vec!["orderId", "itemId"]);
    }

    #[test]
    fn auth_default_none() {
        let src = "service hello\nendpoint ping GET /ping\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.auth, Auth::None);
    }

    #[test]
    fn auth_bearer() {
        let src = "service secure\nauth bearer\nendpoint get GET /data\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.auth, Auth::Bearer);
    }

    #[test]
    fn auth_api_key() {
        let src = "service api\nauth api_key\nendpoint get GET /data\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.auth, Auth::ApiKey);
    }

    #[test]
    fn middleware() {
        let src = "service hello\nmiddleware logger cors\nendpoint ping GET /ping\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.middleware, vec!["logger", "cors"]);
    }

    #[test]
    fn per_endpoint_auth() {
        let src =
            "service mixed\nendpoint public GET /pub\nendpoint private POST /priv auth=bearer\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.endpoints[0].auth, None);
        assert_eq!(svc.endpoints[1].auth, Some(Auth::Bearer));
    }

    #[test]
    fn endpoint_tags() {
        let src = "service svc\nendpoint list GET /items tags=public,stable\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.endpoints[0].tags, vec!["public", "stable"]);
    }

    #[test]
    fn comments_and_blanks_ignored() {
        let src = "# header\n\nservice hello\n# comment\nendpoint ping GET /ping\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.name, "hello");
    }

    #[test]
    fn multiple_services() {
        let src = "service a\nendpoint p GET /p\nservice b\nendpoint q POST /q\n";
        let file = parse(src).unwrap();
        assert_eq!(file.services.len(), 2);
        assert_eq!(file.services[0].name, "a");
        assert_eq!(file.services[1].name, "b");
    }

    #[test]
    fn missing_service_name_errors() {
        assert!(compile("service\nendpoint p GET /p\n").is_err());
    }

    #[test]
    fn missing_endpoint_method_errors() {
        assert!(compile("service s\nendpoint p\n").is_err());
    }

    #[test]
    fn bad_path_errors() {
        assert!(compile("service s\nendpoint p GET no-slash\n").is_err());
    }

    #[test]
    fn empty_service_errors() {
        assert!(compile("service s\n").is_err());
    }

    #[test]
    fn duplicate_endpoint_name_errors() {
        assert!(compile("service s\nendpoint p GET /a\nendpoint p POST /b\n").is_err());
    }

    #[test]
    fn all_methods() {
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
            let src = format!("service s\nendpoint ep {m} /path\n");
            assert!(compile(&src).is_ok(), "method {m} should be valid");
        }
    }

    #[test]
    fn double_slash_comments_ignored() {
        let src = "// header comment\nservice hello\n// another comment\nendpoint ping GET /ping\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.name, "hello");
        assert_eq!(svc.endpoints.len(), 1);
    }

    #[test]
    fn mixed_comment_styles() {
        let src =
            "# hash comment\n// slash comment\nservice svc\n# ep comment\nendpoint ep GET /path\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.name, "svc");
    }

    #[test]
    fn duplicate_service_name_errors() {
        let src = "service a\nendpoint p GET /p\nservice a\nendpoint q POST /q\n";
        assert!(parse(src).is_err());
    }

    #[test]
    fn unknown_keyword_errors() {
        assert!(compile("service s\nunknown keyword\nendpoint p GET /p\n").is_err());
    }

    #[test]
    fn endpoint_outside_service_errors() {
        assert!(parse("endpoint p GET /p\n").is_err());
    }

    #[test]
    fn auth_outside_service_errors() {
        assert!(parse("auth bearer\nservice s\nendpoint p GET /p\n").is_err());
    }

    #[test]
    fn middleware_outside_service_errors() {
        assert!(parse("middleware logger\nservice s\nendpoint p GET /p\n").is_err());
    }

    #[test]
    fn path_params_multiple() {
        let src = "service api\nendpoint detail GET /a/:x/b/:y/c/:z\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.endpoints[0].path_params(), vec!["x", "y", "z"]);
    }

    #[test]
    fn endpoint_with_auth_and_tags() {
        let src = "service svc\nauth bearer\nendpoint ep GET /path auth=api_key tags=v1,beta\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.auth, Auth::Bearer);
        assert_eq!(svc.endpoints[0].auth, Some(Auth::ApiKey));
        assert_eq!(svc.endpoints[0].tags, vec!["v1", "beta"]);
    }

    #[test]
    fn invalid_auth_scheme_errors() {
        assert!(compile("service s\nauth invalid\nendpoint p GET /p\n").is_err());
    }

    #[test]
    fn invalid_method_errors() {
        assert!(compile("service s\nendpoint p INVALID /p\n").is_err());
    }

    #[test]
    fn endpoint_path_must_start_with_slash() {
        assert!(compile("service s\nendpoint p GET noslash\n").is_err());
    }

    #[test]
    fn service_must_have_name() {
        assert!(compile("service\nendpoint p GET /p\n").is_err());
    }

    #[test]
    fn middleware_list_multiple() {
        let src = "service s\nmiddleware auth rate_limit cors\nendpoint p GET /p\n";
        let svc = compile(src).unwrap();
        assert_eq!(svc.middleware, vec!["auth", "rate_limit", "cors"]);
    }

    #[test]
    fn compile_single_service_from_multi_fails() {
        let src = "service a\nendpoint p GET /p\nservice b\nendpoint q POST /q\n";
        assert!(compile(src).is_err()); // compile() only accepts single service
    }

    // ── Route conflict detection ───────────────────────────────────────────

    #[test]
    fn duplicate_route_method_path_rejected() {
        let src = "service s\nendpoint a GET /items\nendpoint b GET /items\n";
        assert!(parse(src).is_err(), "same METHOD+path should be rejected");
    }

    #[test]
    fn same_path_different_methods_ok() {
        let src = "service s\nendpoint list GET /items\nendpoint create POST /items\n";
        assert!(
            parse(src).is_ok(),
            "same path with different methods should be fine"
        );
    }

    // ── Middleware name validation ─────────────────────────────────────────

    #[test]
    fn invalid_middleware_name_rejected() {
        let src = "service s\nmiddleware has space\nendpoint p GET /p\n";
        // "has" and "space" are two separate tokens — both valid names
        assert!(parse(src).is_ok());
    }

    #[test]
    fn middleware_names_stored() {
        let src = "service s\nmiddleware auth-v2 rate_limit\nendpoint p GET /p\n";
        let file = parse(src).unwrap();
        assert_eq!(file.services[0].middleware, vec!["auth-v2", "rate_limit"]);
    }

    // ── filter_by_tag ─────────────────────────────────────────────────────

    #[test]
    fn filter_by_tag_basic() {
        let src = concat!(
            "service api\n",
            "endpoint public GET /public tags=public\n",
            "endpoint private POST /private tags=internal\n",
        );
        let file = parse(src).unwrap();
        let public = file.filter_by_tag("public");
        assert_eq!(public.endpoint_count(), 1);
        assert_eq!(public.services[0].endpoints[0].name, "public");
    }

    #[test]
    fn filter_by_tag_drops_empty_services() {
        let src = concat!(
            "service a\nendpoint x GET /x tags=alpha\n",
            "service b\nendpoint y GET /y tags=beta\n",
        );
        let file = parse(src).unwrap();
        let filtered = file.filter_by_tag("alpha");
        assert_eq!(filtered.services.len(), 1);
        assert_eq!(filtered.services[0].name, "a");
    }

    #[test]
    fn all_tags_returns_sorted_unique() {
        let src = concat!(
            "service s\n",
            "endpoint a GET /a tags=beta,alpha\n",
            "endpoint b GET /b tags=alpha,gamma\n",
        );
        let file = parse(src).unwrap();
        assert_eq!(file.all_tags(), vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn endpoint_count_total() {
        let src = concat!(
            "service a\nendpoint x GET /x\nendpoint y POST /y\n",
            "service b\nendpoint z DELETE /z\n",
        );
        let file = parse(src).unwrap();
        assert_eq!(file.endpoint_count(), 3);
    }

    // ── parse_with_errors rich diagnostics ────────────────────────────────────

    #[test]
    fn parse_with_errors_success() {
        let src = "service users\nendpoint list GET /users\n";
        let file = parse_with_errors(src).unwrap();
        assert_eq!(file.services[0].name, "users");
    }

    #[test]
    fn parse_with_errors_bad_method() {
        let errs = parse_with_errors("service s\nendpoint e FETCH /path\n").unwrap_err();
        assert_eq!(errs[0].code, "E0001");
        assert!(errs[0].message.contains("FETCH"));
        assert!(errs[0].hint.is_some());
    }

    #[test]
    fn parse_with_errors_bad_path() {
        let errs = parse_with_errors("service s\nendpoint e GET noslash\n").unwrap_err();
        assert_eq!(errs[0].code, "E0002");
        assert!(errs[0].hint.as_deref().unwrap().contains("/noslash"));
    }

    #[test]
    fn parse_with_errors_missing_service_name() {
        let errs = parse_with_errors("service\nendpoint e GET /p\n").unwrap_err();
        assert_eq!(errs[0].code, "E0010");
    }

    #[test]
    fn parse_with_errors_endpoint_outside_service() {
        let errs = parse_with_errors("endpoint e GET /p\n").unwrap_err();
        assert_eq!(errs[0].code, "E0007");
    }

    #[test]
    fn parse_with_errors_auth_outside_service() {
        let errs = parse_with_errors("auth bearer\nservice s\nendpoint e GET /p\n").unwrap_err();
        assert_eq!(errs[0].code, "E0008");
    }

    #[test]
    fn parse_with_errors_display_format() {
        let errs = parse_with_errors("service s\nendpoint e INVALID /p\n").unwrap_err();
        let display = errs[0].display("app.bridge");
        assert!(display.contains("error[E0001]"));
        assert!(display.contains("app.bridge"));
        assert!(display.contains("INVALID"));
    }

    #[test]
    fn parse_with_errors_empty_source() {
        let errs = parse_with_errors("").unwrap_err();
        assert_eq!(errs[0].code, "E0003");
    }
}
