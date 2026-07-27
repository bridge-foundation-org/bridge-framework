//! Service Registry - Enable inter-service discovery and communication
//!
//! The ServiceRegistry maintains a registry of all services running in the system,
//! enabling service-to-service discovery and cross-service HTTP calls.
//!
//! # Examples
//!
//! ```ignore
//! let registry = ServiceRegistry::new();
//! registry.register(Service::new("users", "localhost", 8001));
//! let service = registry.discover("users");
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Represents a service in the registry
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    /// Unique service identifier
    name: String,
    /// Host address (IP or hostname)
    host: String,
    /// Port number
    port: u16,
    /// Scheme (http, https)
    scheme: String,
    /// Service metadata
    metadata: HashMap<String, String>,
    /// Last heartbeat timestamp (Unix seconds)
    last_heartbeat: u64,
}

impl Service {
    /// Create a new service with default scheme (http)
    pub fn new(name: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Service {
            name: name.into(),
            host: host.into(),
            port,
            scheme: "http".to_string(),
            metadata: HashMap::new(),
            last_heartbeat: current_timestamp(),
        }
    }

    /// Create a new service with custom scheme
    pub fn with_scheme(
        name: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        scheme: impl Into<String>,
    ) -> Self {
        Service {
            name: name.into(),
            host: host.into(),
            port,
            scheme: scheme.into(),
            metadata: HashMap::new(),
            last_heartbeat: current_timestamp(),
        }
    }

    /// Get the full URL for this service
    pub fn url(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }

    /// Get service name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get host
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Get port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get scheme
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get metadata
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|v| v.as_str())
    }

    /// Update heartbeat
    pub fn heartbeat(&mut self) {
        self.last_heartbeat = current_timestamp();
    }

    /// Check if service is alive (heartbeat within 30 seconds)
    pub fn is_alive(&self) -> bool {
        let now = current_timestamp();
        now.saturating_sub(self.last_heartbeat) < 30
    }
}

/// Service Registry - stores and discovers services
pub struct ServiceRegistry {
    /// Thread-safe map of service name to Service
    services: Arc<Mutex<HashMap<String, Service>>>,
}

impl ServiceRegistry {
    /// Create a new empty service registry
    pub fn new() -> Self {
        ServiceRegistry {
            services: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a service in the registry
    pub fn register(&self, service: Service) {
        if let Ok(mut services) = self.services.lock() {
            services.insert(service.name.clone(), service);
        }
    }

    /// Deregister a service from the registry
    pub fn deregister(&self, name: &str) -> bool {
        if let Ok(mut services) = self.services.lock() {
            services.remove(name).is_some()
        } else {
            false
        }
    }

    /// Discover a service by name
    pub fn discover(&self, name: &str) -> Option<Service> {
        if let Ok(services) = self.services.lock() {
            services.get(name).cloned()
        } else {
            None
        }
    }

    /// Check if a service is registered
    pub fn contains(&self, name: &str) -> bool {
        if let Ok(services) = self.services.lock() {
            services.contains_key(name)
        } else {
            false
        }
    }

    /// List all registered services
    pub fn list_all(&self) -> Vec<Service> {
        if let Ok(services) = self.services.lock() {
            services.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// List services by filter
    pub fn list_filtered<F>(&self, predicate: F) -> Vec<Service>
    where
        F: Fn(&Service) -> bool,
    {
        if let Ok(services) = self.services.lock() {
            services
                .values()
                .filter(|s| predicate(s))
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Count total services
    pub fn count(&self) -> usize {
        if let Ok(services) = self.services.lock() {
            services.len()
        } else {
            0
        }
    }

    /// Update service heartbeat
    pub fn heartbeat(&self, name: &str) -> bool {
        if let Ok(mut services) = self.services.lock() {
            if let Some(service) = services.get_mut(name) {
                service.heartbeat();
                return true;
            }
        }
        false
    }

    /// Get alive services only
    pub fn list_alive(&self) -> Vec<Service> {
        self.list_filtered(|s| s.is_alive())
    }

    /// Clear all services
    pub fn clear(&self) {
        if let Ok(mut services) = self.services.lock() {
            services.clear();
        }
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ServiceRegistry {
    fn clone(&self) -> Self {
        ServiceRegistry {
            services: Arc::clone(&self.services),
        }
    }
}

/// Get current Unix timestamp in seconds
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_new() {
        let service = Service::new("users", "localhost", 8001);
        assert_eq!(service.name(), "users");
        assert_eq!(service.host(), "localhost");
        assert_eq!(service.port(), 8001);
        assert_eq!(service.scheme(), "http");
    }

    #[test]
    fn test_service_url() {
        let service = Service::new("users", "localhost", 8001);
        assert_eq!(service.url(), "http://localhost:8001");
    }

    #[test]
    fn test_service_with_scheme() {
        let service = Service::with_scheme("users", "api.example.com", 443, "https");
        assert_eq!(service.url(), "https://api.example.com:443");
    }

    #[test]
    fn test_service_metadata() {
        let service = Service::new("users", "localhost", 8001)
            .with_metadata("version", "1.0")
            .with_metadata("region", "us-east-1");

        assert_eq!(service.get_metadata("version"), Some("1.0"));
        assert_eq!(service.get_metadata("region"), Some("us-east-1"));
        assert_eq!(service.get_metadata("nonexistent"), None);
    }

    #[test]
    fn test_service_is_alive() {
        let service = Service::new("users", "localhost", 8001);
        assert!(service.is_alive());
    }

    #[test]
    fn test_service_heartbeat() {
        let mut service = Service::new("users", "localhost", 8001);
        let original = service.last_heartbeat;
        service.heartbeat();
        assert!(service.last_heartbeat >= original);
    }

    #[test]
    fn test_registry_new() {
        let registry = ServiceRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_register() {
        let registry = ServiceRegistry::new();
        let service = Service::new("users", "localhost", 8001);

        registry.register(service.clone());

        assert_eq!(registry.count(), 1);
        assert!(registry.contains("users"));
    }

    #[test]
    fn test_registry_discover() {
        let registry = ServiceRegistry::new();
        let service = Service::new("users", "localhost", 8001);

        registry.register(service.clone());

        let discovered = registry.discover("users");
        assert_eq!(discovered, Some(service));
    }

    #[test]
    fn test_registry_discover_not_found() {
        let registry = ServiceRegistry::new();

        let discovered = registry.discover("nonexistent");
        assert_eq!(discovered, None);
    }

    #[test]
    fn test_registry_deregister() {
        let registry = ServiceRegistry::new();
        let service = Service::new("users", "localhost", 8001);

        registry.register(service);
        assert!(registry.contains("users"));

        let removed = registry.deregister("users");
        assert!(removed);
        assert!(!registry.contains("users"));
    }

    #[test]
    fn test_registry_deregister_not_found() {
        let registry = ServiceRegistry::new();

        let removed = registry.deregister("nonexistent");
        assert!(!removed);
    }

    #[test]
    fn test_registry_multiple_services() {
        let registry = ServiceRegistry::new();

        registry.register(Service::new("users", "localhost", 8001));
        registry.register(Service::new("posts", "localhost", 8002));
        registry.register(Service::new("comments", "localhost", 8003));

        assert_eq!(registry.count(), 3);
        assert!(registry.contains("users"));
        assert!(registry.contains("posts"));
        assert!(registry.contains("comments"));
    }

    #[test]
    fn test_registry_list_all() {
        let registry = ServiceRegistry::new();

        registry.register(Service::new("users", "localhost", 8001));
        registry.register(Service::new("posts", "localhost", 8002));

        let all = registry.list_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_registry_list_filtered() {
        let registry = ServiceRegistry::new();

        registry.register(Service::new("users", "localhost", 8001));
        registry.register(Service::new("posts", "localhost", 8002));
        registry.register(Service::new("api", "localhost", 9000));

        let filtered = registry.list_filtered(|s| s.port() < 8500);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_registry_clear() {
        let registry = ServiceRegistry::new();

        registry.register(Service::new("users", "localhost", 8001));
        registry.register(Service::new("posts", "localhost", 8002));

        assert_eq!(registry.count(), 2);

        registry.clear();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_heartbeat() {
        let registry = ServiceRegistry::new();
        let service = Service::new("users", "localhost", 8001);

        registry.register(service);
        assert!(registry.heartbeat("users"));
        assert!(!registry.heartbeat("nonexistent"));
    }

    #[test]
    fn test_registry_list_alive() {
        let registry = ServiceRegistry::new();

        registry.register(Service::new("users", "localhost", 8001));
        registry.register(Service::new("posts", "localhost", 8002));

        let alive = registry.list_alive();
        assert_eq!(alive.len(), 2);
    }

    #[test]
    fn test_registry_clone() {
        let registry = ServiceRegistry::new();
        registry.register(Service::new("users", "localhost", 8001));

        let registry_clone = registry.clone();
        assert_eq!(registry_clone.count(), 1);
        assert!(registry_clone.contains("users"));
    }

    #[test]
    fn test_registry_thread_safe() {
        use std::thread;

        let registry = Arc::new(ServiceRegistry::new());
        let mut handles = vec![];

        // Spawn 5 threads that register services
        for i in 0..5 {
            let reg = Arc::clone(&registry);
            let handle = thread::spawn(move || {
                let service = Service::new(
                    format!("service_{}", i),
                    "localhost",
                    8000 + i as u16,
                );
                reg.register(service);
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(registry.count(), 5);
    }

    #[test]
    fn test_registry_overwrite_service() {
        let registry = ServiceRegistry::new();

        let service1 = Service::new("users", "localhost", 8001);
        registry.register(service1);

        let service2 = Service::new("users", "localhost", 9001);
        registry.register(service2);

        assert_eq!(registry.count(), 1);

        let discovered = registry.discover("users").unwrap();
        assert_eq!(discovered.port(), 9001);
    }

    #[test]
    fn test_service_equality() {
        let service1 = Service::new("users", "localhost", 8001);
        let service2 = Service::new("users", "localhost", 8001);

        // Note: equality compares all fields including heartbeat
        // so they may not be equal if created at different times
        assert_eq!(service1.name(), service2.name());
        assert_eq!(service1.host(), service2.host());
        assert_eq!(service1.port(), service2.port());
    }

    #[test]
    fn test_registry_default() {
        let registry = ServiceRegistry::default();
        assert_eq!(registry.count(), 0);
    }
}
