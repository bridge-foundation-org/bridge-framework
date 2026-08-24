//! Bridge Pub/Sub — topics, subscriptions, message ordering, retry logic.
//!
//! Inspired by Encore commits 1207 (sqs-sns pubsub),
//! 1363 (import-pubsub-subscriptions), 1427 (max-retries-for-nsq),
//! 1696 (serialize-custom-pubsub-attrs), 1758 (message-ordering),
//! 1965 (topic references in TypeScript).
//!
//! Zero external dependencies — pure std.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Message ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub topic: String,
    pub payload: String,
    pub attributes: HashMap<String, String>,
    pub published_at: u64,
    pub ordering_key: Option<String>,
    pub attempt: u32,
}

impl Message {
    pub fn new(topic: impl Into<String>, payload: impl Into<String>) -> Self {
        Message {
            id: gen_id(),
            topic: topic.into(),
            payload: payload.into(),
            attributes: HashMap::new(),
            published_at: now_ms(),
            ordering_key: None,
            attempt: 0,
        }
    }

    pub fn with_attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    pub fn with_ordering_key(mut self, key: impl Into<String>) -> Self {
        self.ordering_key = Some(key.into());
        self
    }

    pub fn to_json(&self) -> String {
        let attrs: String = self
            .attributes
            .iter()
            .map(|(k, v)| format!(",\"{}\":\"{}\"", k, v))
            .collect();
        let ordering = self
            .ordering_key
            .as_deref()
            .map(|k| format!(",\"ordering_key\":\"{}\"", k))
            .unwrap_or_default();
        format!(
            r#"{{"id":"{id}","topic":"{topic}","payload":{payload},"published_at":{ts},"attempt":{attempt}{ordering},{attrs}}}"#,
            id = self.id,
            topic = self.topic,
            payload = self.payload,
            ts = self.published_at,
            attempt = self.attempt,
            ordering = ordering,
            attrs = attrs.trim_start_matches(','),
        )
    }
}

// ── Subscription config ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    pub max_concurrency: usize,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub ack_deadline_secs: u32,
    pub message_ordering: bool,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        SubscriptionConfig {
            max_concurrency: 10,
            max_retries: 3,
            retry_delay_ms: 1000,
            ack_deadline_secs: 30,
            message_ordering: false,
        }
    }
}

// ── Internal queue entry ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum MessageState {
    Pending,
    Delivered { at: u64 },
    Acked,
    DeadLetter { reason: String },
}

#[derive(Debug, Clone)]
struct QueueEntry {
    message: Message,
    state: MessageState,
}

// ── Broker ────────────────────────────────────────────────────────────────────

/// In-process pub/sub broker — suitable for local development.
/// Replace with NSQ/SQS/GCP PubSub adapter in production via trait objects.
#[derive(Clone)]
pub struct Broker(Arc<Mutex<BrokerInner>>);

struct BrokerInner {
    /// topic_name → subscriber_names
    subscriptions: HashMap<String, Vec<String>>,
    /// (topic, subscriber) → queue of entries
    queues: HashMap<(String, String), VecDeque<QueueEntry>>,
    /// Delivered but not yet acked: msg_id → (topic, subscriber, delivered_at)
    in_flight: HashMap<String, (String, String, u64)>,
    /// Config per subscription
    configs: HashMap<String, SubscriptionConfig>,
    /// Dead-letter queue per (topic, subscriber)
    dlq: HashMap<(String, String), Vec<Message>>,
    /// Published message count
    publish_count: u64,
}

impl Broker {
    pub fn new() -> Self {
        Broker(Arc::new(Mutex::new(BrokerInner {
            subscriptions: HashMap::new(),
            queues: HashMap::new(),
            in_flight: HashMap::new(),
            configs: HashMap::new(),
            dlq: HashMap::new(),
            publish_count: 0,
        })))
    }

    /// Register a subscriber on a topic.
    pub fn subscribe(&self, topic: &str, subscriber: &str, config: SubscriptionConfig) {
        let mut inner = self.0.lock().unwrap();
        inner
            .subscriptions
            .entry(topic.to_string())
            .or_default()
            .push(subscriber.to_string());
        inner.configs.insert(subscriber.to_string(), config);
        inner
            .queues
            .entry((topic.to_string(), subscriber.to_string()))
            .or_default();
    }

    /// Publish a message to a topic. Fans out to all subscribers.
    pub fn publish(&self, msg: Message) -> u64 {
        let mut inner = self.0.lock().unwrap();
        inner.publish_count += 1;
        let subs = inner
            .subscriptions
            .get(&msg.topic)
            .cloned()
            .unwrap_or_default();
        for sub in &subs {
            let queue = inner
                .queues
                .entry((msg.topic.clone(), sub.clone()))
                .or_default();
            queue.push_back(QueueEntry {
                message: msg.clone(),
                state: MessageState::Pending,
            });
        }
        inner.publish_count
    }

    /// Pull the next pending message for a subscriber.
    pub fn pull(&self, topic: &str, subscriber: &str) -> Option<Message> {
        let mut inner = self.0.lock().unwrap();
        let queue = inner
            .queues
            .get_mut(&(topic.to_string(), subscriber.to_string()))?;

        let entry = queue
            .iter_mut()
            .find(|e| matches!(e.state, MessageState::Pending))?;
        entry.state = MessageState::Delivered { at: now_ms() };
        entry.message.attempt += 1;

        let msg = entry.message.clone();
        inner.in_flight.insert(
            msg.id.clone(),
            (topic.to_string(), subscriber.to_string(), now_ms()),
        );
        Some(msg)
    }

    /// Acknowledge a message (removes from queue).
    pub fn ack(&self, msg_id: &str) -> bool {
        let mut inner = self.0.lock().unwrap();
        if let Some((topic, sub, _)) = inner.in_flight.remove(msg_id) {
            if let Some(queue) = inner.queues.get_mut(&(topic, sub)) {
                queue.retain(|e| e.message.id != msg_id);
                return true;
            }
        }
        false
    }

    /// Negative-acknowledge — requeue or dead-letter.
    pub fn nack(&self, msg_id: &str, reason: &str) -> bool {
        let mut inner = self.0.lock().unwrap();
        if let Some((topic, sub, _)) = inner.in_flight.remove(msg_id) {
            let max_retries = inner.configs.get(&sub).map(|c| c.max_retries).unwrap_or(3);
            let queue = inner
                .queues
                .entry((topic.clone(), sub.clone()))
                .or_default();
            if let Some(entry) = queue.iter_mut().find(|e| e.message.id == msg_id) {
                if entry.message.attempt > max_retries {
                    let dlq_msg = entry.message.clone();
                    entry.state = MessageState::DeadLetter {
                        reason: reason.to_string(),
                    };
                    inner.dlq.entry((topic, sub)).or_default().push(dlq_msg);
                } else {
                    entry.state = MessageState::Pending; // requeue
                }
                return true;
            }
        }
        false
    }

    /// Queue depth for a topic/subscriber pair.
    pub fn queue_depth(&self, topic: &str, subscriber: &str) -> usize {
        let inner = self.0.lock().unwrap();
        inner
            .queues
            .get(&(topic.to_string(), subscriber.to_string()))
            .map(|q| {
                q.iter()
                    .filter(|e| matches!(e.state, MessageState::Pending))
                    .count()
            })
            .unwrap_or(0)
    }

    /// Dead-letter queue depth.
    pub fn dlq_depth(&self, topic: &str, subscriber: &str) -> usize {
        let inner = self.0.lock().unwrap();
        inner
            .dlq
            .get(&(topic.to_string(), subscriber.to_string()))
            .map(|q| q.len())
            .unwrap_or(0)
    }

    /// Full JSON status.
    pub fn status_json(&self) -> String {
        let inner = self.0.lock().unwrap();
        let topics: Vec<String> = inner.subscriptions.keys().cloned().collect();
        let sub_count: usize = inner.subscriptions.values().map(|v| v.len()).sum();
        format!(
            r#"{{"topics":{tc},"subscriptions":{sc},"published":{pub},"in_flight":{inf}}}"#,
            tc  = topics.len(),
            sc  = sub_count,
            pub = inner.publish_count,
            inf = inner.in_flight.len(),
        )
    }
}

impl Default for Broker {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn gen_id() -> String {
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("msg-{}-{}", now_ms(), n)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn basic_broker() -> Broker {
        let b = Broker::new();
        b.subscribe("orders", "billing", SubscriptionConfig::default());
        b.subscribe("orders", "shipping", SubscriptionConfig::default());
        b
    }

    #[test]
    fn publish_fans_out() {
        let b = basic_broker();
        b.publish(Message::new("orders", r#"{"id":1}"#));
        assert_eq!(b.queue_depth("orders", "billing"), 1);
        assert_eq!(b.queue_depth("orders", "shipping"), 1);
    }

    #[test]
    fn pull_delivers_message() {
        let b = basic_broker();
        b.publish(Message::new("orders", r#"{"id":2}"#));
        let msg = b.pull("orders", "billing");
        assert!(msg.is_some());
        let msg = msg.unwrap();
        assert_eq!(msg.topic, "orders");
        assert_eq!(msg.attempt, 1);
    }

    #[test]
    fn ack_removes_from_queue() {
        let b = basic_broker();
        b.publish(Message::new("orders", r#"{"id":3}"#));
        let msg = b.pull("orders", "billing").unwrap();
        assert!(b.ack(&msg.id));
        assert_eq!(b.queue_depth("orders", "billing"), 0);
    }

    #[test]
    fn nack_requeues() {
        let b = basic_broker();
        b.publish(Message::new("orders", r#"{"id":4}"#));
        let msg = b.pull("orders", "billing").unwrap();
        b.nack(&msg.id, "processing error");
        // Should be requeued (attempt 1 < max_retries 3)
        assert_eq!(b.queue_depth("orders", "billing"), 1);
    }

    #[test]
    fn nack_to_dlq_after_max_retries() {
        let b = Broker::new();
        b.subscribe(
            "jobs",
            "worker",
            SubscriptionConfig {
                max_retries: 1,
                ..Default::default()
            },
        );
        b.publish(Message::new("jobs", r#"{"task":"x"}"#));
        let msg = b.pull("jobs", "worker").unwrap();
        b.nack(&msg.id, "fail");
        // Now pull and nack again — should hit max_retries
        let msg2 = b.pull("jobs", "worker").unwrap();
        b.nack(&msg2.id, "fail again");
        assert_eq!(b.dlq_depth("jobs", "worker"), 1);
    }

    #[test]
    fn message_with_attributes() {
        let msg = Message::new("events", r#"{"type":"signup"}"#)
            .with_attr("source", "web")
            .with_attr("env", "prod")
            .with_ordering_key("user-42");
        assert_eq!(msg.attributes.get("source"), Some(&"web".to_string()));
        assert_eq!(msg.ordering_key, Some("user-42".to_string()));
        let json = msg.to_json();
        assert!(json.contains("source"));
        assert!(json.contains("ordering_key"));
    }

    #[test]
    fn broker_status_json() {
        let b = basic_broker();
        b.publish(Message::new("orders", "{}"));
        let status = b.status_json();
        assert!(status.contains("\"topics\":1"));
        assert!(status.contains("\"subscriptions\":2"));
        assert!(status.contains("\"published\":1"));
    }

    #[test]
    fn pull_returns_none_empty_queue() {
        let b = basic_broker();
        assert!(b.pull("orders", "billing").is_none());
    }
}
