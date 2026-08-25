# Security & Compliance

Security posture, permission model, and database role management for
Bridge deployments (Encore commits 2145-2191 parity).

## Threat Model & Best Practices

Bridge's daemon is a **local development control plane**. Its security
boundary is the machine it runs on; do not expose `:8787` (HTTP) or
`:7878` (TCP) to untrusted networks.

- **Secrets never appear in listings** — the secrets registry redacts by
  default; plaintext is revealed only via explicit `reveal: true` and
  resolution failures return `409`, not the value.
- **Auth is deny-by-default** — services validate bearer tokens through
  the auth pipeline; tests should mock principals rather than disable
  verification in production code paths.
- **Deploy transitions are state-machine enforced** — CI cannot skip
  stages or un-retire failed revisions; out-of-order pipelines fail with
  `400` before any damage.
- **Validation schemas** gate request bodies per `METHOD:/path` — declare
  them for every public endpoint.

### Checklist before sharing a deployment

1. No inline secrets registered (`kind: "inline"` is dev-only); prefer
   `env` or `file` sources.
2. Test-mode mocks cleared (`DELETE /api/v1/testing/mocks`) — a lingering
   auth mock bypasses verification.
3. Test databases torn down (`DELETE /api/v1/testing/databases`).
4. Traces reviewed for accidental sensitive payloads (`GET /api/v1/traces`).

## Cloud Permissions (IAM Scopes)

When deploying to a cloud target, scope credentials to the minimum the
deploy registry actually needs:

| Scope | Purpose |
|-------|---------|
| image push | Push built images to the registry (`/api/v1/deploy/dockerfile` output builds) |
| deploy write | Create/update deployments on the target platform |
| secrets read | Resolve `env`-sourced secrets at boot on the target |
| logs write | Ship service logs to the platform |

Self-hosted targets need only registry + SSH-level access; no cloud IAM
is involved. GCP-style fine-grained scopes (commit 2162) map onto the
same four rows — grant at project level, never org level.

## Database Roles

The daemon models Encore's role split (commits 2145, 2150-2154):

| Role | Granted via | Capabilities |
|------|-------------|--------------|
| app runtime | `db-create` default | DML on schema-owned tables |
| migrator | `superuser: true` test DB | DDL: create/alter/drop during migrations |
| admin | host-level | Role management itself (not exposed over HTTP) |

Rules of thumb:

- Application endpoints run as the app role; they cannot DDL.
- Migrations run against `superuser` databases only:
  ```bash
  curl -X POST localhost:8787/api/v1/testing/databases \
    -d '{"name":"mig_target","superuser":true}'
  ```
- The superuser flag is scoped to the provisioned namespace and dies with
  its teardown — there is no standing superuser credential.

## SOC 2 Notes

For teams mapping Bridge usage onto SOC 2 controls:

- **CC6.1 (logical access)**: auth registry sessions + opaque tokens;
  revocation is immediate.
- **CC7.2 (monitoring)**: traces, metrics, and logs are first-class API
  surfaces — wire alerts onto `/api/v1/metrics`.
- **CC8.1 (change management)**: deploy registry provides an auditable
  revision history with supersede tracking and deterministic rollback.
- Encryption in transit / at rest are inherited from the deployment
  target; the daemon records TLS status (`POST /api/v1/infra/tls`) but
  does not terminate TLS itself.
