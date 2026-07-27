# Hello World Example

A minimal Bridge Framework example demonstrating basic endpoints.

## Features

- **ping** — Simple health check endpoint
- **echo** — Echo back a message  
- **greet** — Greet someone by name using path parameters

## Quick Start

### 1. Start the Bridge daemon

In terminal 1:
```bash
cargo run -p daemon --release
```

The daemon will start on:
- HTTP: `http://localhost:8787`
- TCP: `tcp://localhost:8786`
- Redis: `redis://localhost:6399`

### 2. Compile the Bridge DSL

In terminal 2:
```bash
# Compile the hello.bridge file and generate TypeScript client
bridge compile-file examples/hello-world/hello.bridge
```

This will:
- Validate the DSL syntax
- Generate a TypeScript client in `examples/hello-world/client.ts`
- Output an OpenAPI 3.0 spec in `examples/hello-world/openapi.json`

### 3. Test the endpoints

Using the HTTP API:

```bash
# Ping
curl http://localhost:8787/api/v1/health

# Echo message
curl -X POST http://localhost:8787/echo \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello, Bridge!"}'

# Greet with path parameter
curl http://localhost:8787/greet/Alice
```

## DSL Syntax Reference

```bridge
// Service definition
service <name>

// Optional authentication
auth bearer          # Bearer token in Authorization header
auth api_key         # API key in X-Api-Key header

// Endpoint definition
endpoint <name> <method> <path>
  req <type> <field>    # Request body field
  resp <type> <field>   # Response body field
```

## Generated Client

After compilation, you can use the TypeScript client:

```typescript
import { createHelloClient } from './client';

const client = createHelloClient('http://localhost:8787');

// Ping
await client.hello.ping();

// Echo
const result = await client.hello.echo({ message: 'Hello!' });
console.log(result.message);

// Greet
const greeting = await client.hello.greet('Alice');
console.log(greeting.message);
```

## Files

- `hello.bridge` — Bridge DSL source
- `client.ts` — Generated TypeScript client (auto-generated)
- `openapi.json` — OpenAPI 3.0 specification (auto-generated)
