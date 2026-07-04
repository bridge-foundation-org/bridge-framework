# CLI Reference

Commands for local development and codegen.

## bridge init

```bash
bridge init <project-dir>
```

Scaffolds `bridge.app`, frontend (Vite + Tailwind), and a generated client stub.

## bridge compile / compile-file

```bash
bridge compile "service hello\nendpoint ping GET /ping"
bridge compile-file ./sample.bridge
```

## bridge mode-get / mode-set

```bash
bridge mode-get
bridge mode-set full
```

## HTTP API (daemon)

| Method | Path | Description |
|--------|------|-------------|
| GET | /health | Health check |
| GET | /mode | Current ponytail mode |
| POST | /mode | Set ponytail mode |
| POST | /compile | Compile Bridge source |
| GET | /db/latest | Latest codegen output |

## Build and deploy

```bash
./scripts/build.sh
./scripts/deploy.sh
```
