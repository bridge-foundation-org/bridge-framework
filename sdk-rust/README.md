# bridge-sdk-rust

Rust SDK for BridgeBase — declare infrastructure in code, emit the canonical
`bridgebase.lock.json`.

## What it is

The builder API from ADR-0002: `ManifestBuilder` collects buckets, topics,
databases, crons, secrets, and services, then stamps a SHA-256
`content_hash` over the canonical serialization at `finalize()`. The output
is the lock file every deploy target consumes (ADR-0001) — the schema owner
is the `infra-manifest` crate in `bridgebase-cli`; this SDK mirrors its
serialization exactly.

## Why it exists

Goal §9 requires language parity: whatever TS apps can declare, Rust apps
must declare, producing the *same* manifest so the CLI and orchestrators
never special-case by language. Proc-macro declarations (attribute style)
are planned as an additional layer on top of this same output path; the
builder stays first-class for config-driven names, tests, and codegen'd
apps.

## Parity guarantee

`sdk-rust/tests/fixtures/sample.lock.json` is the golden artifact emitted by
the schema crate. Both this SDK (`lock_text_matches_canonical_fixture_bytes`)
and the TS emitter (`bridge-framework/sdk-ts`) must reproduce it
byte-for-byte — same JSON text, same hash. If an SDK diverges, tests fail
before any deploy target sees a mismatched lock.

## Usage

```rust
use bridge_sdk_rust::{ManifestBuilder, ServiceSpec};

let lock = ManifestBuilder::new("demo")?
    .language("rust")
    .bucket("media", false)?
    .topic("events", None)?
    .database("main")?
    .service(
        "api",
        ServiceSpec::new("registry.example/demo-api:1.0.0")
            .port(8080, 8080)
            .env("RUST_LOG", "info")
            .bucket("media")
            .topic("events")
            .database("main"),
    )?
    .finalize()?;

println!("{}", lock.to_lock());
lock.verify()?; // re-checks the stamped content_hash
```

## Develop

```sh
cargo test -p bridge-sdk-rust   # unit + doc + parity tests
cargo clippy -p bridge-sdk-rust -- -D warnings
```
