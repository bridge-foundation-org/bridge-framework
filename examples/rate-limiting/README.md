# Rate Limiting Example

This example demonstrates Bridge's rate limiting capabilities using the token bucket algorithm.

## Features

- **Token bucket algorithm** — Refills at a configurable rate
- **Per-endpoint rules** — Different limits for different endpoints
- **Wildcard matching** — Apply limits to multiple endpoints
- **429 responses** — Returns Retry-After header when limit exceeded
- **HTTP API** — Configure limits at runtime via REST API

## Setup

### 1. Start the daemon

```bash
cargo run -p daemon --release
```

### 2. Configure rate limiting

Add rate limit rules via the HTTP API:

```bash
# Limit /fast to 100 requests per second (no limit really)
curl -X POST http://localhost:8787/api/v1/ratelimit \
  -H "Content-Type: application/json" \
  -d '{
    "method": "GET",
    "path": "/fast",
    "capacity": 100,
    "refill_rate": 100.0
  }'

# Limit /slow to 10 requests per second
curl -X POST http://localhost:8787/api/v1/ratelimit \
  -H "Content-Type: application/json" \
  -d '{
    "method": "GET",
    "path": "/slow",
    "capacity": 10,
    "refill_rate": 10.0
  }'

# Limit /premium to 5 requests per second
curl -X POST http://localhost:8787/api/v1/ratelimit \
  -H "Content-Type: application/json" \
  -d '{
    "method": "GET",
    "path": "/premium",
    "capacity": 5,
    "refill_rate": 5.0
  }'

# Global limit: 1000 requests per second across all endpoints
curl -X POST http://localhost:8787/api/v1/ratelimit \
  -H "Content-Type: application/json" \
  -d '{
    "method": "*",
    "path": "*",
    "capacity": 1000,
    "refill_rate": 1000.0
  }'
```

### 3. Test rate limiting

#### Successful requests (within limit)

```bash
# Call /fast endpoint 5 times - all succeed
for i in {1..5}; do
  curl http://localhost:8787/fast
done
```

#### Exceeding limits (429 responses)

```bash
# Call /slow endpoint 15 times rapidly
# First 10 succeed, rest get 429 Too Many Requests
for i in {1..15}; do
  response=$(curl -s -w "%{http_code}" -o /dev/null http://localhost:8787/slow)
  echo "Request $i: $response"
done
```

## Response Headers

When a request succeeds, you'll see:

```
X-RateLimit-Limit: 10        # Total capacity
X-RateLimit-Remaining: 9     # Tokens remaining
X-RateLimit-Reset: 1690000000 # Unix timestamp when limit resets
```

When limit exceeded (429):

```
HTTP/1.1 429 Too Many Requests
Content-Type: application/json
Retry-After: 1

{
  "status": 429,
  "error": "rate_limit_exceeded",
  "message": "Too many requests",
  "retry_after": 1
}
```

## Configuration Reference

### Rate Limit Rule

```json
{
  "method": "GET",           // HTTP method: GET, POST, *, etc.
  "path": "/api/users",      // Endpoint path: supports wildcards (*, ?)
  "capacity": 10,            // Maximum tokens in bucket
  "refill_rate": 10.0        // Tokens added per second
}
```

### Wildcard Matching

- `*` in method matches any HTTP method
- `*` in path matches any endpoint
- `?` in path matches any single character
- `/api/*` matches `/api/users`, `/api/posts`, etc.
- `/*` matches any root endpoint

### Specificity Rules

Rate limits are matched in order of specificity:

1. **Exact match** — `/api/users` GET
2. **Method wildcard** — `*` /api/users
3. **Path wildcard** — GET /api/*
4. **Global wildcard** — * *

The most specific rule takes precedence.

## HTTP API Endpoints

### List current rate limits

```bash
curl http://localhost:8787/api/v1/ratelimit
```

Response:
```json
{
  "rules": [
    {
      "method": "GET",
      "path": "/slow",
      "capacity": 10,
      "refill_rate": 10.0
    }
  ]
}
```

### Add a rate limit rule

```bash
curl -X POST http://localhost:8787/api/v1/ratelimit \
  -H "Content-Type: application/json" \
  -d '{"method":"GET","path":"/slow","capacity":10,"refill_rate":10.0}'
```

### Remove a rate limit rule

```bash
curl -X DELETE http://localhost:8787/api/v1/ratelimit \
  -H "Content-Type: application/json" \
  -d '{"method":"GET","path":"/slow"}'
```

## Advanced Testing

### Load test with rate limiting

```bash
# Create a load test that respects rate limits
ab -n 1000 -c 10 http://localhost:8787/slow
# Apache Bench will show how many 429 responses

# Expected: ~10 successful per second, rest get 429
```

### Test token bucket refill

```bash
# Exhaust the bucket
for i in {1..10}; do curl http://localhost:8787/slow; done

# All 10 succeed, 11th gets 429
curl http://localhost:8787/slow  # 429

# Wait 100ms (1/10th second at 10 tokens/sec = 1 token refilled)
sleep 0.1

# Now 1 more request succeeds
curl http://localhost:8787/slow  # 200
```

## Metrics

Check rate limit metrics via:

```bash
curl http://localhost:8787/api/v1/metrics
```

This includes:
- Total requests processed
- Requests rejected by rate limit (429)
- Average response time
- Rate limit bucket states

## Best Practices

1. **Set reasonable defaults** — Global wildcard rule first
2. **Specific rules for sensitive endpoints** — Lower limits for expensive operations
3. **Monitor refill rates** — Match expected traffic patterns
4. **Plan for spikes** — Higher capacity than average rate
5. **Use Retry-After** — Clients should respect the header
