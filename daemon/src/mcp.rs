//! MCP (Model Context Protocol) server surface (Encore commit 1828 parity).
//!
//! Exposes the daemon's control plane as a typed tool catalog an MCP
//! client (Cursor, Claude, ...) can list and invoke. The transport is
//! JSON-RPC 2.0-shaped requests over the HTTP API; a local editor wraps
//! it via stdio.
//!
//! Tool invocations dispatch through the same router the REST routes
//! use — exactly one behavior surface.
//!
//! Inspired by Encore commits 1828 (MCP server), 1830 (reconnect),
//! 1705/1708/1977 (LLM instructions), 2068 (skills/context).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use crate::http::{route, Request};
use crate::state::{SharedState, State};
use std::sync::{Arc, Mutex};

// ── Tool catalog ──────────────────────────────────────────────────────────────

/// One entry in the MCP tool catalog.
#[derive(Debug, Clone)]
pub struct McpTool {
    pub name: &'static str,
    pub description: &'static str,
    /// Default HTTP verb for the tool call.
    pub method: &'static str,
    /// Daemon API path the tool maps to.
    pub path: &'static str,
    /// Example body (empty for GET tools).
    pub body: &'static str,
}

/// The curated tool list an AI agent needs for everyday dev-loop work.
pub const TOOLS: &[McpTool] = &[
    McpTool {
        name: "compile",
        description: "Compile Bridge service source and register endpoints",
        method: "POST",
        path: "/api/v1/compile",
        body: r#"{"source":"service hello\nendpoint ping GET /ping"}"#,
    },
    McpTool {
        name: "services_list",
        description: "List registered services and their endpoints",
        method: "GET",
        path: "/api/v1/services",
        body: "",
    },
    McpTool {
        name: "routes_list",
        description: "List all registered HTTP routes",
        method: "GET",
        path: "/api/v1/routes",
        body: "",
    },
    McpTool {
        name: "traces_list",
        description: "Show recent request traces (status, latency)",
        method: "GET",
        path: "/api/v1/traces",
        body: "",
    },
    McpTool {
        name: "metrics_show",
        description: "Show request/error counters and latencies",
        method: "GET",
        path: "/api/v1/metrics",
        body: "",
    },
    McpTool {
        name: "logs_tail",
        description: "Tail recent log entries",
        method: "GET",
        path: "/api/v1/logs",
        body: "",
    },
    McpTool {
        name: "secrets_set",
        description: "Register a secret from an inline value",
        method: "POST",
        path: "/api/v1/secrets/set",
        body: r#"{"name":"db_pw","source":{"kind":"inline","value":"hunter2"}}"#,
    },
    McpTool {
        name: "secrets_check",
        description: "Verify that named secrets resolve",
        method: "POST",
        path: "/api/v1/secrets/check",
        body: r#"{"names":["db_pw"]}"#,
    },
    McpTool {
        name: "cache_write",
        description: "Write an entry into a cache keyspace",
        method: "POST",
        path: "/api/v1/cache/keyspaces/kv/entries",
        body: r#"{"key":"session:abc","value":"\"alice\""}"#,
    },
    McpTool {
        name: "publish_event",
        description: "Publish a message to a pub/sub topic",
        method: "POST",
        path: "/api/v1/pubsub/publish",
        body: r#"{"topic":"orders","message":{"id":"o_1"}}"#,
    },
    McpTool {
        name: "infra_snapshot",
        description: "Show env vars, services, databases, TLS status",
        method: "GET",
        path: "/api/v1/infra",
        body: "",
    },
    McpTool {
        name: "test_db_create",
        description: "Provision an isolated test database namespace",
        method: "POST",
        path: "/api/v1/testing/databases",
        body: r#"{"name":"users","superuser":true}"#,
    },
    McpTool {
        name: "mock_auth",
        description: "Mock auth with a canned principal for tests",
        method: "POST",
        path: "/api/v1/testing/mocks/auth",
        body: r#"{"principal":"u_test"}"#,
    },
    McpTool {
        name: "deploy_create",
        description: "Create a deployment for a target",
        method: "POST",
        path: "/api/v1/deploy",
        body: r#"{"target":"production","platform":"linux/arm64","revision":"abc123"}"#,
    },
];

/// Shared state constructor for tests and stdio bridge entry points.
pub fn new_state() -> SharedState {
    Arc::new(Mutex::new(State::new(None, None)))
}

// ── JSON-RPC handling ─────────────────────────────────────────────────────────

/// Handle one MCP request. Supports:
/// - `initialize` → protocol handshake
/// - `ping` → liveness for reconnect logic (commit 1830)
/// - `tools/list` → catalog with input-schema hints
/// - `tools/call` → `{name, method?, path?, body?}` dispatched through
///   the normal HTTP router (single source of truth)
pub fn handle(state: &SharedState, method: &str, params: &str) -> String {
    match method {
        "initialize" => ok_result(
            r#"{"protocolVersion":"2024-11-05","serverInfo":{"name":"bridge-daemon","version":""}}"#,
        ),
        "ping" => ok_result("{}"),
        "tools/list" => ok_result(&catalog_json()),
        "tools/call" => call_tool(state, params),
        other => error_result(-32601, &format!("unknown method {other}")),
    }
}

fn catalog_json() -> String {
    let items: Vec<String> = TOOLS
        .iter()
        .map(|t| {
            format!(
                r#"{{"name":"{}","description":{},"inputSchema":{{"type":"object","properties":{{"method":{{"const":"{}"}},"path":{{"const":"{}"}},"body":{{"type":"string"}}}}}}}}"#,
                t.name,
                json_str(t.description),
                t.method,
                t.path,
            )
        })
        .collect();
    format!(r#"{{"tools":[{}]}}"#, items.join(","))
}

fn call_tool(state: &SharedState, params: &str) -> String {
    let name = extract_field(params, "name").unwrap_or_default();
    let Some(tool) = TOOLS.iter().find(|t| t.name == name) else {
        return error_result(-32602, &format!("unknown tool {name}"));
    };
    let mut method = tool.method.to_string();
    let mut path = tool.path.to_string();
    let mut body = tool.body.to_string();
    // Args may override the defaults (advanced use).
    if let Some(m) = extract_field(params, "method") {
        method = m;
    }
    if let Some(p) = extract_field(params, "path") {
        path = p;
    }
    if let Some(b) = extract_field(params, "body") {
        body = b;
    }
    let req = Request::synthetic(&method, &path, &body);
    let result = route(&req, state, "mcp");
    // Status line is `HTTP/1.1 NNN ...`; flag >= 400 as tool errors.
    let is_error = result
        .strip_prefix("HTTP/1.1 ")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|code| code.parse::<u16>().ok())
        .map(|c| c >= 400)
        .unwrap_or(false);
    // MCP wraps results as content items (JSON string escaping applies).
    let esc = json_str(&result);
    ok_result(&format!(
        r#"{{"content":[{{"type":"text","text":{esc}}}],"isError":{is_error}}}"#
    ))
}

fn ok_result(inner: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","result":{inner},"id":null}}"#)
}

fn error_result(code: i32, message: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","error":{{"code":{code},"message":{}}},"id":null}}"#,
        json_str(message)
    )
}

/// Field extraction without pulling in the full parser (kept local so
/// this module has one dependency: `crate::http`).
fn extract_field(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let kpos = json.find(&key)?;
    let rest = &json[kpos + key.len()..];
    let cpos = rest.find(':')?;
    let after = rest[cpos + 1..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = after[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let e = chars.next()?;
                out.push(match e {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '"' => '"',
                    '\\' => '\\',
                    other => other,
                });
            }
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

/// Minimal JSON string escaping.
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_returns_catalog_with_schemas() {
        let resp = handle(&new_state(), "tools/list", "{}");
        assert!(resp.contains(r#""jsonrpc":"2.0","result":{"tools":["#));
        assert!(resp.contains(r#""name":"compile""#));
        assert!(resp.contains(r#""name":"deploy_create""#));
        assert_eq!(TOOLS.len(), 14, "catalog size pinned");
    }

    #[test]
    fn initialize_and_ping_for_reconnect() {
        let init = handle(&new_state(), "initialize", "{}");
        assert!(init.contains("bridge-daemon"));
        assert!(init.contains("protocolVersion"));
        assert_eq!(handle(&new_state(), "ping", "{}"), ok_result("{}"));
    }

    #[test]
    fn unknown_method_is_protocol_error() {
        let resp = handle(&new_state(), "bogus/method", "{}");
        assert!(resp.contains(r#""error":{"code":-32601"#), "got: {resp}");
    }

    #[test]
    fn tool_call_dispatches_through_real_router() {
        let s = new_state();
        // Invoke real mutating tools: set a secret, then check it resolves.
        let set = handle(
            &s,
            "tools/call",
            r#"{"name":"secrets_set","body":"{\"name\":\"k\",\"source\":{\"kind\":\"inline\",\"value\":\"v\"}}"}"#,
        );
        // Body is embedded as an escaped JSON string — assert escaped form.
        assert!(set.contains(r#"\"secret set\""#), "got: {set}");

        let chk = handle(
            &s,
            "tools/call",
            r#"{"name":"secrets_check","body":"{\"names\":[\"k\"]}"}"#,
        );
        assert!(chk.contains(r#"\"ok\":true"#), "got: {chk}");
    }

    #[test]
    fn tool_call_reports_errors_as_content() {
        let resp = handle(&new_state(), "tools/call", r#"{"name":"unknown_tool"}"#);
        assert!(resp.contains(r#""code":-32602"#), "got: {resp}");

        // A 400 from the router surfaces in content, flagged isError.
        let bad = handle(
            &new_state(),
            "tools/call",
            r#"{"name":"secrets_check","body":"{\"names\":[]}"}"#,
        );
        assert!(bad.contains(r#""isError":true"#), "got: {bad}");
    }

    #[test]
    fn get_tools_return_snapshots_as_content() {
        let s = new_state();
        let resp = handle(&s, "tools/call", r#"{"name":"infra_snapshot"}"#);
        assert!(resp.contains(r#"\"env_vars\""#), "got: {resp}");
        assert!(resp.contains(r#""isError":false"#));
    }

    #[test]
    fn json_str_escapes_control_characters() {
        assert_eq!(json_str("a\"b\\c\nd\te"), "\"a\\\"b\\\\c\\nd\\te\"");
        assert_eq!(json_str("\u{1}"), "\"\\u0001\"");
        assert_eq!(json_str("plain"), "\"plain\"");
    }
}
