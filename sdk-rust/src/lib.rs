//! BridgeBase Rust SDK — declarative infra declarations (ADR-0002).
//!
//! The builder API is the primary supported surface for emitting the
//! canonical `bridgebase.lock.json` (schema owned by the `infra-manifest`
//! crate in `bridgebase-cli`; ADR-0001). Proc-macro declarations build on
//! this same output path and land as a later layer — one canonical schema,
//! no per-language downstream special-casing (goal §9).
//!
//! Parity contract: a manifest built here must serialize byte-for-byte
//! identically to the TS emitter (`sdk-ts`) and validate against the Rust
//! schema crate. Tests pin this against the shared golden fixture.
//!
//! Canonical serialization rules (mirror `serde_json` on the schema crate):
//! - struct fields serialize in declaration order:
//!   `manifest: schema_version, app, [language], services, resources,
//!   [content_hash]`; `service: image, ports, env, buckets, topics,
//!   databases`; resource variants internally tagged `{type, ...}`
//! - maps (services / resources / ports / env) serialize key-sorted
//! - `Option::None` / empty map / empty vec are elided
//! - lock text is pretty (2-space); the hash input is compact

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Current manifest schema version. Pinned by tests against the schema
/// crate; bumping here without the owner crate is a parity break.
pub const SCHEMA_VERSION: u32 = 1;

/// Minimum lock version any reader accepts.
pub const MIN_SUPPORTED: u32 = 1;

/// Builder/verification failure modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// Name violates the DNS-label rule shared by app/service/resource names.
    BadName { kind: &'static str, name: String },
    /// A service referenced a resource kind/name never declared.
    UnknownReference {
        service: String,
        kind: &'static str,
        name: String,
    },
    /// Two resources share one name.
    DuplicateResource(String),
    /// Structural problem (empty image, wrong cron arity…).
    Invalid(String),
    /// Post-finalize mutation detected by [`Finalized::verify`].
    HashMismatch { stamped: String, actual: String },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadName { kind, name } => write!(
                f,
                "{kind} name `{name}` must be a lowercase DNS label (a-z, 0-9, '-', <=63 chars, starting with a letter)"
            ),
            Self::UnknownReference { service, kind, name } => write!(
                f,
                "service `{service}` references undeclared {kind} `{name}`"
            ),
            Self::DuplicateResource(n) => write!(f, "duplicate resource name `{n}`"),
            Self::Invalid(m) => write!(f, "invalid manifest: {m}"),
            Self::HashMismatch { stamped, actual } => write!(
                f,
                "content_hash mismatch: lock says {stamped}, contents hash to {actual}; regenerate the lock"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

// ── Data model (mirror of infra-manifest types, field order preserved) ──────

/// A declared infra resource. Internally tagged `{ "type": ... }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resource {
    /// S3-shaped object storage.
    Bucket { public: bool },
    /// Pub/sub topic.
    Topic { region: Option<String> },
    /// Relational database.
    Database { engine: DbEngine },
    /// Periodic job; 5-field cron expression (structural check only).
    Cron { schedule: String },
    /// Named secret placeholder — value lives outside the manifest always.
    Secret,
}

/// Supported database engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbEngine {
    Postgres,
}

impl DbEngine {
    fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
        }
    }
}

/// One deployable unit (OCI image plus wiring).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    pub image: String,
    pub ports: BTreeMap<u16, u16>,
    pub env: BTreeMap<String, String>,
    pub buckets: Vec<String>,
    pub topics: Vec<String>,
    pub databases: Vec<String>,
}

impl ServiceSpec {
    /// A spec with only a non-empty image; everything else empty/elided.
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            ports: BTreeMap::new(),
            env: BTreeMap::new(),
            buckets: Vec::new(),
            topics: Vec::new(),
            databases: Vec::new(),
        }
    }

    /// Map a host port to a container port.
    pub fn port(mut self, host: u16, container: u16) -> Self {
        self.ports.insert(host, container);
        self
    }

    /// Add a non-secret env var.
    pub fn env(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.env.insert(k.into(), v.into());
        self
    }

    /// Reference a declared bucket.
    pub fn bucket(mut self, name: impl Into<String>) -> Self {
        self.buckets.push(name.into());
        self
    }

    /// Reference a declared topic.
    pub fn topic(mut self, name: impl Into<String>) -> Self {
        self.topics.push(name.into());
        self
    }

    /// Reference a declared database.
    pub fn database(mut self, name: impl Into<String>) -> Self {
        self.databases.push(name.into());
        self
    }
}

// ── Canonical JSON rendering ────────────────────────────────────────────────

/// JSON value model restricted to what manifests need, preserving insertion
/// order for struct fields while sorting maps explicitly at build sites.
enum Json {
    Obj(Vec<(String, Json)>),
    Arr(Vec<Json>),
    Str(String),
    Num(u64),
    Bool(bool),
}

impl Json {
    fn write(&self, out: &mut String, pretty: bool, depth: usize) {
        match self {
            Json::Str(s) => write_json_string(s, out),
            Json::Num(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Bool(b) => {
                let _ = write!(out, "{b}");
            }
            Json::Arr(items) if items.is_empty() => out.push_str("[]"),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&"  ".repeat(depth + 1));
                    }
                    item.write(out, pretty, depth + 1);
                }
                if pretty {
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth));
                }
                out.push(']');
            }
            Json::Obj(fields) if fields.is_empty() => out.push_str("{}"),
            Json::Obj(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&"  ".repeat(depth + 1));
                    }
                    write_json_string(k, out);
                    out.push_str(if pretty { ": " } else { ":" });
                    v.write(out, pretty, depth + 1);
                }
                if pretty {
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth));
                }
                out.push('}');
            }
        }
    }
}

/// serde_json-compatible string escaping (control chars, quotes, slashes).
fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn render(v: &Json, pretty: bool) -> String {
    let mut out = String::new();
    v.write(&mut out, pretty, 0);
    out
}

// ── Manifest assembly ────────────────────────────────────────────────────────

/// Build the manifest object in canonical field order with serde's elision.
fn to_object(
    app: &str,
    language: Option<&str>,
    services: &BTreeMap<String, ServiceSpec>,
    resources: &BTreeMap<String, Resource>,
    content_hash: Option<&str>,
) -> Json {
    let mut fields: Vec<(String, Json)> = vec![
        (
            "schema_version".into(),
            Json::Num(u64::from(SCHEMA_VERSION)),
        ),
        ("app".into(), Json::Str(app.into())),
    ];
    if let Some(lang) = language {
        fields.push(("language".into(), Json::Str(lang.into())));
    }

    // Maps serialize key-sorted (BTreeMap iteration order).
    let svc_fields: Vec<(String, Json)> = services
        .iter()
        .map(|(name, s)| (name.clone(), service_object(s)))
        .collect();
    fields.push(("services".into(), Json::Obj(svc_fields)));

    let res_fields: Vec<(String, Json)> = resources
        .iter()
        .map(|(n, r)| (n.clone(), resource_object(r)))
        .collect();
    fields.push(("resources".into(), Json::Obj(res_fields)));

    if let Some(h) = content_hash {
        fields.push(("content_hash".into(), Json::Str(h.into())));
    }
    Json::Obj(fields)
}

/// Service object in declaration order with empty-container elision.
fn service_object(s: &ServiceSpec) -> Json {
    let mut fields = vec![("image".to_string(), Json::Str(s.image.clone()))];
    if !s.ports.is_empty() {
        // Port map keys are JSON strings (BTreeMap<u16, u16> via serde).
        fields.push((
            "ports".into(),
            Json::Obj(
                s.ports
                    .iter()
                    .map(|(h, c)| (h.to_string(), Json::Num(u64::from(*c))))
                    .collect(),
            ),
        ));
    }
    if !s.env.is_empty() {
        fields.push((
            "env".into(),
            Json::Obj(
                s.env
                    .iter()
                    .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                    .collect(),
            ),
        ));
    }
    for (key, list) in [
        ("buckets", &s.buckets),
        ("topics", &s.topics),
        ("databases", &s.databases),
    ] {
        if !list.is_empty() {
            fields.push((
                key.into(),
                Json::Arr(list.iter().map(|n| Json::Str(n.clone())).collect()),
            ));
        }
    }
    Json::Obj(fields)
}

/// Resource object: internally tagged, variant fields in declaration order.
fn resource_object(r: &Resource) -> Json {
    let (tag, rest): (&str, Vec<(String, Json)>) = match r {
        Resource::Bucket { public } => ("bucket", vec![("public".into(), Json::Bool(*public))]),
        Resource::Topic { region } => (
            "topic",
            match region {
                Some(r) => vec![("region".into(), Json::Str(r.clone()))],
                None => vec![],
            },
        ),
        Resource::Database { engine } => (
            "database",
            vec![("engine".into(), Json::Str(engine.as_str().into()))],
        ),
        Resource::Cron { schedule } => (
            "cron",
            vec![("schedule".into(), Json::Str(schedule.clone()))],
        ),
        Resource::Secret => ("secret", vec![]),
    };
    let mut fields = vec![("type".to_string(), Json::Str(tag.into()))];
    fields.extend(rest);
    Json::Obj(fields)
}

/// DNS-label rule, identical semantics to the schema crate and TS emitter.
fn validate_name(kind: &'static str, name: &str) -> Result<(), ManifestError> {
    let ok = !name.is_empty()
        && name.len() <= 63
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(ManifestError::BadName {
            kind,
            name: name.into(),
        })
    }
}

/// SHA-256 over the compact canonical form, hex-encoded. Implemented locally
/// so the SDK stays dependency-light; digests are pinned against the golden
/// fixture, which is produced by the schema crate's own hasher.
fn content_hash(
    app: &str,
    language: Option<&str>,
    services: &BTreeMap<String, ServiceSpec>,
    resources: &BTreeMap<String, Resource>,
) -> String {
    let obj = to_object(app, language, services, resources, None);
    sha256_hex(&render(&obj, false))
}

fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in digest {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

// ── Public builder ───────────────────────────────────────────────────────────

/// Declarative-infra builder producing `bridgebase.lock.json` output.
///
/// ```
/// use bridge_sdk_rust::{ManifestBuilder, ServiceSpec};
///
/// let lock = ManifestBuilder::new("demo")?
///     .language("rust")
///     .bucket("media", false)?
///     .topic("events", None)?
///     .database("main")?
///     .service(
///         "api",
///         ServiceSpec::new("registry.example/demo-api:1.0.0")
///             .port(8080, 8080)
///             .env("RUST_LOG", "info")
///             .bucket("media")
///             .topic("events")
///             .database("main"),
///     )?
///     .finalize()?;
/// assert!(lock.verify().is_ok());
/// # Ok::<(), bridge_sdk_rust::ManifestError>(())
/// ```
#[derive(Debug, Clone)]
pub struct ManifestBuilder {
    app: String,
    language: Option<String>,
    services: BTreeMap<String, ServiceSpec>,
    resources: BTreeMap<String, Resource>,
}

impl ManifestBuilder {
    /// Start a manifest for `app`; the name is validated immediately so bad
    /// names fail at the first call, not at finalize.
    pub fn new(app: impl Into<String>) -> Result<Self, ManifestError> {
        let app = app.into();
        validate_name("app", &app)?;
        Ok(Self {
            app,
            language: None,
            services: BTreeMap::new(),
            resources: BTreeMap::new(),
        })
    }

    /// Record the producing SDK language ("ts" | "rust" | "go").
    pub fn language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }

    fn declare(mut self, name: &str, r: Resource) -> Result<Self, ManifestError> {
        validate_name("resource", name)?;
        if self.resources.insert(name.to_string(), r).is_some() {
            return Err(ManifestError::DuplicateResource(name.into()));
        }
        Ok(self)
    }

    /// Declare an object-storage bucket.
    pub fn bucket(self, name: &str, public: bool) -> Result<Self, ManifestError> {
        self.declare(name, Resource::Bucket { public })
    }

    /// Declare a pub/sub topic (optional region).
    pub fn topic(self, name: &str, region: Option<&str>) -> Result<Self, ManifestError> {
        self.declare(
            name,
            Resource::Topic {
                region: region.map(str::to_string),
            },
        )
    }

    /// Declare a postgres database.
    pub fn database(self, name: &str) -> Result<Self, ManifestError> {
        self.declare(
            name,
            Resource::Database {
                engine: DbEngine::Postgres,
            },
        )
    }

    /// Declare a cron job; schedule must have five whitespace-separated fields.
    pub fn cron(self, name: &str, schedule: &str) -> Result<Self, ManifestError> {
        if schedule.split_whitespace().count() != 5 {
            return Err(ManifestError::Invalid(format!(
                "cron `{name}` schedule must be a 5-field cron expression"
            )));
        }
        self.declare(
            name,
            Resource::Cron {
                schedule: schedule.to_string(),
            },
        )
    }

    /// Declare a named secret (value never enters the manifest).
    pub fn secret(self, name: &str) -> Result<Self, ManifestError> {
        self.declare(name, Resource::Secret)
    }

    /// Register a deployable service. References may resolve against any
    /// resource declared before *or after* this call — full integrity is
    /// enforced at [`ManifestBuilder::finalize`].
    pub fn service(mut self, name: &str, spec: ServiceSpec) -> Result<Self, ManifestError> {
        validate_name("service", name)?;
        if spec.image.trim().is_empty() {
            return Err(ManifestError::Invalid(format!(
                "service `{name}` requires a non-empty image"
            )));
        }
        self.services.insert(name.to_string(), spec);
        Ok(self)
    }

    /// Validate referential integrity, stamp the content hash, freeze.
    pub fn finalize(self) -> Result<Finalized, ManifestError> {
        // Referential integrity across every service→resource edge.
        for (svc_name, svc) in &self.services {
            for (kind, list) in [
                ("bucket", &svc.buckets),
                ("topic", &svc.topics),
                ("database", &svc.databases),
            ] {
                for r in list {
                    if !self.resources.contains_key(r) {
                        return Err(ManifestError::UnknownReference {
                            service: svc_name.clone(),
                            kind,
                            name: r.clone(),
                        });
                    }
                }
            }
        }

        let hash = content_hash(
            &self.app,
            self.language.as_deref(),
            &self.services,
            &self.resources,
        );
        Ok(Finalized {
            app: self.app,
            language: self.language,
            services: self.services,
            resources: self.resources,
            content_hash: hash,
        })
    }
}

/// A finalized manifest: canonical text output plus verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finalized {
    app: String,
    language: Option<String>,
    services: BTreeMap<String, ServiceSpec>,
    resources: BTreeMap<String, Resource>,
    content_hash: String,
}

impl Finalized {
    /// Application name.
    pub fn app(&self) -> &str {
        &self.app
    }

    /// SHA-256 stamped at finalize time.
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    /// Pretty canonical lock text (git-friendly, matches serde_json pretty).
    pub fn to_lock(&self) -> String {
        render(
            &to_object(
                &self.app,
                self.language.as_deref(),
                &self.services,
                &self.resources,
                Some(&self.content_hash),
            ),
            true,
        )
    }

    /// Recompute and compare the stamped hash. Any divergence between the
    /// stamped value and current contents fails loudly instead of deploying
    /// a misread lock.
    pub fn verify(&self) -> Result<(), ManifestError> {
        let expect = content_hash(
            &self.app,
            self.language.as_deref(),
            &self.services,
            &self.resources,
        );
        if expect == self.content_hash {
            Ok(())
        } else {
            Err(ManifestError::HashMismatch {
                stamped: self.content_hash.clone(),
                actual: expect,
            })
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Same shape as the golden fixture (see README); pins byte parity
    /// against the TS emitter and the schema-crate-generated artifact.
    fn sample() -> Result<Finalized, ManifestError> {
        ManifestBuilder::new("demo")?
            .language("ts")
            .bucket("media", false)?
            .bucket("public-assets", true)?
            .topic("events", None)?
            .topic("eu-events", Some("eu-central"))?
            .database("main")?
            .cron("nightly-rollup", "0 3 * * *")?
            .secret("stripe-key")?
            .service(
                "api",
                ServiceSpec::new("registry.example/demo-api:1.4.2")
                    .port(8080, 8080)
                    .port(9090, 9090)
                    .env("RUST_LOG", "info")
                    .env("UPSTREAM", "https://api.example.internal")
                    .bucket("media")
                    .bucket("public-assets")
                    .topic("events")
                    .topic("eu-events")
                    .database("main"),
            )?
            .service(
                "worker",
                ServiceSpec::new("registry.example/demo-worker:1.4.2").topic("events"),
            )?
            .finalize()
    }

    #[test]
    fn sample_builds_and_verifies() {
        let lock = sample().expect("sample builds");
        assert_eq!(lock.app(), "demo");
        assert!(lock.verify().is_ok());
        assert_eq!(lock.content_hash().len(), 64);
        assert!(lock.content_hash().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn lock_text_matches_canonical_fixture_bytes() {
        // Golden fixture emitted by infra-manifest (schema owner) and
        // reproduced byte-for-byte by the TS suite; this SDK must emit the
        // exact same text from equivalent builder calls.
        let fixture = include_str!("../tests/fixtures/sample.lock.json");
        let ours = sample().expect("sample").to_lock();
        assert_eq!(ours.trim_end(), fixture.trim_end());
    }

    #[test]
    fn verify_catches_tampering() {
        let mut lock = sample().unwrap();
        lock.content_hash = "0".repeat(64);
        assert!(matches!(
            lock.verify(),
            Err(ManifestError::HashMismatch { .. })
        ));
    }

    #[test]
    fn bad_names_rejected_everywhere() {
        for bad in ["", "UPPER", "9start", "under_score"] {
            assert!(
                ManifestBuilder::new(bad).is_err(),
                "app `{bad}` must be rejected"
            );
            assert!(
                ManifestBuilder::new("ok")
                    .unwrap()
                    .bucket(bad, false)
                    .is_err(),
                "resource `{bad}` must be rejected"
            );
        }
        assert!(ManifestBuilder::new("ok")
            .unwrap()
            .service("Bad", ServiceSpec::new("i"))
            .is_err());
    }

    #[test]
    fn duplicate_resource_rejected() {
        let b = ManifestBuilder::new("demo").unwrap();
        assert!(matches!(
            b.bucket("a", false).and_then(|x| x.bucket("a", true)),
            Err(ManifestError::DuplicateResource(_))
        ));
    }

    #[test]
    fn cron_requires_five_fields() {
        let b = ManifestBuilder::new("demo").unwrap();
        assert!(b.cron("j", "0 3 * *").is_err());
        let b = ManifestBuilder::new("demo").unwrap();
        assert!(b.cron("j", "0 3 * * *").is_ok());
    }

    #[test]
    fn empty_image_rejected() {
        let b = ManifestBuilder::new("demo").unwrap();
        assert!(matches!(
            b.service("api", ServiceSpec::new("   ")),
            Err(ManifestError::Invalid(_))
        ));
    }

    #[test]
    fn undeclared_reference_fails_finalize_not_service_call() {
        // Declare-after-use is allowed at declaration time; integrity is
        // enforced once, at finalize.
        let late_ref = ManifestBuilder::new("demo")
            .unwrap()
            .service("api", ServiceSpec::new("img:1").bucket("ghost"));
        assert!(late_ref.is_ok());
        assert!(matches!(
            late_ref.unwrap().finalize(),
            Err(ManifestError::UnknownReference { .. })
        ));

        // Declaring the resource afterwards makes the same manifest valid.
        let ok = ManifestBuilder::new("demo")
            .unwrap()
            .service("api", ServiceSpec::new("img:1").bucket("ghost"))
            .unwrap()
            .bucket("ghost", false)
            .unwrap()
            .finalize();
        assert!(ok.is_ok());
    }

    #[test]
    fn hash_changes_when_contents_change() {
        let a = ManifestBuilder::new("demo")
            .unwrap()
            .bucket("x", false)
            .unwrap()
            .finalize()
            .unwrap();
        let b = ManifestBuilder::new("demo")
            .unwrap()
            .bucket("x", true)
            .unwrap()
            .finalize()
            .unwrap();
        assert_ne!(a.content_hash(), b.content_hash());
    }
}
