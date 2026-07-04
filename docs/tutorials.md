# Tutorials

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
