//! Multi-Provider Pub/Sub messaging (V2 - new architecture)
//!
//! Abstract pub/sub interface supporting multiple providers (memory, AWS SNS/SQS, GCP PubSub)

use std::collections::HashMap;

/// Pub/Sub provider types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PubSubProvider {
    Memory,
    AwsSns,
    AwsSqs,
    GcpPubSub,
}

impl PubSubProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            PubSubProvider::Memory => "memory",
            PubSubProvider::AwsSns => "aws-sns",
            PubSubProvider::AwsSqs => "aws-sqs",
            PubSubProvider::GcpPubSub => "gcp-pubsub",
        }
    }
}

/// Message metadata
#[derive(Clone, Debug)]
pub struct MessageMetadata {
    pub message_id: String,
    pub timestamp: u64,
    pub attributes: HashMap<String, String>,
}

impl MessageMetadata {
    pub fn new(message_id: impl Into<String>) -> Self {
        MessageMetadata {
            message_id: message_id.into(),
            timestamp: current_timestamp_ms(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// Published message
#[derive(Clone, Debug)]
pub struct PubSubMessage {
    pub data: Vec<u8>,
    pub metadata: MessageMetadata,
}

impl PubSubMessage {
    pub fn new(data: Vec<u8>, message_id: impl Into<String>) -> Self {
        PubSubMessage {
            data,
            metadata: MessageMetadata::new(message_id),
        }
    }

    pub fn from_string(data: impl Into<String>, message_id: impl Into<String>) -> Self {
        PubSubMessage::new(data.into().into_bytes(), message_id)
    }

    pub fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.data).to_string()
    }
}

/// Topic subscription
#[derive(Clone)]
pub struct Subscription {
    pub id: String,
    pub topic: String,
}

impl Subscription {
    pub fn new(id: impl Into<String>, topic: impl Into<String>) -> Self {
        Subscription {
            id: id.into(),
            topic: topic.into(),
        }
    }
}

/// Pub/Sub topic
#[derive(Clone, Debug)]
pub struct Topic {
    pub name: String,
    pub message_count: usize,
}

impl Topic {
    pub fn new(name: impl Into<String>) -> Self {
        Topic {
            name: name.into(),
            message_count: 0,
        }
    }
}

/// Abstract Pub/Sub trait
pub trait PubSubBackend: Send + Sync {
    /// Publish a message to a topic
    fn publish(&mut self, topic: &str, message: PubSubMessage) -> Result<String, String>;

    /// Create a topic
    fn create_topic(&mut self, name: &str) -> Result<Topic, String>;

    /// Delete a topic
    fn delete_topic(&mut self, name: &str) -> Result<(), String>;

    /// List all topics
    fn list_topics(&self) -> Vec<Topic>;

    /// Get topic by name
    fn get_topic(&self, name: &str) -> Option<Topic>;

    /// Subscribe to a topic
    fn subscribe(&mut self, topic: &str, subscription_id: &str) -> Result<Subscription, String>;

    /// Unsubscribe from a topic
    fn unsubscribe(&mut self, subscription_id: &str) -> Result<(), String>;

    /// Receive messages from subscription
    fn receive(&self, subscription_id: &str, max_messages: usize) -> Vec<PubSubMessage>;
}

/// In-memory pub/sub backend
pub struct MemoryPubSub {
    topics: HashMap<String, Vec<PubSubMessage>>,
    subscriptions: HashMap<String, String>, // subscription_id -> topic
}

impl MemoryPubSub {
    pub fn new() -> Self {
        MemoryPubSub {
            topics: HashMap::new(),
            subscriptions: HashMap::new(),
        }
    }
}

impl Default for MemoryPubSub {
    fn default() -> Self {
        Self::new()
    }
}

impl PubSubBackend for MemoryPubSub {
    fn publish(&mut self, topic: &str, message: PubSubMessage) -> Result<String, String> {
        let message_id = message.metadata.message_id.clone();
        self.topics
            .entry(topic.to_string())
            .or_insert_with(Vec::new)
            .push(message);
        Ok(message_id)
    }

    fn create_topic(&mut self, name: &str) -> Result<Topic, String> {
        if self.topics.contains_key(name) {
            return Err(format!("Topic {} already exists", name));
        }
        self.topics.insert(name.to_string(), Vec::new());
        Ok(Topic::new(name))
    }

    fn delete_topic(&mut self, name: &str) -> Result<(), String> {
        if self.topics.remove(name).is_some() {
            Ok(())
        } else {
            Err(format!("Topic {} not found", name))
        }
    }

    fn list_topics(&self) -> Vec<Topic> {
        self.topics
            .iter()
            .map(|(name, messages)| Topic {
                name: name.clone(),
                message_count: messages.len(),
            })
            .collect()
    }

    fn get_topic(&self, name: &str) -> Option<Topic> {
        self.topics.get(name).map(|messages| Topic {
            name: name.to_string(),
            message_count: messages.len(),
        })
    }

    fn subscribe(&mut self, topic: &str, subscription_id: &str) -> Result<Subscription, String> {
        if !self.topics.contains_key(topic) {
            return Err(format!("Topic {} not found", topic));
        }
        self.subscriptions
            .insert(subscription_id.to_string(), topic.to_string());
        Ok(Subscription::new(subscription_id, topic))
    }

    fn unsubscribe(&mut self, subscription_id: &str) -> Result<(), String> {
        if self.subscriptions.remove(subscription_id).is_some() {
            Ok(())
        } else {
            Err(format!("Subscription {} not found", subscription_id))
        }
    }

    fn receive(&self, subscription_id: &str, max_messages: usize) -> Vec<PubSubMessage> {
        if let Some(topic_name) = self.subscriptions.get(subscription_id) {
            if let Some(messages) = self.topics.get(topic_name) {
                return messages.iter().take(max_messages).cloned().collect();
            }
        }
        Vec::new()
    }
}

/// Pub/Sub client wrapper
pub struct PubSubClient {
    backend: Box<dyn PubSubBackend>,
    provider: PubSubProvider,
}

impl PubSubClient {
    /// Create with memory backend
    pub fn memory() -> Self {
        PubSubClient {
            backend: Box::new(MemoryPubSub::new()),
            provider: PubSubProvider::Memory,
        }
    }

    /// Get provider
    pub fn provider(&self) -> PubSubProvider {
        self.provider
    }

    /// Publish message
    pub fn publish(&mut self, topic: &str, message: PubSubMessage) -> Result<String, String> {
        self.backend.publish(topic, message)
    }

    /// Create topic
    pub fn create_topic(&mut self, name: &str) -> Result<Topic, String> {
        self.backend.create_topic(name)
    }

    /// Delete topic
    pub fn delete_topic(&mut self, name: &str) -> Result<(), String> {
        self.backend.delete_topic(name)
    }

    /// List topics
    pub fn list_topics(&self) -> Vec<Topic> {
        self.backend.list_topics()
    }

    /// Subscribe
    pub fn subscribe(
        &mut self,
        topic: &str,
        subscription_id: &str,
    ) -> Result<Subscription, String> {
        self.backend.subscribe(topic, subscription_id)
    }

    /// Unsubscribe
    pub fn unsubscribe(&mut self, subscription_id: &str) -> Result<(), String> {
        self.backend.unsubscribe(subscription_id)
    }

    /// Receive messages
    pub fn receive(&self, subscription_id: &str, max_messages: usize) -> Vec<PubSubMessage> {
        self.backend.receive(subscription_id, max_messages)
    }
}

fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pubsub_provider_as_str() {
        assert_eq!(PubSubProvider::Memory.as_str(), "memory");
        assert_eq!(PubSubProvider::AwsSns.as_str(), "aws-sns");
    }

    #[test]
    fn test_message_metadata_new() {
        let meta = MessageMetadata::new("msg123");
        assert_eq!(meta.message_id, "msg123");
        assert!(meta.timestamp > 0);
    }

    #[test]
    fn test_message_metadata_with_attribute() {
        let meta = MessageMetadata::new("msg123")
            .with_attribute("source", "api")
            .with_attribute("version", "1.0");

        assert_eq!(meta.attributes.len(), 2);
        assert_eq!(meta.attributes.get("source"), Some(&"api".to_string()));
    }

    #[test]
    fn test_pubsub_message_new() {
        let msg = PubSubMessage::new(vec![1, 2, 3], "msg123");
        assert_eq!(msg.data, vec![1, 2, 3]);
        assert_eq!(msg.metadata.message_id, "msg123");
    }

    #[test]
    fn test_pubsub_message_from_string() {
        let msg = PubSubMessage::from_string("hello", "msg123");
        assert_eq!(msg.as_string(), "hello");
    }

    #[test]
    fn test_subscription_new() {
        let sub = Subscription::new("sub123", "topic_name");
        assert_eq!(sub.id, "sub123");
        assert_eq!(sub.topic, "topic_name");
    }

    #[test]
    fn test_topic_new() {
        let topic = Topic::new("users");
        assert_eq!(topic.name, "users");
        assert_eq!(topic.message_count, 0);
    }

    #[test]
    fn test_memory_pubsub_create_topic() {
        let mut pubsub = MemoryPubSub::new();
        let result = pubsub.create_topic("users");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "users");
    }

    #[test]
    fn test_memory_pubsub_create_duplicate_topic() {
        let mut pubsub = MemoryPubSub::new();
        pubsub.create_topic("users").unwrap();
        let result = pubsub.create_topic("users");
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_pubsub_publish() {
        let mut pubsub = MemoryPubSub::new();
        pubsub.create_topic("users").unwrap();

        let msg = PubSubMessage::from_string("user_created", "msg123");
        let result = pubsub.publish("users", msg);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "msg123");
    }

    #[test]
    fn test_memory_pubsub_list_topics() {
        let mut pubsub = MemoryPubSub::new();
        pubsub.create_topic("users").unwrap();
        pubsub.create_topic("posts").unwrap();

        let topics = pubsub.list_topics();
        assert_eq!(topics.len(), 2);
    }

    #[test]
    fn test_memory_pubsub_get_topic() {
        let mut pubsub = MemoryPubSub::new();
        pubsub.create_topic("users").unwrap();

        let topic = pubsub.get_topic("users");
        assert!(topic.is_some());
        assert_eq!(topic.unwrap().name, "users");
    }

    #[test]
    fn test_memory_pubsub_delete_topic() {
        let mut pubsub = MemoryPubSub::new();
        pubsub.create_topic("users").unwrap();

        let result = pubsub.delete_topic("users");
        assert!(result.is_ok());
        assert!(pubsub.get_topic("users").is_none());
    }

    #[test]
    fn test_memory_pubsub_subscribe() {
        let mut pubsub = MemoryPubSub::new();
        pubsub.create_topic("users").unwrap();

        let result = pubsub.subscribe("users", "sub123");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "sub123");
    }

    #[test]
    fn test_memory_pubsub_unsubscribe() {
        let mut pubsub = MemoryPubSub::new();
        pubsub.create_topic("users").unwrap();
        pubsub.subscribe("users", "sub123").unwrap();

        let result = pubsub.unsubscribe("sub123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_memory_pubsub_receive() {
        let mut pubsub = MemoryPubSub::new();
        pubsub.create_topic("users").unwrap();
        pubsub.subscribe("users", "sub123").unwrap();

        let msg1 = PubSubMessage::from_string("message1", "msg1");
        let msg2 = PubSubMessage::from_string("message2", "msg2");
        pubsub.publish("users", msg1).unwrap();
        pubsub.publish("users", msg2).unwrap();

        let received = pubsub.receive("sub123", 10);
        assert_eq!(received.len(), 2);
    }

    #[test]
    fn test_pubsub_client_memory() {
        let client = PubSubClient::memory();
        assert_eq!(client.provider(), PubSubProvider::Memory);
    }

    #[test]
    fn test_pubsub_client_publish() {
        let mut client = PubSubClient::memory();
        client.create_topic("users").unwrap();

        let msg = PubSubMessage::from_string("user_created", "msg123");
        let result = client.publish("users", msg);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pubsub_client_full_workflow() {
        let mut client = PubSubClient::memory();

        // Create topic
        client.create_topic("notifications").unwrap();

        // Subscribe
        client.subscribe("notifications", "sub1").unwrap();

        // Publish
        let msg = PubSubMessage::from_string("alert", "msg1");
        client.publish("notifications", msg).unwrap();

        // Receive
        let received = client.receive("sub1", 10);
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].as_string(), "alert");
    }
}
