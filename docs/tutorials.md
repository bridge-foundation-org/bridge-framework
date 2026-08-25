# Tutorials

End-to-end walkthroughs for building, testing, and shipping a Bridge
service. Each tutorial is self-contained; start with the REST API one
if you are new.

## 1. Defining a Service

Create `hello.bridge`:

```
service hello
endpoint ping GET /ping
endpoint echo POST /echo
```

## 2. Generating a TypeScript Client

```bash
bridge compile-file hello.bridge > client.ts
```

## 3. Using the Dev Dashboard

1. `cargo run -p daemon`
2. `cd frontend && npm run dev`
3. Open `http://localhost:5173`
4. Paste Bridge source, click "Compile + Codegen"

## 4. Setting Up a Database

```bash
bridge db-create myapp
bridge db-migrate schema.sql
bridge db-status
bridge db-destroy myapp
```

## 5. Using the Redis Cache

```bash
redis-cli -p 6399
SET session:abc123 '{"user":"alice"}' EX 3600
GET session:abc123
```

## 6. Running Tests

```bash
cargo test --workspace
cargo test -p e2e-tests
cd frontend && npm run build
```

---

## Tutorial: Building a REST API with Pub/Sub and Caching

A complete vertical slice — service definition, HTTP surface, eventing,
caching, and verification — using only the daemon's HTTP API.

### Step 1 — Declare the service

`orders.bridge`:

```
service orders
endpoint create POST /orders
endpoint get GET /orders/:id
topic order-events
```

### Step 2 — Start the daemon

```bash
cargo run -p daemon
# TCP :7878 (compile protocol) · HTTP :8787 (control plane)
```

### Step 3 — Register a cache keyspace for hot reads

```bash
curl -X POST localhost:8787/api/v1/cache/keyspaces \
  -d '{"name":"order-summary","ttl_ms":60000,"max_entries":1000}'
```

The keyspace evicts LRU entries past `max_entries`; TTLs are honored on
read. Reads/writes go through the `/api/v1/cache/keyspaces/order-summary/*`
endpoints (see [api-reference](./api-reference.md#caching)).

### Step 4 — Wire a pub/sub topic with a DLQ

```bash
curl -X POST localhost:8787/api/v1/pubsub/topics \
  -d '{"name":"order-events","dlq":true}'
curl -X POST localhost:8787/api/v1/pubsub/subscriptions \
  -d '{"topic":"order-events","name":"billing","dlq":true}'
```

Subscribe idempotently from any number of consumers:

```bash
curl -X POST localhost:8787/api/v1/pubsub/subscribe \
  -d '{"topic":"order-events","subscription":"billing","subscriber":"worker-1"}'
```

Publishing returns the delivery fan-out count; a poison message that
exhausts retries lands in the subscription's DLQ
(`/api/v1/pubsub/dlq/billing`) for inspection and re-drive.

### Step 5 — Verify the whole flow

```bash
# Publish an event
curl -X POST localhost:8787/api/v1/pubsub/publish \
  -d '{"topic":"order-events","message":{"order_id":"o_42"}}'

# Pull it as the billing subscriber
curl "localhost:8787/api/v1/pubsub/pull?subscription=billing&subscriber=worker-1"

# Cache the rendered summary
curl -X POST localhost:8787/api/v1/cache/keyspaces/order-summary/entries \
  -d '{"key":"o_42","value":"{\"total\":1990}"}'
```

Every call above appears in `GET /api/v1/traces` with status and latency.

---

## Tutorial: Writing Tests Against Your App

Use the daemon's [testing surface](./api-reference.md#testing) to get
Encore-style test primitives without a live cloud.

```bash
# Enter test mode — quiet logs by default
curl -X POST localhost:8787/api/v1/testing/mode/enter -d '{"log_level":"warn"}'

# Provision an isolated database namespace (superuser for migrations)
curl -X POST localhost:8787/api/v1/testing/databases \
  -d '{"name":"users","superuser":true}'
# → {"namespace":"t1_users","superuser":true}

# Mock auth so handlers see a fixed principal
curl -X POST localhost:8787/api/v1/testing/mocks/auth -d '{"principal":"u_test"}'

# ... run your scenario ...

# Tear everything down
curl -X DELETE localhost:8787/api/v1/testing/databases   # destroy test DBs
curl -X DELETE localhost:8787/api/v1/testing/mocks       # clear mocks
curl -X POST localhost:8787/api/v1/testing/mode/exit     # leave test mode
```

Each provisioned database gets a unique namespace (`t{seq}_{name}`), so
parallel test runs never collide even with the same base name.

---

## Tutorial: Deploying to Production

The [deploy registry](./api-reference.md#deployments) models the full
lifecycle locally, including rollback.

```bash
# Create a deployment for a target
curl -X POST localhost:8787/api/v1/deploy \
  -d '{"target":"production","platform":"linux/arm64","revision":"git-abc123"}'

# Walk it through the pipeline
curl -X POST localhost:8787/api/v1/deploy/status -d '{"id":"dep-1","status":"building"}'
curl -X POST localhost:8787/api/v1/deploy/status -d '{"id":"dep-1","status":"deploying"}'
curl -X POST localhost:8787/api/v1/deploy/status -d '{"id":"dep-1","status":"deployed"}'

# Inspect the generated multi-platform Dockerfile
curl localhost:8787/api/v1/deploy/dockerfile

# If v2 misbehaves: roll back to exactly the revision v2 displaced
curl -X POST localhost:8787/api/v1/deploy/rollback -d '{"target":"production"}'
```

Illegal transitions (skipping stages, un-retiring terminals) are rejected
with `400`, so CI pipelines fail fast on out-of-order steps.
