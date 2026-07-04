# Deployment Guide

## Building for Production

```bash
./scripts/build.sh
```

Output in `dist/`:
- `bin/daemon` — release daemon binary
- `bin/bridge` — release CLI binary  
- `frontend/` — static Vite build
- `docs/` — markdown documentation

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| BRIDGE_TCP_ADDR | 127.0.0.1:7878 | TCP protocol listener |
| BRIDGE_HTTP_ADDR | 127.0.0.1:8787 | HTTP API listener |
| BRIDGE_REDIS_ADDR | 127.0.0.1:6399 | Miniredis listener |

## Running in Production

```bash
./bin/daemon
```

## Serving the Frontend

The built frontend is static HTML/JS/CSS. Serve with any static host:

```bash
npx serve dist/frontend
```
