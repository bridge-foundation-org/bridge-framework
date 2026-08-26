//! Service Struct & Dependency Injection
//!
//! Enables service structs with field injection and lifecycle management.
//! Services can define Init and Shutdown methods for setup/teardown.

// Parts of this module are forward-scaffolding: their public API is
// intentionally ahead of its call sites. Trim this allow item-by-item as the
// dead surface shrinks.
#![allow(dead_code)]

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Represents a service that can be instantiated with dependency injection
pub trait Service: Send + Sync {
    /// Initialize the service
    fn init(&mut self) -> Result<(), String>;

    /// Shutdown the service
    fn shutdown(&mut self) -> Result<(), String>;

    /// Get the service name
    fn name(&self) -> &str;
}

/// Field metadata for dependency injection
#[derive(Clone, Debug)]
pub struct FieldDef {
    /// Field name
    pub name: String,
    /// Field type ID
    pub type_id: TypeId,
    /// Whether field is required
    pub required: bool,
    /// Field tags (metadata)
    pub tags: HashMap<String, String>,
}

impl FieldDef {
    /// Create a new field definition
    pub fn new(name: impl Into<String>, type_id: TypeId) -> Self {
        FieldDef {
            name: name.into(),
            type_id,
            required: true,
            tags: HashMap::new(),
        }
    }

    /// Mark field as optional
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// Add tag metadata
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }
}

/// Service definition - describes a service and its fields
#[derive(Clone)]
pub struct ServiceDef {
    /// Service name
    pub name: String,
    /// Fields to inject
    pub fields: Vec<FieldDef>,
    /// Service constructor
    pub constructor: SharedInstanceConstructor,
}

impl ServiceDef {
    /// Create a new service definition
    pub fn new(name: impl Into<String>) -> Self {
        ServiceDef {
            name: name.into(),
            fields: Vec::new(),
            constructor: Arc::new(|| Box::new(())),
        }
    }

    /// Add a field definition
    pub fn field(mut self, field: FieldDef) -> Self {
        self.fields.push(field);
        self
    }

    /// Set the constructor
    pub fn with_constructor<F>(mut self, constructor: F) -> Self
    where
        F: Fn() -> BoxedService + Send + Sync + 'static,
    {
        self.constructor = Arc::new(constructor);
        self
    }
}

impl std::fmt::Debug for ServiceDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceDef")
            .field("name", &self.name)
            .field("fields", &self.fields)
            .finish()
    }
}

/// Type-erased service instance restricted to thread-safe payloads.
type BoxedService = Box<dyn Any + Send + Sync>;
/// Constructor closure producing a [`BoxedService`].
type SharedInstanceConstructor = Arc<dyn Fn() -> BoxedService + Send + Sync>;
/// Shared handle to a constructed singleton.
type SharedInstance = Arc<Mutex<BoxedService>>;

/// Dependency container for service injection
pub struct ServiceContainer {
    /// Registered service definitions
    services: Arc<Mutex<HashMap<String, ServiceDef>>>,
    /// Singleton instances
    instances: Arc<Mutex<HashMap<String, SharedInstance>>>,
}

impl ServiceContainer {
    /// Create a new empty service container
    pub fn new() -> Self {
        ServiceContainer {
            services: Arc::new(Mutex::new(HashMap::new())),
            instances: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a service definition
    pub fn register(&self, def: ServiceDef) -> Result<(), String> {
        if let Ok(mut services) = self.services.lock() {
            if services.contains_key(&def.name) {
                return Err(format!("Service '{}' already registered", def.name));
            }
            services.insert(def.name.clone(), def);
            Ok(())
        } else {
            Err("Failed to acquire lock".to_string())
        }
    }

    /// Get a service definition
    pub fn get_service(&self, name: &str) -> Result<Option<ServiceDef>, String> {
        if let Ok(services) = self.services.lock() {
            Ok(services.get(name).cloned())
        } else {
            Err("Failed to acquire lock".to_string())
        }
    }

    /// Resolve (instantiate) a service
    pub fn resolve(&self, name: &str) -> Result<SharedInstance, String> {
        // Check if singleton already exists
        if let Ok(instances) = self.instances.lock() {
            if let Some(instance) = instances.get(name) {
                return Ok(Arc::clone(instance));
            }
        }

        // Get service definition
        let def = self
            .get_service(name)?
            .ok_or_else(|| format!("Service '{}' not found", name))?;

        // Create new instance
        let instance = (def.constructor)();
        let instance = Arc::new(Mutex::new(instance));

        // Store in instances
        if let Ok(mut instances) = self.instances.lock() {
            instances.insert(name.to_string(), Arc::clone(&instance));
        }

        Ok(instance)
    }

    /// List all registered services
    pub fn list_services(&self) -> Result<Vec<String>, String> {
        if let Ok(services) = self.services.lock() {
            Ok(services.keys().cloned().collect())
        } else {
            Err("Failed to acquire lock".to_string())
        }
    }

    /// Clear all registered services and instances
    pub fn clear(&self) -> Result<(), String> {
        if let Ok(mut services) = self.services.lock() {
            services.clear();
        }
        if let Ok(mut instances) = self.instances.lock() {
            instances.clear();
        }
        Ok(())
    }

    /// Get all resolved instances
    pub fn get_instances(&self) -> Result<Vec<String>, String> {
        if let Ok(instances) = self.instances.lock() {
            Ok(instances.keys().cloned().collect())
        } else {
            Err("Failed to acquire lock".to_string())
        }
    }
}

impl Default for ServiceContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ServiceContainer {
    fn clone(&self) -> Self {
        ServiceContainer {
            services: Arc::clone(&self.services),
            instances: Arc::clone(&self.instances),
        }
    }
}

/// Lifecycle hooks for service initialization and shutdown
pub struct Lifecycle {
    /// Init callback
    pub init: Option<Box<dyn Fn() -> Result<(), String> + Send + Sync>>,
    /// Shutdown callback
    pub shutdown: Option<Box<dyn Fn() -> Result<(), String> + Send + Sync>>,
}

impl Lifecycle {
    /// Create a new lifecycle without hooks
    pub fn new() -> Self {
        Lifecycle {
            init: None,
            shutdown: None,
        }
    }

    /// Set init hook
    pub fn with_init<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Result<(), String> + Send + Sync + 'static,
    {
        self.init = Some(Box::new(f));
        self
    }

    /// Set shutdown hook
    pub fn with_shutdown<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Result<(), String> + Send + Sync + 'static,
    {
        self.shutdown = Some(Box::new(f));
        self
    }

    /// Call init hook if it exists
    pub fn call_init(&self) -> Result<(), String> {
        if let Some(init) = &self.init {
            init()
        } else {
            Ok(())
        }
    }

    /// Call shutdown hook if it exists
    pub fn call_shutdown(&self) -> Result<(), String> {
        if let Some(shutdown) = &self.shutdown {
            shutdown()
        } else {
            Ok(())
        }
    }
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_def_new() {
        let field = FieldDef::new("name", TypeId::of::<String>());
        assert_eq!(field.name, "name");
        assert!(field.required);
    }

    #[test]
    fn test_field_def_optional() {
        let field = FieldDef::new("name", TypeId::of::<String>()).optional();
        assert!(!field.required);
    }

    #[test]
    fn test_field_def_tag() {
        let field = FieldDef::new("name", TypeId::of::<String>())
            .tag("validate", "email")
            .tag("nullable", "false");

        assert_eq!(field.tags.len(), 2);
        assert_eq!(field.tags.get("validate"), Some(&"email".to_string()));
    }

    #[test]
    fn test_service_def_new() {
        let def = ServiceDef::new("UserService");
        assert_eq!(def.name, "UserService");
        assert!(def.fields.is_empty());
    }

    #[test]
    fn test_service_def_field() {
        let field = FieldDef::new("db", TypeId::of::<String>());
        let def = ServiceDef::new("UserService").field(field);

        assert_eq!(def.fields.len(), 1);
        assert_eq!(def.fields[0].name, "db");
    }

    #[test]
    fn test_service_def_multiple_fields() {
        let def = ServiceDef::new("UserService")
            .field(FieldDef::new("db", TypeId::of::<String>()))
            .field(FieldDef::new("logger", TypeId::of::<i32>()))
            .field(FieldDef::new("cache", TypeId::of::<u64>()));

        assert_eq!(def.fields.len(), 3);
    }

    #[test]
    fn test_service_container_new() {
        let container = ServiceContainer::new();
        assert_eq!(container.list_services().unwrap().len(), 0);
    }

    #[test]
    fn test_service_container_register() {
        let container = ServiceContainer::new();
        let def = ServiceDef::new("UserService");

        assert!(container.register(def).is_ok());
        assert_eq!(container.list_services().unwrap().len(), 1);
    }

    #[test]
    fn test_service_container_register_duplicate() {
        let container = ServiceContainer::new();
        let def = ServiceDef::new("UserService");

        assert!(container.register(def.clone()).is_ok());
        assert!(container.register(def).is_err());
    }

    #[test]
    fn test_service_container_get_service() {
        let container = ServiceContainer::new();
        let def = ServiceDef::new("UserService");

        container.register(def).unwrap();
        let retrieved = container.get_service("UserService").unwrap();

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "UserService");
    }

    #[test]
    fn test_service_container_get_service_not_found() {
        let container = ServiceContainer::new();
        let retrieved = container.get_service("NonExistent").unwrap();

        assert!(retrieved.is_none());
    }

    #[test]
    fn test_service_container_resolve() {
        let container = ServiceContainer::new();
        let def = ServiceDef::new("UserService").with_constructor(|| Box::new("test_instance"));

        container.register(def).unwrap();
        let instance = container.resolve("UserService").unwrap();

        assert!(instance.lock().is_ok());
    }

    #[test]
    fn test_service_container_resolve_singleton() {
        let container = ServiceContainer::new();
        let def = ServiceDef::new("UserService").with_constructor(|| Box::new("test_instance"));

        container.register(def).unwrap();

        let instance1 = container.resolve("UserService").unwrap();
        let instance2 = container.resolve("UserService").unwrap();

        // Should be the same Arc reference
        assert!(Arc::ptr_eq(&instance1, &instance2));
    }

    #[test]
    fn test_service_container_list_services() {
        let container = ServiceContainer::new();

        container.register(ServiceDef::new("UserService")).unwrap();
        container.register(ServiceDef::new("PostService")).unwrap();
        container
            .register(ServiceDef::new("CommentService"))
            .unwrap();

        let services = container.list_services().unwrap();
        assert_eq!(services.len(), 3);
    }

    #[test]
    fn test_service_container_clear() {
        let container = ServiceContainer::new();

        container.register(ServiceDef::new("UserService")).unwrap();
        container.register(ServiceDef::new("PostService")).unwrap();

        assert_eq!(container.list_services().unwrap().len(), 2);

        container.clear().unwrap();
        assert_eq!(container.list_services().unwrap().len(), 0);
    }

    #[test]
    fn test_service_container_get_instances() {
        let container = ServiceContainer::new();
        let def = ServiceDef::new("UserService").with_constructor(|| Box::new("instance"));

        container.register(def).unwrap();
        assert_eq!(container.get_instances().unwrap().len(), 0);

        container.resolve("UserService").unwrap();
        assert_eq!(container.get_instances().unwrap().len(), 1);
    }

    #[test]
    fn test_service_container_clone() {
        let container = ServiceContainer::new();
        let def = ServiceDef::new("UserService").with_constructor(|| Box::new("instance"));

        container.register(def).unwrap();

        let cloned = container.clone();
        assert_eq!(cloned.list_services().unwrap().len(), 1);
    }

    #[test]
    fn test_lifecycle_new() {
        let lifecycle = Lifecycle::new();
        assert!(lifecycle.init.is_none());
        assert!(lifecycle.shutdown.is_none());
    }

    #[test]
    fn test_lifecycle_with_init() {
        let lifecycle = Lifecycle::new().with_init(|| Ok(()));
        assert!(lifecycle.init.is_some());
    }

    #[test]
    fn test_lifecycle_with_shutdown() {
        let lifecycle = Lifecycle::new().with_shutdown(|| Ok(()));
        assert!(lifecycle.shutdown.is_some());
    }

    #[test]
    fn test_lifecycle_call_init() {
        let lifecycle = Lifecycle::new().with_init(|| Ok(()));
        assert!(lifecycle.call_init().is_ok());
    }

    #[test]
    fn test_lifecycle_call_init_error() {
        let lifecycle = Lifecycle::new().with_init(|| Err("init failed".to_string()));
        assert!(lifecycle.call_init().is_err());
    }

    #[test]
    fn test_lifecycle_call_shutdown() {
        let lifecycle = Lifecycle::new().with_shutdown(|| Ok(()));
        assert!(lifecycle.call_shutdown().is_ok());
    }

    #[test]
    fn test_lifecycle_call_shutdown_error() {
        let lifecycle = Lifecycle::new().with_shutdown(|| Err("shutdown failed".to_string()));
        assert!(lifecycle.call_shutdown().is_err());
    }

    #[test]
    fn test_lifecycle_no_hooks() {
        let lifecycle = Lifecycle::new();
        assert!(lifecycle.call_init().is_ok());
        assert!(lifecycle.call_shutdown().is_ok());
    }

    #[test]
    fn test_lifecycle_full_chain() {
        let lifecycle = Lifecycle::new()
            .with_init(|| Ok(()))
            .with_shutdown(|| Ok(()));

        assert!(lifecycle.call_init().is_ok());
        assert!(lifecycle.call_shutdown().is_ok());
    }

    #[test]
    fn test_service_def_builder() {
        let def = ServiceDef::new("UserService")
            .field(FieldDef::new("db", TypeId::of::<String>()))
            .field(FieldDef::new("logger", TypeId::of::<i32>()).optional())
            .with_constructor(|| Box::new("service"));

        assert_eq!(def.name, "UserService");
        assert_eq!(def.fields.len(), 2);
        assert!(def.fields[0].required);
        assert!(!def.fields[1].required);
    }

    #[test]
    fn test_container_multiple_services() {
        let container = ServiceContainer::new();

        let user_service =
            ServiceDef::new("UserService").with_constructor(|| Box::new("user_instance"));
        let post_service =
            ServiceDef::new("PostService").with_constructor(|| Box::new("post_instance"));

        container.register(user_service).unwrap();
        container.register(post_service).unwrap();

        let user = container.resolve("UserService").unwrap();
        let post = container.resolve("PostService").unwrap();

        assert!(user.lock().is_ok());
        assert!(post.lock().is_ok());
    }
}
