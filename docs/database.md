# Database Management

## Overview

Bridge manages PostgreSQL databases via Docker containers. The daemon wraps `docker` CLI commands to create, inspect, migrate, and destroy Postgres containers.

## Prerequisites

- Docker must be installed and running on the host
- The daemon gracefully handles missing Docker with clear error messages

## CLI Commands

```bash
bridge db-create myapp       # Create a Postgres container
bridge db-status             # Check container status
bridge db-migrate schema.sql # Run SQL migration
bridge db-destroy myapp      # Remove container
```

## HTTP Endpoints

| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | /db/create | container name | Create Postgres container |
| GET | /db/status | — | List running containers |
| POST | /db/migrate | SQL statements | Execute SQL via psql |
| DELETE | /db/destroy | container name | Stop and remove container |

## Container Naming

Containers are named `bridge_pg_<name>`, e.g. `bridge_pg_myapp`.

## Default Configuration

- Image: `postgres:16`
- Password: `bridge`
- Port: `5432:5432`
