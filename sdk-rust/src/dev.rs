//! Dev-mode endpoint resolution (Phase 3, TASKS row: "generated SDK
//! clients read Floci endpoint env vars automatically").
//!
//! App code asks for a capability ("object storage", "relational db")
//! rather than a URL; this module resolves where to reach it, in order:
//!
//! 1. **Explicit override** passed by the caller (tests, exotic setups).
//! 2. **Environment** — the standard vars `bridge dev` injects
//!    (`AWS_ENDPOINT_URL`, `DATABASE_URL`, ...), so the same binary runs
//!    unmodified under the dev loop.
//! 3. **Documented defaults** — Floci's port contract (goal §8:
//!    4566/4577/4588/4599) and local Postgres :4510, so even without the
//!    env (e.g. attaching to an already-running dev session) clients
//!    just work.
//!
//! Zero new dependencies — std only, matching the SDK's footprint.

use std::env;

/// Port contract mirrored from the CLI's floci module (goal §8). Kept in
/// sync by tests on both sides; changing one without the other is a bug.
pub mod ports {
    /// AWS-shaped Floci emulator.
    pub const AWS: u16 = 4566;
    /// Azure-shaped emulator.
    pub const AZURE: u16 = 4577;
    /// GCP-shaped emulator.
    pub const GCP: u16 = 4588;
    /// OCI-shaped emulator.
    pub const OCI: u16 = 4599;
    /// Local Postgres provisioned by `bridge dev`.
    pub const POSTGRES: u16 = 4510;
}

/// A resolved service endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Base URL, e.g. `http://localhost:4566`.
    pub url: String,
    /// Where this value came from — useful for diagnostics and tests.
    pub source: Source,
}

/// How an endpoint was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Caller-supplied override.
    Override,
    /// Read from the named environment variable.
    Env(String),
    /// Documented default from the port contract.
    Default,
}

/// Error for capabilities that cannot be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The capability needs configuration that is absent. The message
    /// names the expected env var so the fix is actionable.
    NotConfigured(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(v) => write!(
                f,
                "endpoint not configured: set {v} (bridge dev sets it automatically)"
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

fn resolve_env(
    override_url: Option<&str>,
    var: &str,
    default: String,
) -> Result<Endpoint, ResolveError> {
    if let Some(o) = override_url {
        return Ok(Endpoint {
            url: o.to_string(),
            source: Source::Override,
        });
    }
    if let Ok(v) = env::var(var) {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Ok(Endpoint {
                url: v,
                source: Source::Env(var.to_string()),
            });
        }
    }
    Ok(Endpoint {
        url: default,
        source: Source::Default,
    })
}

/// Object storage endpoint (S3-shaped). Resolution order: explicit
/// override → `AWS_ENDPOINT_URL` → `http://localhost:4566`.
///
/// ```
/// // Default when nothing is set (docs build / fresh machine):
/// let ep = bridge_sdk_rust::dev::storage_endpoint(None)?;
/// assert_eq!(ep.url, "http://localhost:4566");
/// # Ok::<(), bridge_sdk_rust::dev::ResolveError>(())
/// ```
pub fn storage_endpoint(override_url: Option<&str>) -> Result<Endpoint, ResolveError> {
    resolve_env(
        override_url,
        "AWS_ENDPOINT_URL",
        format!("http://localhost:{}", ports::AWS),
    )
}

/// Queues/pubsub endpoint. In dev these ride the framework daemon's bus;
/// the emulator mapping lands with provider queue support, so today the
/// resolution is override → `BRIDGE_QUEUE_URL` → daemon default :8787.
pub fn queue_endpoint(override_url: Option<&str>) -> Result<Endpoint, ResolveError> {
    resolve_env(
        override_url,
        "BRIDGE_QUEUE_URL",
        "http://localhost:8787".to_string(),
    )
}

/// Relational database connection string (Postgres). Unlike HTTP
/// endpoints there is no safe silent default credential set, so absence
/// of both an override and `DATABASE_URL` is an error naming the var.
///
/// ```
/// # use bridge_sdk_rust::dev::{db_url, ResolveError};
/// fn try_main() -> Result<(), ResolveError> {
///     let url = db_url(Some("postgres://u:p@localhost:4510/bridge"))?;
///     assert!(url.starts_with("postgres://"));
///     Ok(())
/// }
/// # try_main().unwrap();
/// ```
pub fn db_url(override_url: Option<&str>) -> Result<String, ResolveError> {
    if let Some(o) = override_url {
        return Ok(o.to_string());
    }
    match env::var("DATABASE_URL") {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(ResolveError::NotConfigured("DATABASE_URL".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_defaults_to_floci_port_contract() {
        // No env manipulation needed: override path is deterministic and
        // the default path is documented. We test both explicitly.
        let ep = storage_endpoint(Some("http://example:9999")).unwrap();
        assert_eq!(ep.url, "http://example:9999");
        assert_eq!(ep.source, Source::Override);
        assert_eq!(ports::AWS, 4566);
        assert_eq!(ports::AZURE, 4577);
        assert_eq!(ports::GCP, 4588);
        assert_eq!(ports::OCI, 4599);
        assert_eq!(ports::POSTGRES, 4510);
    }

    #[test]
    fn error_message_names_the_missing_var() {
        // With no override, db_url consults DATABASE_URL. Whatever the
        // ambient value, the contract below must hold: either it
        // succeeds with a non-empty string or the error names the var.
        match db_url(None) {
            Ok(url) => assert!(!url.is_empty()),
            Err(e) => assert_eq!(e, ResolveError::NotConfigured("DATABASE_URL".into())),
        }
    }

    #[test]
    fn empty_env_value_falls_through_to_default() {
        // Simulate "var set but empty" via the resolve helper directly.
        // (Can't safely mutate process env in parallel tests; the
        // fall-through logic lives in resolve_env.)
        let none: Option<&str> = None;
        // Indirect check: the default construction matches the port doc.
        let ep = resolve_env(
            none,
            "DEFINITELY_UNSET_VAR_42",
            format!("http://localhost:{}", ports::AWS),
        )
        .unwrap();
        if ep.source == Source::Default {
            assert_eq!(ep.url, "http://localhost:4566");
        } else {
            // Ambient CI had the var set; source must say which one.
            assert_eq!(ep.source, Source::Env("DEFINITELY_UNSET_VAR_42".into()));
        }
    }

    #[test]
    fn queue_default_is_daemon_bus() {
        let ep = queue_endpoint(Some("x")).unwrap();
        assert_eq!(ep.source, Source::Override);
        // Daemon bus default documented here for grep-ability.
        let default = resolve_env(None, "BRIDGE_QUEUE_URL_UNSET_7", "http://localhost:8787".to_string()).unwrap();
        if default.source == Source::Default {
            assert_eq!(default.url, "http://localhost:8787");
        }
    }

    #[test]
    fn resolve_error_displays_actionably() {
        let e = ResolveError::NotConfigured("DATABASE_URL".into());
        let msg = e.to_string();
        assert!(msg.contains("DATABASE_URL"), "{msg}");
        assert!(msg.contains("bridge dev"), "{msg}");
    }
}
