# Pub/Sub Example

Comprehensive example of Bridge's Pub/Sub messaging system for building event-driven applications.

## Features

- **Topics & Subscriptions** — Publish events to topics, subscribe to receive them
- **Message Ordering** — Optional ordering keys guarantee message ordering per key
- **Retry Logic** — Automatic retries with exponential backoff (configurable)
- **Attributes** — Rich message metadata with key-value pairs
- **Message History** — Query past messages by topic
- **Dead Letter Queue** — Failed messages after max retries

## Services

### Events Service
- `POST /events/publish` — Publish an event to a topic
- `GET /events/subscribe/:topic` — Subscribe and receive messages
- `GET /events/history/:topic` — Get event history
- `POST /events/acknowledge/:message_id` — Mark message as processed

### Notifications Service
- `GET /notifications/subscribe/:user_id` — Subscribe to user notifications
- `POST /notifications/send/:user_id` — Send notification to user

## Setup

### 1. Start the daemon

```bash
cargo run -p daemon --release
```

### 2. Compile the example

```bash
bridge compile-file examples/pubsub/api.bridge > examples/pubsub/client.ts
```

## Usage

### Basic Publish

```bash
# Publish an event
curl -X POST http://localhost:8787/events/publish \
  -H "Content-Type: application/json" \
  -d '{
    "topic": "orders",
    "data": {"order_id": 123, "user_id": 456}
  }'

# Response
{
  "message_id": "msg-abc123",
  "topic": "orders",
  "sequence": 1
}
```

### Subscribe to Topic

```bash
# Subscribe to messages (long-polling)
curl http://localhost:8787/events/subscribe/orders

# Response (streaming):
{
  "messages": [
    {
      "id": "msg-abc123",
      "topic": "orders",
      "payload": "{\"order_id\": 123}",
      "attributes": {"source": "api"},
      "published_at": 1690000000000
    }
  ]
}
```

### Get Message History

```bash
# Get last 100 messages for a topic
curl "http://localhost:8787/events/history/orders?limit=100"

# Response:
{
  "topic": "orders",
  "messages": [
    {
      "id": "msg-abc123",
      "payload": "{\"order_id\": 123}",
      "published_at": 1690000000000,
      "attempt": 1
    }
  ],
  "total": 1
}
```

### Message Acknowledgment

```bash
# Mark message as processed
curl -X POST http://localhost:8787/events/acknowledge/msg-abc123 \
  -H "Content-Type: application/json"

# Response
{
  "status": "acknowledged",
  "message_id": "msg-abc123"
}
```

### User Notifications

```bash
# Send notification to user
curl -X POST http://localhost:8787/notifications/send/user-123 \
  -H "Content-Type: application/json" \
  -d '{
    "title": "New order",
    "body": "Your order #456 has shipped",
    "action_url": "/orders/456"
  }'

# Subscribe to notifications
curl http://localhost:8787/notifications/subscribe/user-123
```

## TypeScript Client Usage

```typescript
import { createClient } from './client';

const client = createClient('http://localhost:8787');

// Publish an event
const result = await client.events.publish({
  topic: 'orders',
  data: {
    order_id: 123,
    user_id: 456,
    total: 99.99
  }
});
console.log('Published:', result.message_id);

// Subscribe to messages
const subscription = await client.events.subscribe('orders');
console.log('Messages:', subscription.messages);

// Acknowledge a message
await client.events.ack(result.message_id);

// Get history
const history = await client.events.history('orders');
console.log('Last 100 messages:', history.messages.length);

// Send notification
await client.notifications.send('user-123', {
  title: 'New order',
  body: 'Your order has been processed'
});

// Subscribe to notifications
const notifs = await client.notifications.subscribe('user-123');
console.log('Notifications:', notifs.messages);
```

## Architecture

### Message Structure

```json
{
  "id": "msg-abc123",
  "topic": "orders",
  "payload": "{}",
  "attributes": {
    "source": "api",
    "version": "v1",
    "user_id": "456"
  },
  "published_at": 1690000000000,
  "ordering_key": "user-456",
  "attempt": 1
}
```

### Topics

- Named destinations for messages
- Multiple subscribers can listen to the same topic
- Messages persist until acknowledged or TTL expires
- Ordering guaranteed per `ordering_key`

### Subscriptions

- Subscribers receive messages from topics they're subscribed to
- Automatic retry on delivery failure
- Dead letter queue for messages exceeding max retries
- Configurable retry policy

## Configuration

### HTTP API Endpoints

#### Publish Message
```
POST /api/v1/pubsub/publish
{
  "topic": "string",
  "payload": "string",
  "attributes": {"key": "value"},
  "ordering_key": "optional-string"
}
```

#### Subscribe
```
GET /api/v1/pubsub/subscribe/:topic?timeout=30s&limit=100
```

#### Get History
```
GET /api/v1/pubsub/history/:topic?limit=100&offset=0
```

#### List Topics
```
GET /api/v1/pubsub/topics
```

#### Delete Topic
```
DELETE /api/v1/pubsub/topics/:topic
```

## Message Ordering

Messages with the same `ordering_key` are guaranteed to be delivered in order:

```typescript
// All messages with key "user-123" will be processed in order
const msg1 = await client.events.publish({
  topic: 'user-events',
  ordering_key: 'user-123',
  data: { action: 'login' }
});

const msg2 = await client.events.publish({
  topic: 'user-events',
  ordering_key: 'user-123',
  data: { action: 'purchase', amount: 99.99 }
});

// Guaranteed: login processed before purchase
```

## Retry Policy

- **Max Retries** — Configurable (default: 5)
- **Backoff** — Exponential (1s, 2s, 4s, 8s, 16s...)
- **Dead Letter Queue** — Messages exceeding max retries
- **Acknowledgment** — Explicit acknowledgment stops retries

## Monitoring

Check pub/sub metrics:

```bash
curl http://localhost:8787/api/v1/metrics
```

Metrics include:
- Messages published per topic
- Delivery attempts
- Failed deliveries
- Average latency

## Best Practices

1. **Use ordering keys** — For related events that must process in order
2. **Set appropriate TTLs** — Delete old messages automatically
3. **Acknowledge explicitly** — After successful processing
4. **Monitor dead letters** — Alert on messages exceeding max retries
5. **Use attributes** — Rich metadata for filtering/routing
6. **Idempotent processing** — Retry-safe message handlers

## Example: Order Processing

```typescript
// Publisher: Order Service
async function createOrder(userId: string, items: OrderItem[]) {
  const order = await saveOrder(userId, items);
  
  // Publish order created event
  await client.events.publish({
    topic: 'orders',
    ordering_key: order.id,  // Ensure sequential processing
    data: {
      order_id: order.id,
      user_id: userId,
      total: order.total,
      timestamp: Date.now()
    }
  });
}

// Subscriber: Inventory Service
async function processOrderEvents() {
  while (true) {
    const subscription = await client.events.subscribe('orders');
    
    for (const msg of subscription.messages) {
      try {
        const order = JSON.parse(msg.payload);
        await updateInventory(order);
        await client.events.ack(msg.id);  // Acknowledge success
      } catch (err) {
        console.error('Failed to process order:', err);
        // Don't acknowledge - retry will be automatic
      }
    }
  }
}
```

## Next Steps

- Add message filtering
- Implement custom retry policies
- Set up dead letter queue monitoring
- Create event sourcing patterns
- Build event replay functionality
