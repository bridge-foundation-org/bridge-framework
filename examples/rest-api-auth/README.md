# REST API Example with Authentication

A comprehensive example demonstrating:
- Multiple services (users, posts)
- Bearer token authentication
- API key authentication
- RESTful CRUD operations
- Path parameters

## Features

### Users Service (Bearer Token Auth)
- `GET /users` — List all users
- `GET /users/:id` — Get a specific user
- `POST /users` — Create a new user
- `PUT /users/:id` — Update a user
- `DELETE /users/:id` — Delete a user

### Posts Service (API Key Auth)
- `GET /posts` — List all posts
- `GET /posts/:id` — Get a specific post
- `POST /posts` — Create a new post
- `POST /posts/:id/publish` — Publish a post

## Setup

### 1. Start the Bridge daemon

```bash
cargo run -p daemon --release
```

### 2. Compile the DSL

```bash
bridge compile-file examples/rest-api-auth/api.bridge > examples/rest-api-auth/client.ts
```

### 3. Set authentication tokens

#### Users Service (Bearer Token)
```bash
# Set a bearer token for the users service
curl -X POST http://localhost:8787/api/v1/auth/set \
  -H "Content-Type: application/json" \
  -d '{"scheme":"bearer","token":"my-secret-token-123"}'
```

#### Posts Service (API Key)
```bash
# Set an API key for the posts service
curl -X POST http://localhost:8787/api/v1/auth/set \
  -H "Content-Type: application/json" \
  -d '{"scheme":"api_key","token":"my-api-key-456"}'
```

## Usage

### Test Users Service with Bearer Token

```bash
# With valid bearer token
curl -X GET http://localhost:8787/users \
  -H "Authorization: Bearer my-secret-token-123"

# Result: 200 OK

# Without token - should fail
curl -X GET http://localhost:8787/users
# Result: 401 Unauthorized
```

### Test Posts Service with API Key

```bash
# With valid API key
curl -X GET http://localhost:8787/posts \
  -H "X-Api-Key: my-api-key-456"

# Result: 200 OK

# Without API key - should fail
curl -X GET http://localhost:8787/posts
# Result: 401 Unauthorized
```

### Using the Generated TypeScript Client

```typescript
import { createClient } from './client';

const client = createClient('http://localhost:8787');

// Users service with bearer token
const usersClient = client.users;
try {
  const users = await usersClient.list();
  console.log('Users:', users);
} catch (err) {
  console.error('Failed to fetch users:', err);
}

// Posts service with API key
const postsClient = client.posts;
try {
  const posts = await postsClient.list();
  console.log('Posts:', posts);
} catch (err) {
  console.error('Failed to fetch posts:', err);
}

// Create a new user
const newUser = await usersClient.create({
  name: 'Alice',
  email: 'alice@example.com'
});

// Update a user
const updated = await usersClient.update('user-123', {
  name: 'Alice Johnson'
});

// Delete a user
await usersClient.delete('user-123');
```

## Authentication Schemes

### Bearer Token (Users Service)
The bearer token is passed in the `Authorization` header:
```
Authorization: Bearer <token>
```

### API Key (Posts Service)
The API key is passed in the `X-Api-Key` header:
```
X-Api-Key: <token>
```

### Alternative Headers
All services also accept:
- `X-Bridge-Token: <token>` — Universal token header
- Different services can have different auth schemes

## Error Handling

When authentication fails, you'll receive a 401 Unauthorized response:

```json
{
  "status": 401,
  "error": "authentication required",
  "message": "Invalid or missing authentication token"
}
```

The generated TypeScript client throws a `BridgeError` with status code 401.

## DSL Syntax Reference

```bridge
# Define a service with authentication
service <name>
auth <scheme>        # bearer | api_key

# Define endpoints
endpoint <name> <METHOD> <path>
  # Optional per-endpoint auth override
  auth <scheme>
```

## Next Steps

- Add a database layer to store users/posts
- Implement middleware for logging
- Add rate limiting to prevent abuse
- Create unit tests for endpoints
