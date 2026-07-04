//! Docker Postgres management.
//!
//! Wraps `docker` CLI commands to create, inspect, migrate, and destroy
//! PostgreSQL containers. All operations shell out to `docker` via
//! `std::process::Command` — no external Rust crates needed.

use std::process::Command as ProcessCommand;

/// Derive the container name from a user-supplied name.
fn container_name(name: &str) -> String {
    format!("bridge_pg_{name}")
}

/// Returns `true` if Docker is installed and responsive.
pub fn docker_available() -> bool {
    ProcessCommand::new("docker")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a new Postgres 16 container named `bridge_pg_<name>`.
///
/// ```text
/// docker run -d --name bridge_pg_<name> \
///   -e POSTGRES_PASSWORD=bridge -p 5432:5432 postgres:16
/// ```
pub fn create(name: &str) -> Result<String, String> {
    if !docker_available() {
        return Err("Docker is not available on this system".to_string());
    }
    let cname = container_name(name);
    let output = ProcessCommand::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            &cname,
            "-e",
            "POSTGRES_PASSWORD=bridge",
            "-p",
            "5432:5432",
            "postgres:16",
        ])
        .output()
        .map_err(|e| format!("failed to run docker: {e}"))?;

    if output.status.success() {
        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(format!("created container {cname} ({id})"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("docker run failed: {stderr}"))
    }
}

/// List running Bridge Postgres containers.
pub fn status() -> Result<String, String> {
    if !docker_available() {
        return Ok("docker not available".to_string());
    }
    let output = ProcessCommand::new("docker")
        .args([
            "ps",
            "--filter",
            "name=bridge_pg_",
            "--format",
            "{{.Names}}\t{{.Status}}",
        ])
        .output()
        .map_err(|e| format!("docker ps failed: {e}"))?;

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        Ok("no bridge postgres containers running".to_string())
    } else {
        Ok(text)
    }
}

/// Execute SQL against the first running `bridge_pg_*` container via `psql`.
pub fn migrate(sql: &str) -> Result<String, String> {
    if !docker_available() {
        return Err("Docker is not available on this system".to_string());
    }
    // Find a running bridge_pg container
    let ps_output = ProcessCommand::new("docker")
        .args([
            "ps",
            "--filter",
            "name=bridge_pg_",
            "--format",
            "{{.Names}}",
            "-q",
        ])
        .output()
        .map_err(|e| format!("docker ps failed: {e}"))?;
    let containers = String::from_utf8_lossy(&ps_output.stdout).trim().to_string();
    if containers.is_empty() {
        return Err("no running bridge postgres containers found".to_string());
    }
    // Use first container found via its name
    let first_ps = ProcessCommand::new("docker")
        .args([
            "ps",
            "--filter",
            "name=bridge_pg_",
            "--format",
            "{{.Names}}",
        ])
        .output()
        .map_err(|e| format!("docker ps failed: {e}"))?;
    let cname = String::from_utf8_lossy(&first_ps.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if cname.is_empty() {
        return Err("no running bridge postgres containers found".to_string());
    }

    let output = ProcessCommand::new("docker")
        .args(["exec", "-i", &cname, "psql", "-U", "postgres", "-c", sql])
        .output()
        .map_err(|e| format!("docker exec psql failed: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("psql error: {stderr}"))
    }
}

/// Stop and remove the container `bridge_pg_<name>`.
pub fn destroy(name: &str) -> Result<String, String> {
    if !docker_available() {
        return Err("Docker is not available on this system".to_string());
    }
    let cname = container_name(name);
    // Stop
    let _ = ProcessCommand::new("docker").args(["stop", &cname]).output();
    // Remove
    let output = ProcessCommand::new("docker")
        .args(["rm", "-f", &cname])
        .output()
        .map_err(|e| format!("docker rm failed: {e}"))?;

    if output.status.success() {
        Ok(format!("destroyed container {cname}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("docker rm failed: {stderr}"))
    }
}
