# Integration Guides

Patterns for wiring Bridge into external services. Each guide works
against the daemon's local HTTP surface (`127.0.0.1:8787`) with no
cloud dependency.

## Better Auth–style Session Validation

Bridge's auth registry issues JWT sessions and opaque tokens. To adopt
a Better-Auth-style flow, validate the session server-side on every
request and treat the token as bearer-only:

1. Client presents `Authorization: Bearer <opaque-token>`.
2. The service resolves the token to a session via the auth pipeline
   (see the auth endpoints in [api-reference](./api-reference.md)).
3. Expired/revoked sessions return `401`; the client re-authenticates.

For tests, skip real verification entirely by mocking the principal:

```bash
curl -X POST localhost:8787/api/v1/testing/mocks/auth \
  -d '{"principal":"u_integration_test"}'
```

(Encore parity: commits 1737, 1819 — auth mocking docs and override.)

## Resend-style Transactional Email

Model an email provider as a Bridge service with a canned-response mock
in tests:

```
service email
endpoint send POST /email/send
```

```bash
# Register the provider endpoint for discovery
curl -X POST localhost:8787/api/v1/infra/services \
  -d '{"name":"email","addr":"127.0.0.1:9100"}'

# In tests, stub it so no real mail leaves the machine
curl -X POST localhost:8787/api/v1/testing/mocks/services \
  -d '{"service":"email","response":{"id":"em_123","delivered":true}}'
```

The mock's response body is stored verbatim and echoed in
`GET /api/v1/testing`, so assertions run without network I/O.

## Polar-style Webhooks over Pub/Sub

Inbound webhooks map naturally onto a topic with one subscription per
handler; the DLQ captures signature-failures for later re-drive:

```bash
curl -X POST localhost:8787/api/v1/pubsub/topics -d '{"name":"polar","dlq":true}'
curl -X POST localhost:8787/api/v1/pubsub/subscriptions \
  -d '{"topic":"polar","name":"grant-benefits","dlq":true}'
```

Your webhook receiver authenticates the payload, then publishes:

```bash
curl -X POST localhost:8787/api/v1/pubsub/publish \
  -d '{"topic":"polar","message":{"type":"subscription.updated","data":{}}}'
```

Consumers pull via `/api/v1/pubsub/pull` and ack/nack per message;
messages that exhaust retries land in `/api/v1/pubsub/dlq/grant-benefits`.

## NestJS-style Config Injection

Expose runtime configuration to services through the infra config
surface instead of process env only — values are hot-readable and
sorted deterministically:

```bash
curl -X POST localhost:8787/api/v1/infra/env \
  -d '{"name":"EMAIL_FROM","value":"noreply@example.com"}'
```

Services read the full snapshot from `GET /api/v1/infra`. Setting a
variable to `""` removes it — handy for per-test overrides.

## Logto-style Identity Provider

Register the IdP as a discovered service and front it with middleware:

```bash
curl -X POST localhost:8787/api/v1/infra/services \
  -d '{"name":"logto","addr":"127.0.0.1:3001"}'
```

Issue app sessions through the auth registry, keeping the IdP's tokens
opaque to services: services see Bridge sessions only, so swapping IdPs
never touches handler code.

## Prisma-style Database Workflows

Map Prisma's dev workflow onto the sqldb + testing surfaces:

| Prisma command | Bridge equivalent |
|----------------|-------------------|
| `prisma migrate dev` | `bridge db-migrate schema.sql` against a `superuser` test database |
| `prisma studio` | Dev dashboard + `GET /api/v1/sqldb/*` |
| Ephemeral test DBs | `POST /api/v1/testing/databases` (unique namespaces) |
| Tear down | `DELETE /api/v1/testing/databases` |

See [tutorials](./tutorials.md#writing-tests-against-your-app) for the
full test-database lifecycle.
