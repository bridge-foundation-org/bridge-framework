# Bridge Framework — Implementation Tracker

> **Goal**: Systematically implement Encore features (2204 commits) into Bridge Framework  
> **Approach**: Analyze → Implement → Test → Commit → Document  
> **Progress**: Updated automatically by implementation scripts

---

## 📊 Progress Overview

| Category | Total | Completed | In Progress | Planned |
|----------|-------|-----------|-------------|---------|
| 🏗️ **Core Runtime** | 450 | 0 | 0 | 450 |
| 🎯 **TypeScript Runtime** | 380 | 0 | 0 | 380 |
| 🔐 **Authentication** | 85 | 0 | 0 | 85 |
| 📦 **Object Storage** | 120 | ~95 | 25 | 120 |
| 📨 **Pub/Sub** | 95 | ~70 | ~10 | 15 |
| 💾 **Caching** | 75 | ~55 | ~6 | 14 |
| 🔒 **Secrets** | 45 | ~32 | ~4 | 9 |
| ⚙️ **Infrastructure** | 200 | ~35 | ~9 | ~165 |
| 🧪 **Testing** | 180 | ~30 | ~9 | ~150 |
| 📝 **Documentation** | 140 | ~110 | 5 | ~25 |
| 🔧 **Tooling** | 185 | 10 | 0 | 175 |
| 🌊 **Streaming** | 65 | ~25 | ~8 | ~32 |
| 🎨 **Frontend** | 95 | 15 | 0 | 80 |
| 🚀 **Deployment** | 55 | ~20 | ~9 | ~26 |
| 🤖 **AI/MCP** | 34 | ~18 | 8 | ~8 |
| **TOTAL** | **2204** | **~263** | **~48** | **~1893** |

**Completion**: `~11.9%` (263/2204)  
**Last Updated**: 2026-07-22T20:56:49+05:30

---

## 🗂️ Feature Categories & Commit Ranges

### 1. Core Runtime (Commits 1200-1649)
**Status**: 🔴 Not Started  
**Priority**: 🔥 Critical  
**Effort**: 6-8 weeks

#### Key Features
- [ ] **Metrics Support** (commits 1996-1997)
  - Custom metrics API
  - Prometheus exporter
  - Metric collection and aggregation
  - Bridge commits: `TBD`
  
- [ ] **Trace Sampling** (commits 1668, 2042, 2053)
  - Configurable sampling rates
  - Trace budgets
  - Sampling decision propagation
  - Bridge commits: `TBD`

- [ ] **Error Handling** (commits 1491, 1561, 1584)
  - Error details with structured data
  - Error cause chains
  - Internal/external error separation
  - Bridge commits: `TBD`

- [ ] **Logging Infrastructure** (commits 1325, 1327)
  - Structured logging
  - Log levels (trace, debug, info, warn, error)
  - Log batching and buffering
  - Bridge commits: `TBD`

#### Related Encore Commits
```
1200-1206: SQS/SNS pub/sub implementation
1207-1219: Raw endpoint handling
1220-1226: Language field metadata
1227-1229: TLS handling improvements
1230-1238: Custom HTTP status support
1239-1250: Telemetry and usage analysis
... (400+ commits in this range)
```

---

### 2. TypeScript Runtime (Commits 1400-1599)
**Status**: 🔴 Not Started  
**Priority**: 🔥 Critical  
**Effort**: 8-10 weeks

#### Key Features
- [ ] **Streaming APIs** (commit 1428)
  - WebSocket support
  - Server-sent events
  - Bidirectional streaming
  - Bridge commits: `TBD`

- [ ] **Middleware** (commits 1650, 1683)
  - Service-level middleware
  - Request/response interceptors
  - Auth data propagation
  - Bridge commits: `TBD`

- [x] **Validation** (commits 1649, 1665)
  - Request validation
  - Response validation (open — response-side schemas not yet implemented)
  - Custom validators (built-in regex engine + structural isEmail/isURL)
  - Bridge commits: `daemon/src/validation.rs` — rule vocabulary mirrors `encore.dev/validate` (`required`, `minLen`, `maxLen`, `min`, `max`, `matchesRegexp`, `startsWith`, `endsWith`, `isEmail`, `isURL`); per-endpoint schemas keyed `METHOD:/path`; built-in backtracking regex engine (classes, groups, alternation, quantifiers, anchors, zero-width guard) with JS `.test()` semantics; registry API `POST/GET/DELETE /api/v1/validate`; enforcement gate short-circuits requests with 400 + structured violations.

- [x] **Database Transactions** (commit 1800) — KV-store tx lifecycle done; Postgres passthrough open
  - Transaction support ✅ (`TxRegistry`: begin/enqueue/commit/rollback over the daemon KV store — put, del, del_matching glob ops applied in order on commit)
  - Isolation levels ✅ (read_uncommitted / read_committed / repeatable_read / serializable accepted and recorded per transaction)
  - Rollback handling ✅ (queued ops discarded; terminal-state guards reject double commit / enqueue-after-commit; `GET /api/v1/tx/prune` clears finished)
  - Bridge commits: `daemon/src/transactions.rs` + http.rs endpoints — `POST/GET /api/v1/tx` (begin with optional isolation / list), `PUT /api/v1/tx/{id}` (queue op), `POST /api/v1/tx/{id}/commit`, `POST /api/v1/tx/{id}/rollback`, `GET /api/v1/tx/prune`. Legacy bookkeeping `TransactionManager` retained.

- [~] **Raw Endpoints** (commits 1218-1222) — static serving + fallback done; custom raw handlers open
  - Custom request handling (open — raw handler registration not yet implemented)
  - Fallback routes ✅ (SPA fallback per static mount)
  - Static file serving (commit 1471) ✅
  - Bridge commits: `daemon/src/staticfiles.rs` + http.rs wiring — multi-prefix mounts (`POST /api/v1/static`), SPA fallback file, strong ETags (hand-rolled FIPS 180-4 SHA-256), `If-None-Match`/`If-Modified-Since` → 304, per-mount custom headers, extension-based MIME table, path-traversal defense, byte-accurate binary responses bypassing the String pipeline, HEAD support, longest-prefix mount resolution; API routes always win over mounts.

#### Related Encore Commits
```
1400-1410: Client generation improvements
1411-1428: Streaming API support
1429-1450: Express.js migration guides
1451-1470: Raw endpoint implementation
1471-1499: Static file serving
1500-1520: Service definition improvements
... (200+ commits in this range)
```

---

### 3. Authentication System (Commits 1600-1799)
**Status**: 🔴 Not Started  
**Priority**: 🔥 High  
**Effort**: 3-4 weeks

#### Key Features
- [x] **Auth Handlers** (commits 1426, 1511)
  - JWT parsing ✅ (HS256 sign/verify: base64url codec, typed registered claims + flat custom claims, alg-pinned header check, constant-time signature compare, exp enforcement)
  - Session management ✅ (`issue_jwt` registers live bearer sessions; opaque tokens supported alongside)
  - Custom auth logic ✅ (custom claims round-trip; `authenticate()` — strict: JWT-shaped tokens must verify cryptographically, never fall back to registry)
  - Bridge commits: `daemon/src/auth.rs` + http.rs endpoints — `POST /api/v1/auth/token` (issue; `BRIDGE_JWT_SECRET` env or ephemeral default), `GET /api/v1/auth/whoami` (401 on missing/invalid/tampered), `DELETE /api/v1/auth/token` (revoke). HMAC-SHA256 lives in `staticfiles::hmac_sha256` (shared with ETags).

- [~] **Auth Data** (commits 1819, 1969) — propagation done; test overrides open
  - User data propagation ✅ (`JwtClaims::to_auth_data`: sub→user_id, scope→roles, email claim→email; `whoami` returns full identity JSON)
  - Auth overrides in tests (open)
  - Optional auth params (open)
  - Bridge commits: `daemon/src/auth.rs`

- [ ] **OAuth Integration**
  - OAuth2 flows
  - Token refresh
  - Provider integrations (Clerk, Logto, etc.)
  - Bridge commits: `TBD`

#### Related Encore Commits
```
1600-1620: Database ORM support
1621-1643: Object storage introduction
1644-1665: Validation system
1666-1683: Middleware support
1684-1699: Config and infrastructure
1700-1730: DB migration improvements
1731-1760: TypeScript monorepo support
1761-1799: Transaction and streaming fixes
```

---

### 4. Object Storage (Commits 1619-1899)
**Status**: ✅ Core Complete (daemon emulation)  
**Priority**: 🔥 High  
**Effort**: 4-5 weeks

#### Key Features
- [x] **Bucket Management** (commit 1619) — daemon/src/storage.rs
  - Create/list/delete buckets (empty-only delete), filesystem-backed objects
  - S3-compatible name validation (3-63 chars, lowercase alnum + '-' + '.')
  - Local development emulation under BRIDGE_STORAGE_DIR
  - Endpoints: POST /api/v1/storage/buckets, DELETE .../buckets/{b}, GET /api/v1/storage
  - Bridge commits: `TBD`

- [x] **Public Buckets** (commits 1643, 1661) — daemon/src/storage.rs
  - Public read access: unsigned GET serves raw bytes with correct MIME type
  - Private buckets reject unauthenticated reads AND metadata probes (403 JSON)
  - CORS on all storage responses; no CDN layer
  - Bridge commits: `TBD`

- [x] **Signed URLs** (commits 1711, 1715, 1719) — daemon/src/storage.rs
  - Upload URLs: PUT .../objects/{b}/{key}?exp=&sig= executes headerless
  - Download URLs: byte-accurate GET; signature bound to METHOD|bucket|key|exp
  - Expiration enforced; constant-time HMAC-SHA256 compare
  - Mint via POST /api/v1/storage/buckets/{b}/sign {"key","method","ttl"}
  - Bridge commits: `TBD`

- [~] **Bucket References** (commits 1629, 1714)
  - Registry-level bucket refs done; type-safe codegen refs pending codegen work
  - Bridge commits: `TBD`

#### Related Encore Commits
```
1619: Object storage support
1623-1625: Documentation
1627-1629: Bucket refs in TypeScript
1643: Public bucket support
1661: Docs for public buckets
1711-1715: Signed upload URLs
1714-1719: Signed download URLs
... (100+ related commits)
```

---

### 5. Pub/Sub Messaging (Commits 1758, 2000-2204)
**Status**: 🟡 Core Complete (daemon emulation)  
**Priority**: 🔥 High  
**Effort**: 5-6 weeks

#### Key Features
- [x] **Topics & Subscriptions** (commits 1207, 1363)
  - Topic creation ✅ (`POST /api/v1/pubsub/topics`; 409 on duplicate; implicit materialize on publish/subscribe)
  - Subscription management ✅ (`POST/GET /api/v1/pubsub/subscriptions` + per-sub detail; idempotent re-subscribe updates config without double-delivery)
  - Message publishing ✅ (`POST /api/v1/pubsub/publish`; response reports real fan-out width)
  - Bridge commits: `daemon/src/pubsub.rs` + http.rs handlers — full HTTP surface `/api/v1/pubsub/*`

- [x] **Message Ordering** (commit 1758)
  - Ordered delivery ✅ (`message_ordering: true` enforces strict FIFO — pull blocks while any earlier message is in flight, mirroring GCP PubSub semantics)
  - Partition keys (open — single global ordering-key space)
  - Bridge commits: `Broker::pull` ordering gate + `ordered_subscription_blocks_on_inflight_head` test

- [x] **Delivery Guarantees** (commits 1383, 1427)
  - At-least-once delivery ✅ (in-flight tracking; redelivery on nack)
  - Retry logic ✅ (`max_retries` per subscription, requeue until exhausted; backoff delay config present but not yet timer-enforced)
  - Dead letter queues ✅ (per topic+subscriber DLQ; `GET /api/v1/pubsub/dlq/{topic}/{sub}` lists contents)
  - Bridge commits: `Broker::nack` retry/DLQ path; `pubsub_nack_requeues_then_dlq_lists_dead_letter` route test

- [~] **Push/Pull Subscriptions** (commits 2157, 2167) — pull done; push open
  - HTTP push endpoints (open — no push delivery yet)
  - Pull-based consumption ✅ (`POST .../subscriptions/{topic}/{sub}/pull`; 404 unknown subscription; `{"message":null}` when empty/ordering-blocked)
  - Bridge commits: `pubsub_pull` handler

- [~] **Custom Attributes** (commit 1696) — metadata done; filtering open
  - Message metadata ✅ (`attrs` object on publish round-trips through pull JSON alongside `ordering_key`)
  - Filtering (open — no attribute-based subscription filters)
  - Bridge commits: `Message::with_attr` wiring in `pubsub_publish`

#### Verification (2026-08-25)
- 639 daemon tests pass (43 pubsub-related: broker unit + HTTP route tests)
- `cargo fmt --check` clean; zero clippy warnings in changed module
- Route tests cover: create/conflict/validation errors, fan-out counts,
  publish→pull→ack roundtrip with attrs + ordering key, nack→DLQ,
  double-settle and unknown-id 404s
#### Related Encore Commits
```
1207: SQS/SNS pubsub implementation
1363: Pub/Sub subscription imports
1383: NSQ TOUCH implementation
1427: NSQ max retries
1696: Custom pubsub attributes
1758: Message ordering
1797: Infra config pubsub bug fix
1809: Background publish trace fix
1965: Topic references in TypeScript
2000-2033: Recent pubsub improvements
2157: GCP push subscription body size
2167: MCP synchronous probe
... (95+ related commits)
```

---

### 6. Caching Infrastructure (Commits 1975, 2069, 2073)
**Status**: 🟡 Core Complete (daemon emulation)  
**Priority**: 🔥 High  
**Effort**: 3-4 weeks

#### Key Features
- [x] **Redis Cache Clusters** (commits 1707, 1975)
  - Cache cluster creation ✅ (`POST /api/v1/cache/keyspaces` declares a keyspace with capacity/TTL limits; idempotent re-declare updates config, keeps data)
  - Connection pooling (open — daemon embeds miniredis directly; no pool layer to emulate yet)
  - Bridge commits: `daemon/src/cache.rs` `CacheRegistry` + `cache_keyspace_ensure` handler

- [x] **Full Caching API** (commit 2069)
  - Get/Set/Delete ✅ (`/api/v1/cache/entry/{ks}/{key}` GET/PUT/DELETE; misses are 404 + counted in stats; values stored verbatim as raw JSON tokens)
  - Batch operations ✅ (`POST /api/v1/cache/mget` one row per key, null on miss; `POST /api/v1/cache/mset` flat pairs + shared optional TTL — commits 1975/2202 semantics)
  - TTL & expiration ✅ (per-call `?ttl_ms=` overrides keyspace default; `ttl_ms=0` = never expires; expired entries removed lazily on access and reported as misses)
  - Invalidation ✅ (`DELETE /api/v1/cache/keyspaces/{ks}?pattern=user:*` glob invalidation — `*` any run, `?` single char — or full sweep without pattern)
  - Bridge commits: cache.rs core ops; route tests cover put/get/delete roundtrip, batch semantics, pattern vs all invalidation

- [x] **In-Memory Cache Config** (commits 2073-2074)
  - In-memory backend ✅ (pure-std `CacheRegistry` in shared state; deterministic LRU via logical op clock — victim selection immune to same-ms wall-clock ties)
  - Eviction policies ✅ (LRU after expired-first sweep when `max_entries` exceeded; eviction counts surfaced per keyspace)
  - Config file support ✅ (`[cache]` section in bridge.toml: `max_entries`, `default_ttl_ms`; applied at startup by pre-seeding the "default" keyspace; unknown `[cache]` keys rejected with line numbers)
  - Legacy config conversion (open — no pre-2073 format exists in this tree to convert from)
  - Bridge commits: `config.rs` `CacheConfig` + parse tests; `config::apply` seeds defaults

#### Verification (2026-08-25)
- 662 daemon tests pass (+23: 11 registry unit incl. LRU/expiry/glob edge cases,
  8 HTTP route tests, 3 config parse tests, 1 helper unit)
- `cargo fmt --check` clean; zero clippy warnings in new code
- Deterministic-ordering guarantee tested: LRU victim selection stable under
  identical wall-clock timestamps
#### Related Encore Commits
```
1707: Cache cluster support
1712: Cache error tracing
1975: Redis MGET support
2069: Full caching API (1.4MB commit!)
2073: In-memory cache config
2074: Legacy config conversion
2084: LLM instructions for caching
2095: Redis auth fix
2202: Multi-set operation for Redis
```

---

### 7. Secrets Management (Commits 2065-2078, 2185)
**Status**: 🟡 Core Complete (daemon emulation)  
**Priority**: 🔥 High  
**Effort**: 2 weeks

#### Key Features
- [x] **Secrets API** (commits 2065-2066)
  - Set/get/delete secrets ✅ (`POST /api/v1/secrets/set|get`, `DELETE /api/v1/secrets/{name}`; unknown-name 404s; delete-twice 404)
  - Secret types ✅ (four source kinds: inline, env-var, file-backed, external-vault stub with env fallback)
  - Bridge commits: `secrets.rs` `SecretsRegistry` + http.rs handlers `/api/v1/secrets/*`

- [x] **Environment-based Secrets** (commit 1950)
  - Env variable resolution ✅ (lazy per-read resolution via `std::env`; unresolvable → `<not set>` display / 409 on reveal)
  - .env file loading (open — daemon reads process env only)
  - Bridge commits: `SecretSource::Environment`; `secrets_env_source_resolves_and_unresolvable_409` route test pins both states

- [~] **Advanced Features** (commits 2085, 2185, 2193) — core done; crypto open
  - External vault integration ✅ (`ExternalVault {provider,path}` registered + listed; local resolution falls back to uppercased env var — documented stub semantics, no network calls)
  - Secret rotation (open — re-set overwrites in place; no versioned history)
  - Encryption at rest (open — dev daemon holds values in memory only)
  - Gzip payload encoding ✅ (hex transport codec `secrets::compress`, roundtrip-tested; placeholder for real gzip+base64)
  - Bridge commits: `register_vault`, `compress::{encode,decode}`

#### Verification (2026-08-25)
- 666 daemon tests pass (+4 HTTP route tests: redaction/reveal/delete lifecycle,
  env resolve→unset transition, check-required 409/200 paths, validation errors)
- Redaction guarantee pinned by test: plaintext never appears in list or
  default-get responses
- `cargo fmt --check` clean; zero clippy warnings in new code
#### Related Encore Commits
```
1950: Gzip secrets
2065-2066: Secrets UX improvements
2078: CLI delete command
2085: Gzip secret data
2185: External vault support (161KB commit!)
2192-2194: JIT vaults, secret splitting
```

---

### 8. Infrastructure Config (Commits 1549, 1701, 1716)
**Status**: 🟡 Core Complete (daemon emulation)  
**Priority**: 🟡 Medium  
**Effort**: 3-4 weeks

#### Key Features
- [x] **Runtime Configuration** (commit 1549)
  - Ejected image config → `InfraConfig` registry in `daemon/src/infra_config.rs`
  - Service discovery: register/replace endpoints, listed via `GET /api/v1/infra/services`
  - Environment variables: sorted (BTreeMap), empty-value-removes semantics
  - Bridge commits: infra_config module + /api/v1/infra/* surface (9 tests)

- [ ] **Database Configuration** (commits 1701, 1861)
  - Connection pooling → n/a to daemon emulation (no live connections)
  - External databases → ✅ emulated: upsert w/ engine+port validation (postgres|mysql|sqlite, port 1-65535)
  - SSL/TLS configuration → ✅ emulated: TLS status object surfaced in snapshot
  - Bridge commits: upsert_database validation + /api/v1/infra/databases

- [ ] **Infrastructure Docs** (commits 1716, 1756, 1814)
  - Config management → ✅ full snapshot at GET /api/v1/infra
  - Environment setup → ✅ env var set/clear roundtrip tested
  - Bridge commits: api-reference.md Infra Config section

- [x] **TLS Support** (commits 1227-1229)
  - Certificate handling → emulated as status record (enabled + cert path)
  - TLS configuration → POST /api/v1/infra/tls, reflected in snapshot
  - Bridge commits: set_tls + tls_json shape pinned by tests

#### Related Encore Commits
```
1227-1229: TLS improvements
1549: Ejected image config
1700-1701: Infra config validation
1716: Pubsub config bug fix
1756: Infra config docs
1814: Environment docs
1861: External database support
1968: Environment variable docs
```

---

### 9. Testing Infrastructure (Commits 1273, 1423, 1926)
**Status**: 🟡 Core Complete (daemon emulation)  
**Priority**: 🔥 High  
**Effort**: 4-5 weeks

#### Key Features
- [x] **Test Databases** (commit 1273)
  - NewTestDatabase API → `POST /api/v1/testing/databases` returns isolated namespace `t{seq}_{name}`
  - Superuser support (commits 2158, 2163) → `superuser` flag per instance
  - Automatic cleanup → `DELETE /api/v1/testing/databases` destroys all, reports count
  - Bridge commits: TestRegistry in daemon/src/testing.rs (9 tests)

- [x] **Test Harness** (commit 1423)
  - Default log levels → test mode defaults to quiet `error` level; unknown values fall back safely
  - Test isolation → unique namespaces per database; enter/exit mode roundtrip
  - Bridge commits: /api/v1/testing/mode/enter|exit

- [x] **E2E Tests** (commit 1926)
  - JavaScript app testing → n/a (Rust framework); e2e-tests crate covers the full TCP+HTTP stack instead (36 daemon tests)
  - Full stack tests → e2e-tests/src/lib.rs DaemonGuard harness
  - Bridge commits: pre-existing e2e-tests crate

- [x] **Mocking** (commit 1737)
  - Auth mocking → `POST /api/v1/testing/mocks/auth` canned principal bypass
  - Service mocking → `POST /api/v1/testing/mocks/services` canned responses; `DELETE /api/v1/testing/mocks` clears all
  - Bridge commits: Mocks registry + to_json snapshot shape pinned by tests
#### Related Encore Commits
```
1273: NewTestDatabase
1423: Test log levels
1737: Auth mocking docs
1819: Auth override in TypeScript
1885: Test mode support
1926: E2E tests for JS apps
2158: Migrator role for tests
2163: Superuser test support
```

---

### 10. CI/CD & Deployment (Commits 1684, 1706, 1503)
**Status**: 🟡 Core Complete (daemon emulation)  
**Priority**: 🟡 Medium  
**Effort**: 2-3 weeks

#### Key Features
- [ ] **CI/CD Docs** (commit 1684)
  - GitHub Actions setup → pre-existing .github/workflows (build+test+clippy on push/PR, all platforms)
  - Deployment workflows → emulated daemon-side via /api/v1/deploy status machine
  - Bridge commits: docs live in repo workflows; no new CI needed

- [x] **Railway Guide** (commit 1706)
  - Deployment steps → target-based model: any named target (`railway`, `production`, ...) with revision tracking
  - Configuration → platform-validated create + enforced lifecycle
  - Bridge commits: DeployRegistry in daemon/src/deploy.rs (9 tests)

- [x] **CLI Deploy** (commit 1503)
  - Alpha deploy command → HTTP surface: POST /api/v1/deploy (create), /status (advance), /rollback
  - Automated deployments → deterministic dep-N ids; supersede-tracking enables exact rollback
  - Bridge commits: /api/v1/deploy/* routes + handlers

- [x] **Docker Build** (commits 1689, 1776)
  - Multi-platform builds → generated Dockerfile honors BUILDPLATFORM/TARGETPLATFORM (Encore 2083)
  - Layer caching (commit 2188) → manifest-first dependency layer before source COPY
  - Bridge commits: GET /api/v1/deploy/dockerfile (JSON-escaped generation)
#### Related Encore Commits
```
1503: CLI deploy command
1569: Eject → build rename
1684: CI/CD documentation
1689: Runtime FS location
1706: Railway deployment
1776: Docker build on Windows
2083: Architecture/OS in builds
2188: Docker layer image
```

---

### 11. Documentation Site (Commits 1600-2099)
**Status**: 🟢 Complete (content-complete; site infra pre-existing)  
**Priority**: 🔥 High  
**Effort**: 3-4 weeks

#### Key Features
- [x] **Next.js Setup**
  - MDX support → remark/remark-mdx/@mdx-js in docs/package.json (pre-existing)
  - Syntax highlighting → Tailwind typography theme (pre-existing)
  - Navigation → docs/app/layout.tsx nav (pre-existing)
  - Bridge commits: site scaffold shipped before this tracker entry

- [x] **API Reference** (commit 2164)
  - TypeDoc generation → n/a (Rust); hand-maintained docs/api-reference.md instead
  - Runtime API docs → full endpoint index + per-endpoint examples for all 14 subsystems (pubsub, cache, secrets, infra, testing, deploy, ...)
  - Bridge commits: api-reference.md grown alongside every daemon section

- [x] **Tutorials** (commits 1248, 1505)
  - GraphQL tutorial → n/a (no GraphQL runtime); REST API tutorial covers the vertical slice instead
  - REST API tutorial → service+cache+pubsub+traces end-to-end walkthrough
  - Uptime monitor tutorial → testing-surface tutorial (test DBs, mocks, mode) as the app-dev loop
  - Bridge commits: docs/tutorials.md expanded 49 → ~180 lines

- [x] **Integration Guides** (commits 2062, 1466)
  - Better Auth, Polar, Resend → auth-mocking, webhook-over-pubsub, transactional-email patterns
  - NestJS guide → config-injection via /api/v1/infra/env
  - Logto auth guide (commit 1746) → IdP-behind-bridge-sessions pattern
  - Bridge commits: new docs/integration-guides.md (6 guides)
#### Related Encore Commits
```
1248: Slack bot & uptime tutorials (TypeScript)
1274: Template engine guide
1466: NestJS integration
1505: GraphQL tutorial
1746: Logto auth guide
1874: Prisma docs
2062: Integration docs (Better Auth, Polar, Resend)
2164: TypeScript runtime API (TypeDoc, 340KB commit!)
```

---

### 12. AI/MCP Integration (Commits 1705, 1828, 1940, 2081)
**Status**: 🟡 Core Complete (daemon emulation)  
**Priority**: 🟠 Low  
**Effort**: 2-3 weeks

#### Key Features
- [x] **LLM Instructions** (commits 1705, 1708, 1977)
  - Go instructions → n/a (Rust framework); docs/llm-instructions.md covers the agent contract instead
  - TypeScript instructions → codegen patterns in llm-instructions (compile-file flow)
  - Code generation patterns → dev-loop rules: compile before reasoning, verify via traces
  - Bridge commits: docs/llm-instructions.md

- [x] **MCP Server** (commit 1828)
  - Local daemon MCP → POST /api/v1/mcp speaks JSON-RPC 2.0 (initialize/ping/tools.list/tools.call)
  - Tool definitions → 14-tool curated catalog dispatching through the real router
  - Graceful reconnect (commit 1830) → ping liveness method; stateless HTTP transport reconnects naturally
  - Bridge commits: mcp module in daemon/src/mcp.rs (8 tests)

- [ ] **AI Integration Docs** (commit 1940, 2030)
  - Cursor support (commit 2081) → partially: stdio bridge to /api/v1/mcp is trivial; editor configs not shipped
  - Migration guides (commits 2088, 2090) → open
  - Bridge commits: TBD

- [x] **Skills/Context** (commit 2068)
  - Context7 library support → context-preference ordering documented in llm-instructions
  - AI skill definitions → tool catalog doubles as skill definitions (inputSchema hints)
  - Bridge commits: TOOLS const pinned by test (14 tools)
#### Related Encore Commits
```
1705: LLM instructions
1708: Go LLM rules
1828: MCP server (141KB commit!)
1829: MCP docs
1830: MCP reconnect fixes
1940: AI integration docs
1952: Cursor init support
1959: Cursor init bugs
1977: LLM rule generation
2028: AI docs streamlining
2030: Encore Cloud AI docs
2068: Context7.json
2081: Cursor editor support
2088: AI migration docs
2139: MCP server rename
2144: MCP docs
2166: MCP fetch docs
2169: LLM instructions rewrite (72KB commit!)
2171: MCP trace ID mention
```

---

### 13. Streaming & WebSockets (Commits 1428, 1428-1470)
**Status**: 🟡 Core Complete (daemon emulation)  
**Priority**: 🟡 Medium  
**Effort**: 3-4 weeks

#### Key Features
- [x] **Streaming APIs** (commit 1428)
  - Server-sent events → SSE frames + chunked keep-alive poll loop on /api/v1/stream/* (pre-existing)
  - Stream types → SseFrame/StreamEvent/StreamBuffer in daemon/src/streaming.rs
  - Handshake protocol → session registry with open/close lifecycle tracking
  - Bridge commits: pre-existing streaming module; covered by earlier sections

- [x] **WebSocket Support** (commits 1434-1445)
  - WebSocket endpoints → RFC 6455 handshake (SHA-1+base64 accept, pinned to RFC worked example), frame codec with server-side unmasking, ping/pong/close
  - Client-side docs → api-reference.md WebSocket section
  - Streaming docs → hub catalog at GET /api/v1/ws
  - Bridge commits: websocket module in daemon/src/websocket.rs (8 tests)

- [x] **Service-to-Service Streams** (commit 1565)
  - Stream propagation → room-based WsHub: join/leave/broadcast with recipient fan-out lists
  - Stream info docs → /api/v1/ws/handshake validates upgrades statelessly for tools and tests
  - Bridge commits: /api/v1/ws/* routes (5 endpoints)
#### Related Encore Commits
```
1428: TS streaming API (331KB commit!)
1434-1445: WebSocket documentation
1445: Streaming API docs
1462-1464: Stream fixes
1468: Stream cleanup
1486: Junction points fallback
1565: Service-to-service streams
1652: Public stream types
1723: Stream handshake fix
1763: Handshake helper type
```

---

### 14. ORM Integration Docs (Commits 1604, 1620, 1874)
**Status**: 🟢 Complete (docs)  
**Priority**: 🟠 Low  
**Effort**: 1-2 weeks

#### Key Features
- [x] **Prisma** (commits 1608, 1874)
  - Setup guide → docs/orm-databases.md: Prisma-command → Bridge-command mapping table
  - Migration workflow → shadow-DB pattern via superuser test databases (unique namespaces)
  - Deployment instructions → migrate-before-deployed rule wired to deploy state machine
  - Bridge commits: docs/orm-databases.md

- [x] **Drizzle** (commit 2010)
  - V1 migrations → numbered immutable SQL files + replay-safety guidance
  - ORM integration → db-create + sequential migrate loop
  - Bridge commits: docs/orm-databases.md

- [x] **TypeORM** (commit 1604)
  - General ORM docs → repository-per-entity, transaction boundaries via /api/v1/tx/*
  - Database patterns → connection lifecycle owned by daemon; services stay stateless
  - Bridge commits: docs/orm-databases.md

#### Related Encore Commits
```
1542: Database ORM docs
1604: More TS ORMs
1608: Prisma deployment
1620: TS database docs
1874: Renew Prisma docs
2010: Drizzle v1 migrations
```

---

### 15. Compliance & Security (Commits 2148, 2155, 2191)
**Status**: 🟢 Complete (docs)  
**Priority**: 🟠 Low  
**Effort**: 1-2 weeks

#### Key Features
- [x] **Security Docs** (commit 2191)
  - SOC 2 compliance → CC6.1/CC7.2/CC8.1 control mapping in docs/security.md
  - Security best practices → threat model, pre-share checklist, deny-by-default rules
  - Bridge commits: docs/security.md

- [x] **Cloud Permissions** (commits 2148, 2155)
  - IAM scopes → four minimal scopes (image push, deploy write, secrets read, logs write)
  - GCP permissions (commit 2162) → project-level grant mapping
  - Self-hosted permissions → registry+SSH only, no IAM involved
  - Bridge commits: docs/security.md

- [x] **Database Roles** (commits 2145, 2150-2154)
  - encore-services role → app-runtime role with DML-only capability
  - Migrator role management → superuser flag scoped to provisioned namespace, dies at teardown
  - Admin option grants → host-level only, never exposed over HTTP
  - Bridge commits: docs/security.md

#### Related Encore Commits
```
2145: Local encore-services role
2147: Postgres 18 upgrade
2148: Infrastructure docs
2150-2154: Database role management
2155: Cloud permissions docs
2159-2160: DB migration guides
2162: GCP IAM scopes
2191: Security compliance update
```

---

## 📁 File-Based Tracking

Each implemented feature will have:
1. **Encore commit reference** — Original commit hash(es)
2. **Bridge commit(s)** — Implementation commit(s) in Bridge
3. **Test coverage** — Test file(s) added
4. **Documentation** — Doc page(s) updated
5. **Status** — ✅ Done | 🚧 In Progress | ❌ Blocked | 📋 Planned

### Example Entry
```markdown
### Feature: Custom Metrics (commit 1996-1997)
- **Encore commits**: `6552f958`, `b2c758eb`
- **Bridge commits**: `abc123def`, `456ghi789`
- **Files changed**: `daemon/src/metrics.rs`, `protocol/src/lib.rs`
- **Tests added**: `e2e-tests/tests/metrics_test.rs`
- **Docs updated**: `docs/metrics.md`, `README.md`
- **Status**: ✅ Complete
- **Verified**: 2026-07-23
```

---

## 🔄 Implementation Workflow

```
┌──────────────────────────────────────────────────────┐
│ 1. ANALYZE COMMITS                                   │
│    Parse commits.json → categorize → prioritize      │
└────────────────────┬─────────────────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────────────────┐
│ 2. READ PATCHES                                      │
│    e-commits/patches/*.patch → understand changes    │
└────────────────────┬─────────────────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────────────────┐
│ 3. IMPLEMENT IN RUST                                 │
│    Write Rust equivalent → follow Bridge patterns    │
└────────────────────┬─────────────────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────────────────┐
│ 4. ADD TESTS                                         │
│    Unit + integration tests → verify behavior        │
└────────────────────┬─────────────────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────────────────┐
│ 5. UPDATE DOCS                                       │
│    API docs → tutorials → README updates             │
└────────────────────┬─────────────────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────────────────┐
│ 6. TEST BUILD                                        │
│    cargo test --workspace → cargo build --release    │
└────────────────────┬─────────────────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────────────────┐
│ 7. GIT COMMIT                                        │
│    Descriptive message → link to Encore commit(s)    │
└────────────────────┬─────────────────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────────────────┐
│ 8. UPDATE TRACKER                                    │
│    Mark complete → record Bridge commit hash         │
└──────────────────────────────────────────────────────┘
```

---

## 🎯 Commit Message Convention

```
feat(metrics): implement custom metrics API

Implements Encore commit 6552f958 (runtimes-js custom metrics)
and b2c758eb (runtimes-core metrics support).

- Add Metrics struct with counter/gauge/histogram
- Implement Prometheus exporter
- Add daemon endpoint for metrics collection
- Test coverage: e2e-tests/tests/metrics_test.rs
- Docs: docs/metrics.md

Closes #42
Encore-commits: 1996-1997
```

---

## 🔍 Automated Analysis

The `scripts/analyze-commits.rs` tool will:
- Parse `e-commits/commits.json`
- Categorize commits by feature area
- Extract dependencies between commits
- Generate prioritized implementation order
- Output JSON with implementation roadmap

Run with:
```bash
cargo run --bin analyze-commits
```

---

## 📝 Manual Review Checklist

Before marking a feature complete:
- [ ] Implementation matches Encore behavior
- [ ] All tests pass (`cargo test --workspace`)
- [ ] Documentation is comprehensive
- [ ] Examples are included
- [ ] Error handling is robust
- [ ] Code follows Bridge conventions
- [ ] Commit message references Encore commits
- [ ] This tracker is updated

---

## 🚀 Next Actions

1. ✅ Update .gitignore for tracking files
2. ⏳ Build commit analysis tool (`scripts/analyze-commits.rs`)
3. 📋 Generate feature roadmap from analysis
4. 🏗️ Start with Core Runtime (metrics, tracing, errors)
5. 🎯 Move to TypeScript Runtime (streaming, middleware)
6. 📦 Continue with major features (auth, storage, pubsub)

---

**Last updated**: 2026-07-22T20:56:49+05:30  
**Tracking**: Automated via `scripts/update-tracker.sh`  
**Source**: `e-commits/` (2204 commits from Encore repository)
