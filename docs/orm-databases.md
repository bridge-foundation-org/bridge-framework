# ORM & Database Workflows

How common ORMs and database tools map onto Bridge's database surface
(`bridge db-*` CLI commands plus the daemon's `/api/v1/testing/*` and
`/api/v1/sqldb/*` endpoints). Bridge is Rust-first; these guides cover
interop scenarios where an existing toolchain must keep working.

## Prisma-style Migration Workflow

Prisma's dev loop maps cleanly onto the test-database + migrate flow
(Encore commits 1608, 1874):

| Prisma | Bridge equivalent |
|--------|-------------------|
| `prisma migrate dev` | `bridge db-migrate schema.sql` against a fresh superuser DB |
| `prisma migrate reset` | `DELETE /api/v1/testing/databases` then re-provision |
| `prisma studio` | Dev dashboard + sqldb query endpoints |
| Shadow database | `POST /api/v1/testing/databases {"name":"shadow","superuser":true}` |

The shadow-database pattern is safe because every provisioned test
database gets its own namespace (`t{seq}_{name}`) — migrations can be
rehearsed against identical schema without touching shared state.

### Deployment

Encore's Prisma deployment guide (1874) translates to:

1. Apply migrations in CI before the deploy status machine leaves
   `building` (`POST /api/v1/deploy/status`).
2. Only mark `deployed` after migrations succeeded — a failed migration
   should transition to `failed`, never skip ahead.
3. Use the rollback endpoint to return traffic to the previous revision;
   re-run the old migrations only if the new ones are destructive.

## Drizzle-style V1 Migrations

Drizzle v1 (commit 2010) introduced declarative, folder-based
migrations. Equivalent discipline with plain SQL files:

```
migrations/
  0001_init.sql
  0002_add_orders.sql
```

```bash
bridge db-create myapp
for f in migrations/*.sql; do bridge db-migrate "$f"; done
```

Keep files immutable once applied; append new numbered files instead of
editing history — the daemon does not track checksums, so replay safety
is your responsibility.

## TypeORM-style Patterns

TypeORM's entity-decorator style (commit 1604) has no direct Rust
equivalent; the transferable patterns are:

- **Repository-per-entity**: wrap each table's queries in one module,
  exposed through a Bridge service endpoint.
- **Transaction boundaries**: use the daemon transaction registry
  (`/api/v1/tx/*`) for multi-statement units of work; commit or roll
  back explicitly rather than relying on implicit autocommit.
- **Connection lifecycle**: in Bridge's model the daemon owns the
  connection pool; services stay stateless and address the database
  through the registry entry created at `db-create`.

## Testing Against Real Postgres

All database-backed tests should provision real, isolated instances
rather than mocks (the daemon's emulation keeps semantics honest):

```bash
# Per-suite setup
NS=$(curl -s -X POST localhost:8787/api/v1/testing/databases \
  -d '{"name":"repo_tests","superuser":true}' | sed 's/.*"namespace":"\([^"]*\)".*/\1/')

# ... run suite against $NS ...

# Teardown
curl -s -X DELETE localhost:8787/api/v1/testing/databases
```

See [tutorials](./tutorials.md#writing-tests-against-your-app) for the
full lifecycle including auth mocking.
