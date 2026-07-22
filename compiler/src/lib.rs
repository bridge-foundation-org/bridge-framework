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
            Method::Get     => "GET",
            Method::Post    => "POST",
            Method::Put     => "PUT",
            Method::Patch   => "PATCH",
            Method::Delete  => "DELETE",
            Method::Head    => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_uppercase().as_str() {
            "GET"     => Ok(Method::Get),
            "POST"    => Ok(Method::Post),
            "PUT"     => Ok(Method::Put),
            "PATCH"   => Ok(Method::Patch),
            "DELETE"  => Ok(Method::Delete),
            "HEAD"    => Ok(Method::Head),
            "OPTIONS" => Ok(Method::Options),
            other     => Err(format!("unknown HTTP method: {other}")),
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
            "none"    => Ok(Auth::None),
            "bearer"  => Ok(Auth::Bearer),
            "api_key" | "apikey" => Ok(Auth::ApiKey),
            other => Err(format!("unknown auth scheme: {other} (use: none|bearer|api_key)")),
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Auth::None   => "none",
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

// ── Parser ────────────────────────────────────────────────────────────────────

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
                let name = tokens.next()
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
                let scheme = tokens.next()
                    .ok_or_else(|| format!("line {}: 'auth' requires a scheme (none|bearer|api_key)", lineno + 1))?;
                let auth = Auth::parse(scheme)
                    .map_err(|e| format!("line {}: {e}", lineno + 1))?;
                match &mut current {
                    Some(svc) => svc.auth = auth,
                    None => return Err(format!("line {}: 'auth' must appear inside a service block", lineno + 1)),
                }
            }

            "middleware" => {
                let names: Vec<String> = tokens.map(str::to_string).collect();
                if names.is_empty() {
                    return Err(format!("line {}: 'middleware' requires at least one name", lineno + 1));
                }
                match &mut current {
                    Some(svc) => svc.middleware.extend(names),
                    None => return Err(format!("line {}: 'middleware' must appear inside a service block", lineno + 1)),
                }
            }

            "endpoint" => {
                let name = tokens.next()
                    .ok_or_else(|| format!("line {}: 'endpoint' requires a name", lineno + 1))?;
                let method_str = tokens.next()
                    .ok_or_else(|| format!("line {}: endpoint '{name}' missing HTTP method", lineno + 1))?;
                let path = tokens.next()
                    .ok_or_else(|| format!("line {}: endpoint '{name}' missing path", lineno + 1))?;

                validate_ident(name, lineno)?;
                let method = Method::parse(method_str)
                    .map_err(|e| format!("line {}: {e}", lineno + 1))?;
                if !path.starts_with('/') {
                    return Err(format!("line {}: endpoint path must start with '/' (got '{path}')", lineno + 1));
                }

                // optional trailing qualifiers: auth=<scheme> tags=<t1,t2>
                let mut ep_auth: Option<Auth> = None;
                let mut tags: Vec<String> = Vec::new();
                for qual in tokens {
                    if let Some(scheme) = qual.strip_prefix("auth=") {
                        ep_auth = Some(Auth::parse(scheme)
                            .map_err(|e| format!("line {}: {e}", lineno + 1))?);
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
                    None => return Err(format!("line {}: 'endpoint' must appear inside a service block", lineno + 1)),
                }
            }

            other => return Err(format!("line {}: unrecognised keyword '{other}'", lineno + 1)),
        }
    }

    // Flush last service
    if let Some(svc) = current.take() {
        validate_service(&svc, 0)?;
        file.services.push(svc);
    }

    if file.services.is_empty() {
        return Err("no services found — file must contain at least one 'service' block".to_string());
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
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
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
            svc.name, lineno + 1
        ));
    }
    // Duplicate endpoint name check
    let mut seen = std::collections::HashSet::new();
    for ep in &svc.endpoints {
        if !seen.insert(&ep.name) {
            return Err(format!(
                "service '{}': duplicate endpoint name '{}'",
                svc.name, ep.name
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
        let src = "service mixed\nendpoint public GET /pub\nendpoint private POST /priv auth=bearer\n";
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
        let src = "# hash comment\n// slash comment\nservice svc\n# ep comment\nendpoint ep GET /path\n";
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
}
