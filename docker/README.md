# Docker Configuration

Docker-related configuration files for Bridge Framework infrastructure.

## Files

### init-db.sql

PostgreSQL initialization script that runs automatically when the database container starts for the first time.

**Contents:**
- Creates `bridge_app` schema
- Creates sample `users` table
- Inserts sample data
- Sets up permissions

**Usage:**

This file is automatically mounted and executed by the PostgreSQL container defined in `docker-compose.yml`:

```yaml
volumes:
  - ./docker/init-db.sql:/docker-entrypoint-initdb.d/init.sql
```

**When it runs:**
- First time container starts
- When container is recreated with fresh volumes

**To re-run the script:**

```bash
# Reset the database completely
npm run docker:reset

# Or manually
docker-compose down -v
docker-compose up -d
```

## Docker Compose

The main `docker-compose.yml` is in the root directory and uses files from this directory.

**Services defined:**
- **postgres** — PostgreSQL 16 database (uses init-db.sql)
- **redis** — Redis 7 cache (alternative to embedded miniredis)
- **pgadmin** — Database management UI

**Quick commands:**

```bash
# Start all services
npm run docker:up
# or
docker-compose up -d

# Stop services
npm run docker:down

# View logs
npm run docker:logs

# Reset (remove volumes and restart)
npm run docker:reset
```

## Customizing init-db.sql

You can modify `init-db.sql` to:
- Create additional schemas
- Add more tables
- Insert different seed data
- Configure database settings

**Example additions:**

```sql
-- Add a new table
CREATE TABLE IF NOT EXISTS bridge_app.posts (
    id SERIAL PRIMARY KEY,
    user_id INTEGER REFERENCES bridge_app.users(id),
    title TEXT NOT NULL,
    content TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Add indexes
CREATE INDEX idx_posts_user_id ON bridge_app.posts(user_id);

-- Insert sample data
INSERT INTO bridge_app.posts (user_id, title, content) VALUES
    (1, 'First Post', 'Hello from Alice!'),
    (2, 'Welcome', 'Hi everyone, I''m Bob')
ON CONFLICT DO NOTHING;
```

## Connection Details

When using Docker Compose:

**PostgreSQL:**
```
Host: localhost
Port: 5432
Database: bridge_dev
Username: bridge
Password: bridge
```

**Redis:**
```
Host: localhost
Port: 6379
No password
```

**pgAdmin:**
```
URL: http://localhost:5050
Email: admin@bridge.local
Password: bridge
```

### Connecting from Code

**PostgreSQL:**
```bash
DATABASE_URL=postgres://bridge:bridge@localhost:5432/bridge_dev
```

**Redis:**
```bash
REDIS_URL=redis://localhost:6379
```

## Volumes

Docker Compose creates persistent volumes:

- `postgres_data` — Database files
- `redis_data` — Redis persistence (if enabled)
- `pgadmin_data` — pgAdmin settings

**To completely reset:**

```bash
docker-compose down -v  # -v removes volumes
docker-compose up -d
```

## Network

All services are on the `bridge_network` Docker network, allowing them to communicate.

## Alternative: Daemon-Managed PostgreSQL

Bridge daemon can also manage PostgreSQL containers directly via CLI:

```bash
bridge db-create mydb
bridge db-status
bridge db-migrate schema.sql
bridge db-destroy mydb
```

This is an alternative to Docker Compose for database management.

## Troubleshooting

### Port conflicts

If ports 5432, 6379, or 5050 are already in use:

1. Edit `docker-compose.yml`
2. Change port mappings:
   ```yaml
   ports:
     - "5433:5432"  # Use 5433 on host
   ```

### Init script not running

The init script only runs on first container creation. To re-run:

```bash
docker-compose down -v  # Remove volumes
docker-compose up -d    # Recreate with fresh volumes
```

### Cannot connect to database

1. Check container is running: `docker ps`
2. Check logs: `docker-compose logs postgres`
3. Verify port is open: `netstat -an | grep 5432`
4. Test connection: `psql -h localhost -U bridge -d bridge_dev`

## See Also

- [Database Guide](../docs/database.md) — Bridge database management
- [Deployment Guide](../docs/deployment.md) — Production deployment
- Root `docker-compose.yml` — Service definitions

---

**Note:** For embedded miniredis (no Docker needed), just run the daemon normally. It starts automatically on port 6399.
