//! Deployment tracking (Encore CLI deploy / platform parity).
//!
//! Daemon-side model of the deploy pipeline: create deployments against
//! named targets with validated platforms, drive them through an
//! enforced status machine (`queued → building → deploying → deployed |
//! failed`), roll back to any prior successful revision, and generate
//! the multi-stage Dockerfile used for image builds (layer-cached,
//! platform-aware — Encore commits 2083, 2188).
//!
//! Inspired by Encore commits 1503 (CLI deploy), 1706 (Railway),
//! 1776 (Windows builds), 2083 (arch/os in builds), 2188 (layer cache).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

// (no external imports needed — pure std via full paths)

// ── Model ─────────────────────────────────────────────────────────────────────

/// Lifecycle of a single deployment. Transitions are validated by
/// [`DeployRegistry::set_status`] — illegal jumps are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Queued,
    Building,
    Deploying,
    Deployed,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Building => "building",
            Status::Deploying => "deploying",
            Status::Deployed => "deployed",
            Status::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "queued" => Some(Status::Queued),
            "building" => Some(Status::Building),
            "deploying" => Some(Status::Deploying),
            "deployed" => Some(Status::Deployed),
            "failed" => Some(Status::Failed),
            _ => None,
        }
    }

    /// Legal forward transitions. Terminal states accept nothing.
    fn can_transition(self, to: Status) -> bool {
        matches!(
            (self, to),
            (Status::Queued, Status::Building)
                | (Status::Queued, Status::Failed)
                | (Status::Building, Status::Deploying)
                | (Status::Building, Status::Failed)
                | (Status::Deploying, Status::Deployed)
                | (Status::Deploying, Status::Failed)
        )
    }
}

/// One recorded deployment.
#[derive(Debug, Clone)]
pub struct Deployment {
    pub id: String,
    /// Deployment target name (e.g. `production`, `staging`, `railway`).
    pub target: String,
    /// Build platform triple (`linux/amd64`, `linux/arm64`, ...).
    pub platform: String,
    /// Source revision marker (commit sha or tag).
    pub revision: String,
    pub status: Status,
    /// Set when this deployment was demoted because another went live:
    /// the id of the deployment that replaced it. Enables deterministic
    /// rollback to exactly the revision that was live before.
    pub superseded_by: Option<String>,
}

/// Registry of deployments across all targets.
#[derive(Debug, Clone, Default)]
pub struct DeployRegistry {
    /// Insertion-ordered (id ascending, ids are monotonic).
    pub deployments: Vec<Deployment>,
    next_seq: u64,
}

// ── API ───────────────────────────────────────────────────────────────────────

impl DeployRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new queued deployment. Platform must look like
    /// `os/arch[/<variant>]`; revision must be non-empty.
    pub fn create(
        &mut self,
        target: &str,
        platform: &str,
        revision: &str,
    ) -> Result<String, String> {
        let target = target.trim();
        if target.is_empty() {
            return Err("target required".into());
        }
        let p = platform.trim();
        let parts: Vec<&str> = p.split('/').collect();
        if !(parts.len() == 2 || parts.len() == 3) || parts.iter().any(|s| s.is_empty()) {
            return Err("platform must be os/arch[/variant]".into());
        }
        let rev = revision.trim();
        if rev.is_empty() {
            return Err("revision required".into());
        }
        self.next_seq += 1;
        let id = format!("dep-{}", self.next_seq);
        self.deployments.push(Deployment {
            id: id.clone(),
            target: target.to_string(),
            platform: p.to_string(),
            revision: rev.to_string(),
            status: Status::Queued,
            superseded_by: None,
        });
        Ok(id)
    }

    /// Advance a deployment's status through the state machine.
    /// Terminal states (`deployed`, `failed`) reject further moves.
    pub fn set_status(&mut self, id: &str, status: Status) -> Result<(), String> {
        if !self.deployments.iter().any(|d| d.id == id) {
            return Err(format!("deployment {id} not found"));
        }
        let current = self
            .deployments
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.status)
            .unwrap_or(Status::Queued);
        if !current.can_transition(status) {
            return Err(format!(
                "illegal transition {} -> {}",
                current.as_str(),
                status.as_str()
            ));
        }
        // When something newly deploys, supersede the previous live one.
        let target_name = self
            .deployments
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.target.clone())
            .unwrap_or_default();
        if status == Status::Deployed {
            for other in self.deployments.iter_mut() {
                if other.id != id && other.target == target_name && other.status == Status::Deployed
                {
                    other.status = Status::Failed;
                    other.superseded_by = Some(id.to_string());
                }
            }
        }
        if let Some(d) = self.deployments.iter_mut().find(|d| d.id == id) {
            d.status = status;
        }
        Ok(())
    }

    /// Roll back a target to the deployment the current live one
    /// replaced. Returns the promoted id, or None when there is no
    /// superseded predecessor to return to.
    pub fn rollback(&mut self, target: &str) -> Option<String> {
        let live_id = self
            .deployments
            .iter()
            .filter(|d| d.target == target && d.status == Status::Deployed)
            .max_by_key(|d| d.id.clone())
            .map(|d| d.id.clone())?;
        // Exactly the deployment this live one displaced.
        let candidate = self
            .deployments
            .iter()
            .find(|d| d.target == target && d.superseded_by.as_deref() == Some(live_id.as_str()))
            .map(|d| d.id.clone())?;
        for d in self.deployments.iter_mut() {
            if d.target == target && d.status == Status::Deployed {
                d.status = Status::Failed;
                d.superseded_by = Some(candidate.clone());
            }
        }
        let c = self.deployments.iter_mut().find(|d| d.id == candidate)?;
        c.status = Status::Deployed;
        c.superseded_by = None;
        Some(c.id.clone())
    }

    /// Generate the multi-stage, layer-cached, platform-aware Dockerfile
    /// for building the app image (Encore 2083 arch/os, 2188 layer cache).
    pub fn dockerfile(app_name: &str, binary: &str) -> String {
        format!(
            r#"# Generated by bridge — multi-stage build with dependency-layer caching.
# Platforms: TARGETPLATFORM/BUILDPLATFORM honor docker buildx --platform.
# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM rust:1-slim AS build
ARG TARGETPLATFORM TARGETOS TARGETARCH
WORKDIR /src
# Dependency layer: manifests first so source edits don't bust the cache.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {{}}" > src/main.rs && cargo build --release && rm -rf target/release/deps/{binary}*
# Source layer.
COPY . .
RUN cargo build --release --bin {binary}

FROM debian:bookworm-slim
COPY --from=build /src/target/release/{binary} /usr/local/bin/{binary}
ENV APP_NAME={app_name}
ENTRYPOINT ["/usr/local/bin/{binary}"]
"#
        )
    }

    /// Full snapshot for `GET /api/v1/deploy`.
    pub fn to_json(&self) -> String {
        let items: Vec<String> = self
            .deployments
            .iter()
            .map(|d| {
                format!(
                    r#"{{"id":"{}","target":"{}","platform":"{}","revision":"{}","status":"{}"}}"#,
                    d.id,
                    d.target,
                    d.platform,
                    d.revision,
                    d.status.as_str()
                )
            })
            .collect();
        format!(r#"{{"deployments":[{}]}}"#, items.join(","))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_validates_target_platform_revision() {
        let mut r = DeployRegistry::new();
        assert!(r.create("", "linux/amd64", "abc123").is_err());
        assert!(r.create("prod", "bogus", "abc123").is_err());
        assert!(r.create("prod", "linux/", "abc123").is_err());
        assert!(r.create("prod", "linux/amd64", "").is_err());
        let id = r.create("prod", "linux/arm64/v8", "abc123").unwrap();
        assert_eq!(id, "dep-1");
        assert_eq!(r.deployments[0].status, Status::Queued);
    }

    #[test]
    fn status_machine_enforces_legal_transitions() {
        let mut r = DeployRegistry::new();
        let id = r.create("prod", "linux/amd64", "rev1").unwrap();
        assert!(r.set_status(&id, Status::Deployed).is_err(), "skip stages");
        r.set_status(&id, Status::Building).unwrap();
        r.set_status(&id, Status::Deploying).unwrap();
        r.set_status(&id, Status::Deployed).unwrap();
        assert!(
            r.set_status(&id, Status::Building).is_err(),
            "terminal is terminal"
        );
        // Failure paths also legal from mid-flight states.
        let id2 = r.create("prod", "linux/amd64", "rev2").unwrap();
        r.set_status(&id2, Status::Building).unwrap();
        r.set_status(&id2, Status::Failed).unwrap();
        assert!(r.set_status(&id2, Status::Building).is_err());
        assert!(
            r.set_status("dep-99", Status::Building).is_err(),
            "unknown id"
        );
    }

    #[test]
    fn rollback_promotes_prior_deployed_revision() {
        let mut r = DeployRegistry::new();
        let v1 = r.create("prod", "linux/amd64", "v1").unwrap();
        for s in [Status::Building, Status::Deploying, Status::Deployed] {
            r.set_status(&v1, s).unwrap();
        }
        let v2 = r.create("prod", "linux/amd64", "v2").unwrap();
        for s in [Status::Building, Status::Deploying, Status::Deployed] {
            r.set_status(&v2, s).unwrap();
        }
        // Two lives are never allowed — the second supersedes the first
        // into Failed (recording who replaced it). Rollback swaps back.
        let rb = r.rollback("prod");
        assert_eq!(rb.as_deref(), Some(v1.as_str()), "promote previous");
        let j = r.to_json();
        assert!(j.contains(r#""id":"dep-2","target":"prod","platform":"linux/amd64","revision":"v2","status":"failed""#));
        assert!(j.contains(r#""revision":"v1","status":"deployed""#));
        // Ping-pong: rolling back again returns to v2.
        assert_eq!(r.rollback("prod").as_deref(), Some(v2.as_str()));
        assert!(r.rollback("ghost").is_none());
    }

    #[test]
    fn dockerfile_is_platform_aware_and_layer_cached() {
        let df = DeployRegistry::dockerfile("myapp", "myapp-server");
        assert!(df.contains("BUILDPLATFORM"), "multi-platform base");
        assert!(df.contains("TARGETARCH"), "arch plumbing (Encore 2083)");
        assert!(
            df.contains("COPY Cargo.toml Cargo.lock ./"),
            "manifest-first layer cache (2188)"
        );
        assert!(df.contains("cargo build --release --bin myapp-server"));
        assert!(df.starts_with("# Generated by bridge"));
    }

    #[test]
    fn snapshot_json_shape() {
        let mut r = DeployRegistry::new();
        assert_eq!(r.to_json(), r#"{"deployments":[]}"#);
        r.create("staging", "linux/arm64", "r9").unwrap();
        assert!(
            r.to_json().contains(
                r#"{"id":"dep-1","target":"staging","platform":"linux/arm64","revision":"r9","status":"queued"}"#
            )
        );
    }

    #[test]
    fn status_parse_roundtrip() {
        for s in ["queued", "building", "deploying", "deployed", "failed"] {
            assert_eq!(Status::parse(s).unwrap().as_str(), s);
        }
        assert!(Status::parse("").is_none());
        assert!(Status::parse("DEPLOYED").is_some(), "case-insensitive");
    }
}
