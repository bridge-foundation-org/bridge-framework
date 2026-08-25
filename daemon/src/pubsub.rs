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
        let mut fields: Vec<String> = vec![
            format!(r#""id":"{}""#, self.id),
            format!(r#""topic":"{}""#, self.topic),
            format!(r#""payload":{}"#, self.payload),
            format!(r#""published_at":{}"#, self.published_at),
            format!(r#""attempt":{}"#, self.attempt),
        ];
        if let Some(k) = &self.ordering_key {
            fields.push(format!(r#""ordering_key":"{k}""#));
        }
        for (k, v) in &self.attributes {
            fields.push(format!(r#""{k}":"{v}""#));
        }
        format!("{{{}}}", fields.join(","))
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

    /// Register a subscriber on a topic. Idempotent: re-subscribing updates
    /// the subscription's config in place instead of duplicating the entry
    /// (which would double-deliver every later publish).
    pub fn subscribe(&self, topic: &str, subscriber: &str, config: SubscriptionConfig) {
        let mut inner = self.0.lock().unwrap();
        let subs = inner.subscriptions.entry(topic.to_string()).or_default();
        if !subs.iter().any(|s| s == subscriber) {
            subs.push(subscriber.to_string());
        }
        inner.configs.insert(subscriber.to_string(), config);
        inner
            .queues
            .entry((topic.to_string(), subscriber.to_string()))
            .or_default();
    }

    /// Does the topic exist (declared or materialized by a publish)?
    pub fn topic_exists(&self, topic: &str) -> bool {
        self.0.lock().unwrap().subscriptions.contains_key(topic)
    }

    /// Is `subscriber` attached to `topic`?
    pub fn has_subscription(&self, topic: &str, subscriber: &str) -> bool {
        self.0
            .lock()
            .unwrap()
            .subscriptions
            .get(topic)
            .map(|subs| subs.iter().any(|s| s == subscriber))
            .unwrap_or(false)
    }

    /// Declare an empty topic (no subscribers yet).
    pub fn ensure_topic(&self, topic: &str) {
        self.0
            .lock()
            .unwrap()
            .subscriptions
            .entry(topic.to_string())
            .or_default();
    }

    /// Number of subscribers attached to `topic` (0 when the topic is absent).
    pub fn subscriber_count(&self, topic: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .subscriptions
            .get(topic)
            .map(|subs| subs.len())
            .unwrap_or(0)
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
    ///
    /// Ordered subscriptions (`message_ordering: true`) enforce strict FIFO:
    /// if any earlier message is still in flight (delivered, unacked), pull
    /// returns None until it is settled — mirroring GCP PubSub ordering.
    pub fn pull(&self, topic: &str, subscriber: &str) -> Option<Message> {
        let mut inner = self.0.lock().unwrap();

        // Read config BEFORE taking the mutable queue borrow.
        let ordered = inner
            .configs
            .get(subscriber)
            .map(|c| c.message_ordering)
            .unwrap_or(false);
        let queue = inner
            .queues
            .get_mut(&(topic.to_string(), subscriber.to_string()))?;

        // Ordered delivery: block when an earlier message is unsettled.
        if ordered
            && queue
                .iter()
                .any(|e| matches!(e.state, MessageState::Delivered { .. }))
        {
            return None;
        }

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

    /// Detailed JSON for one subscription's queues and config.
    pub fn subscription_json(&self, topic: &str, subscriber: &str) -> Option<String> {
        let inner = self.0.lock().unwrap();
        if !inner
            .subscriptions
            .get(topic)?
            .iter()
            .any(|s| s == subscriber)
        {
            return None;
        }
        let key = (topic.to_string(), subscriber.to_string());
        let cfg = inner.configs.get(subscriber);
        let pending = inner
            .queues
            .get(&key)
            .map(|q| {
                q.iter()
                    .filter(|e| matches!(e.state, MessageState::Pending))
                    .count()
            })
            .unwrap_or(0);
        let dlq_len = inner.dlq.get(&key).map(|q| q.len()).unwrap_or(0);
        Some(format!(
            r#"{{"topic":"{t}","subscriber":"{s}","pending":{p},"dlq":{d},"max_retries":{mr},"message_ordering":{mo}}}"#,
            t = topic,
            s = subscriber,
            p = pending,
            d = dlq_len,
            mr = cfg.map(|c| c.max_retries).unwrap_or(3),
            mo = cfg.map(|c| c.message_ordering).unwrap_or(false),
        ))
    }

    /// List all subscriptions as JSON array items.
    pub fn subscriptions_json(&self) -> String {
        let inner = self.0.lock().unwrap();
        let mut pairs: Vec<(&String, &String)> = inner
            .subscriptions
            .iter()
            .flat_map(|(t, subs)| subs.iter().map(move |s| (t, s)))
            .collect();
        pairs.sort();
        let items: Vec<String> = pairs
            .iter()
            .map(|(t, s)| format!(r#"{{"topic":"{t}","subscriber":"{s}"}}"#))
            .collect();
        format!(r#"{{"subscriptions":[{}]}}"#, items.join(","))
    }

    /// Messages currently sitting in the DLQ for (topic, subscriber).
    pub fn dlq_messages_json(&self, topic: &str, subscriber: &str) -> String {
        let inner = self.0.lock().unwrap();
        let msgs = inner
            .dlq
            .get(&(topic.to_string(), subscriber.to_string()))
            .cloned()
            .unwrap_or_default();
        let items: Vec<String> = msgs.iter().map(|m| m.to_json()).collect();
        format!(
            r#"{{"topic":"{t}","subscriber":"{s}","messages":[{items}]}}"#,
            t = topic,
            s = subscriber,
            items = items.join(",")
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

    #[test]
    fn ordered_subscription_blocks_on_inflight_head() {
        let b = Broker::new();
        b.subscribe(
            "events",
            "seq",
            SubscriptionConfig {
                message_ordering: true,
                ..Default::default()
            },
        );
        let m1 = Message::new("events", r#"{"n":1}"#);
        let m2 = Message::new("events", r#"{"n":2}"#);
        assert_eq!(b.publish(m1), 1);
        assert_eq!(b.publish(m2), 2);

        // First pull OK; second must block while m1 is unacked.
        let first = b.pull("events", "seq").expect("first message");
        assert_eq!(first.payload, r#"{"n":1}"#);
        assert!(b.pull("events", "seq").is_none(), "ordering must block");

        // Settle the head — next pull now yields m2.
        assert!(b.ack(&first.id));
        let second = b.pull("events", "seq").expect("second after ack");
        assert_eq!(second.payload, r#"{"n":2}"#);
    }

    #[test]
    fn unordered_subscription_allows_parallel_inflight() {
        let b = basic_broker(); // default config: message_ordering false
        b.publish(Message::new("orders", r#"{"n":1}"#));
        b.publish(Message::new("orders", r#"{"n":2}"#));
        assert!(b.pull("orders", "billing").is_some());
        assert!(
            b.pull("orders", "billing").is_some(),
            "unordered keeps flowing"
        );
    }

    #[test]
    fn dlq_messages_json_lists_dead_letters() {
        let b = Broker::new();
        b.subscribe(
            "jobs",
            "w",
            SubscriptionConfig {
                max_retries: 0,
                ..Default::default()
            },
        );
        b.publish(Message::new("jobs", r#"{"t":1}"#));
        let msg = b.pull("jobs", "w").unwrap();
        b.nack(&msg.id, "boom");
        let json = b.dlq_messages_json("jobs", "w");
        assert!(
            json.contains(r#"{"t":1}"#),
            "dead letter payload missing: {json}"
        );
    }

    #[test]
    fn subscriptions_json_lists_pairs() {
        let b = basic_broker();
        let json = b.subscriptions_json();
        assert!(json.contains(r#""topic":"orders","subscriber":"billing""#));
        assert!(json.contains(r#""topic":"orders","subscriber":"shipping""#));
    }

    #[test]
    fn subscribe_is_idempotent_no_double_delivery() {
        let b = Broker::new();
        b.subscribe("orders", "billing", SubscriptionConfig::default());
        b.subscribe("orders", "billing", SubscriptionConfig::default());
        assert_eq!(
            b.subscriber_count("orders"),
            1,
            "resubscribe must not duplicate"
        );
        b.publish(Message::new("orders", r#"{"n":1}"#));
        assert_eq!(
            b.queue_depth("orders", "billing"),
            1,
            "duplicate subscription must not double-enqueue"
        );
    }

    #[test]
    fn subscriber_count_reflects_attached_subscribers() {
        let b = basic_broker();
        assert_eq!(b.subscriber_count("orders"), 2);
        assert_eq!(b.subscriber_count("nope"), 0);
    }

    #[test]
    fn message_to_json_without_extras_is_valid_object() {
        let json = Message::new("t", "null").to_json();
        assert!(json.starts_with('{') && json.ends_with('}'), "got: {json}");
        assert!(
            !json.contains(",}"),
            "trailing comma makes invalid JSON: {json}"
        );
        assert!(json.contains(r#""id":"msg-"#));
    }
}
