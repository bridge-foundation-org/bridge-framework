//! Docker Postgres lifecycle management.
//!
//! All operations shell out to `docker` via `std::process::Command`.
//! No external Rust crates required.
//!
//! Container naming convention: `bridge_pg_<name>`
//! Default credentials: user=`postgres`, password=`bridge`, port=`5432`.

use std::process::Command;

const PG_IMAGE: &str = "postgres:16";
const PG_PASSWORD: &str = "bridge";
const PG_USER: &str = "postgres";

// ── Helpers ───────────────────────────────────────────────────────────────────

fn cname(name: &str) -> String {
    format!("bridge_pg_{name}")
}

/// Returns true when Docker is installed and responsive.
pub fn docker_available() -> bool {
    Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run(args: &[&str]) -> Result<String, String> {
    let out = Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| format!("docker exec failed: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Create and start a new Postgres container.
///
/// Skips creation if a container with the same name already exists.
pub fn create(name: &str) -> Result<String, String> {
    if !docker_available() {
        return Err("Docker is not available on this system".into());
    }
    let cn = cname(name);
    // Check if already running
    let existing = run(&["ps", "-a", "--filter", &format!("name={cn}"), "--format", "{{.Names}}"])?;
    if !existing.is_empty() {
        return Err(format!("container '{cn}' already exists — use 'pg-destroy {name}' first"));
    }
    let id = run(&[
        "run", "-d",
        "--name", &cn,
        "-e", &format!("POSTGRES_PASSWORD={PG_PASSWORD}"),
        "-e", &format!("POSTGRES_USER={PG_USER}"),
        "-p", "5432:5432",
        PG_IMAGE,
    ])?;
    Ok(format!("created {cn} ({id})"))
}

/// List all bridge_pg_* containers (running and stopped).
pub fn status() -> Result<String, String> {
    if !docker_available() {
        return Ok("docker not available".into());
    }
    let output = run(&[
        "ps", "-a",
        "--filter", "name=bridge_pg_",
        "--format", r#"{"name":"{{.Names}}","status":"{{.Status}}","id":"{{.ID}}"}"#,
    ])?;
    if output.is_empty() {
        Ok(r#"{"containers":[],"message":"no bridge postgres containers found"}"#.into())
    } else {
        // wrap lines as JSON array
        let items: Vec<&str> = output.lines().collect();
        Ok(format!("{{\"containers\":[{}]}}", items.join(",")))
    }
}

/// Run a SQL statement against the named container via `psql`.
pub fn migrate(sql: &str) -> Result<String, String> {
    if !docker_available() {
        return Err("Docker is not available".into());
    }
    // Find first running bridge_pg_ container
    let cn = run(&[
        "ps",
        "--filter", "name=bridge_pg_",
        "--filter", "status=running",
        "--format", "{{.Names}}",
    ])?;
    let cn = cn.lines().next().unwrap_or("").trim().to_string();
    if cn.is_empty() {
        return Err("no running bridge postgres container found".into());
    }
    let out = Command::new("docker")
        .args(["exec", "-i", &cn, "psql", "-U", PG_USER, "-c", sql])
        .output()
        .map_err(|e| format!("docker exec psql: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Stop and remove the container.
pub fn destroy(name: &str) -> Result<String, String> {
    if !docker_available() {
        return Err("Docker is not available".into());
    }
    let cn = cname(name);
    let _ = run(&["stop", &cn]);
    run(&["rm", "-f", &cn]).map(|_| format!("destroyed {cn}"))
}
