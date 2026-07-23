# compiler — Bridge DSL Parser

Parses `.bridge` source files into a typed AST.

## Overview

The `compiler` crate is a pure-stdlib recursive-descent parser for the Bridge DSL. Given a source string it produces a `BridgeFile` containing one or more `Service` structs, each with their `Endpoint` list. It validates:

- Valid HTTP methods
- Path format (must start with `/`)
- Duplicate service names within a file
- Duplicate endpoint names within a service
- Auth scheme names (`none` | `bearer` | `api_key`)
- Route conflicts — two endpoints with the same method + path
- Endpoints / auth / middleware lines declared outside a `service` block

## DSL Syntax

```bridge
# comment

service <name>
  [auth <scheme>]         # bearer | api_key | none  (default: none)
  [middleware <name>...]  # space-separated hook names

endpoint <name> <METHOD> <path>
  [auth <scheme>]         # per-endpoint override
  [tags <tag>...]
```

### Path Parameters

Path segments starting with `:` become typed parameters in the generated client:

```bridge
service users
endpoint get    GET    /users/:id
endpoint list   GET    /users
endpoint create POST   /users
endpoint update PUT    /users/:id
endpoint delete DELETE /users/:id
```

### Multiple Services Per File

A single `.bridge` file may declare multiple services. Each `service` line starts a new service block:

```bridge
service users
auth bearer
endpoint list   GET  /users
endpoint create POST /users

service posts
endpoint list   GET  /posts/:userId
endpoint create POST /posts
```

## Quick Start

```rust
use compiler::parse;

let src = r#"
service users
auth bearer
endpoint list   GET  /users
endpoint get    GET  /users/:id
endpoint create POST /users
"#;

let file = parse(src).unwrap();
let svc = &file.services[0];
assert_eq!(svc.name, "users");
assert_eq!(svc.auth, compiler::Auth::Bearer);
assert_eq!(svc.endpoints.len(), 3);
assert_eq!(svc.endpoints[1].path_params, vec!["id"]);
```

## API Reference

### `parse(source: &str) → Result<BridgeFile, Vec<ParseError>>`

Parses the entire source. Returns `Ok(BridgeFile)` on success, or
`Err(Vec<ParseError>)` with one entry per validation failure.

### `parse_with_source(source: &str, filename: &str) → Result<BridgeFile, Vec<ParseError>>`

Same as `parse` but attaches `filename` to each error for richer messages.

### AST Types

```rust
pub struct BridgeFile {
    pub services: Vec<Service>,
}

pub struct Service {
    pub name: String,
    pub auth: Auth,                // default: Auth::None
    pub middleware: Vec<String>,   // hook names, e.g. ["log", "reject:403:blocked"]
    pub endpoints: Vec<Endpoint>,
}

pub struct Endpoint {
    pub name: String,
    pub method: Method,
    pub path: String,              // e.g. "/users/:id"
    pub path_params: Vec<String>,  // e.g. ["id"]
    pub auth: Auth,                // inherits from service if None
    pub tags: Vec<String>,
}

pub enum Method { Get, Post, Put, Patch, Delete, Head, Options }
pub enum Auth   { None, Bearer, ApiKey }
```

### `ParseError`

```rust
pub struct ParseError {
    pub line:    usize,    // 1-based line number
    pub column:  usize,    // 1-based column
    pub message: String,   // human-readable description
    pub snippet: String,   // source line text
    pub hint:    Option<String>, // suggested fix
}

impl ParseError {
    /// Format with colour and context, Rust-compiler style.
    pub fn display(&self, filename: &str) -> String { ... }
}
```

### Error examples

```
error[E0001]: unknown HTTP method: FETCH
  --> api.bridge:3:14
   |
 3 | endpoint list FETCH /users
   |               ^^^^^ unknown method
   |
   = hint: valid methods are GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS

error[E0004]: duplicate endpoint name 'list'
  --> api.bridge:5:10
   |
 5 | endpoint list GET /v2/users
   |          ^^^^ already defined on line 2
```

## Validation Rules

| Code | Rule |
|------|------|
| E0001 | Unknown HTTP method |
| E0002 | Path must start with `/` |
| E0003 | Duplicate service name |
| E0004 | Duplicate endpoint name within a service |
| E0005 | Unknown auth scheme |
| E0006 | Route conflict (same method + resolved path) |
| E0007 | `endpoint` declared outside a `service` block |
| E0008 | `auth` / `middleware` / `tags` declared outside a `service` block |
| E0009 | Service has no endpoints |
| E0010 | Missing service name |

## Design Notes

- Zero external dependencies — pure `std`.
- Single-pass recursive-descent parser; O(n) in source length.
- Errors are collected rather than short-circuiting, so you get all problems in one parse.
- Path parameter extraction (`/users/:id` → `["id"]`) is done during parsing so codegen doesn't need to re-parse paths.
