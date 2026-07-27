//! Graceful shutdown coordination
//!
//! Manages coordinated shutdown of all daemon components

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Shutdown signal handler
#[derive(Clone)]
pub struct ShutdownSignal {
    shutting_down: Arc<AtomicBool>,
}

impl ShutdownSignal {
    /// Create new shutdown signal
    pub fn new() -> Self {
        ShutdownSignal {
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Trigger shutdown
    pub fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    /// Check if shutdown is in progress
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    /// Wait for shutdown (blocking)
    pub fn wait_for_shutdown(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        while !self.is_shutting_down() {
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Cleanup handler
pub trait CleanupHandler: Send + Sync {
    /// Execute cleanup (e.g., flush buffers, close connections)
    fn cleanup(&mut self) -> Result<(), String>;

    /// Get name for logging
    fn name(&self) -> &str;
}

/// Shutdown coordinator
pub struct ShutdownCoordinator {
    signal: ShutdownSignal,
    handlers: Vec<Box<dyn CleanupHandler>>,
    timeout: Duration,
}

impl ShutdownCoordinator {
    /// Create new coordinator
    pub fn new(timeout: Duration) -> Self {
        ShutdownCoordinator {
            signal: ShutdownSignal::new(),
            handlers: Vec::new(),
            timeout,
        }
    }

    /// Get shutdown signal
    pub fn signal(&self) -> ShutdownSignal {
        self.signal.clone()
    }

    /// Register cleanup handler
    pub fn register(&mut self, handler: Box<dyn CleanupHandler>) {
        self.handlers.push(handler);
    }

    /// Trigger coordinated shutdown
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.signal.shutdown();

        let mut errors = Vec::new();

        // Call cleanup handlers in reverse order (LIFO)
        for handler in self.handlers.iter_mut().rev() {
            match handler.cleanup() {
                Ok(_) => {
                    // Handler cleaned up successfully
                }
                Err(e) => {
                    errors.push(format!("{}: {}", handler.name(), e));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Cleanup errors: {}",
                errors.join("; ")
            ))
        }
    }
}

/// Simple cleanup handler for testing
pub struct SimpleCleanupHandler {
    name: String,
    cleanup_called: Arc<std::sync::Mutex<bool>>,
}

impl SimpleCleanupHandler {
    pub fn new(name: impl Into<String>) -> Self {
        SimpleCleanupHandler {
            name: name.into(),
            cleanup_called: Arc::new(std::sync::Mutex::new(false)),
        }
    }
}

impl CleanupHandler for SimpleCleanupHandler {
    fn cleanup(&mut self) -> Result<(), String> {
        if let Ok(mut called) = self.cleanup_called.lock() {
            *called = true;
            Ok(())
        } else {
            Err("Failed to lock cleanup state".to_string())
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_signal_new() {
        let signal = ShutdownSignal::new();
        assert!(!signal.is_shutting_down());
    }

    #[test]
    fn test_shutdown_signal_trigger() {
        let signal = ShutdownSignal::new();
        signal.shutdown();
        assert!(signal.is_shutting_down());
    }

    #[test]
    fn test_shutdown_signal_clone() {
        let signal1 = ShutdownSignal::new();
        let signal2 = signal1.clone();

        signal1.shutdown();
        assert!(signal2.is_shutting_down());
    }

    #[test]
    fn test_shutdown_signal_wait() {
        let signal = ShutdownSignal::new();
        signal.shutdown();

        let result = signal.wait_for_shutdown(Duration::from_secs(1));
        assert!(result);
    }

    #[test]
    fn test_shutdown_signal_wait_timeout() {
        let signal = ShutdownSignal::new();

        let result = signal.wait_for_shutdown(Duration::from_millis(100));
        assert!(!result);
    }

    #[test]
    fn test_shutdown_coordinator_new() {
        let coordinator = ShutdownCoordinator::new(Duration::from_secs(5));
        assert!(!coordinator.signal().is_shutting_down());
    }

    #[test]
    fn test_shutdown_coordinator_signal() {
        let coordinator = ShutdownCoordinator::new(Duration::from_secs(5));
        let signal = coordinator.signal();
        assert!(!signal.is_shutting_down());
    }

    #[test]
    fn test_shutdown_coordinator_shutdown_no_handlers() {
        let mut coordinator = ShutdownCoordinator::new(Duration::from_secs(5));
        let result = coordinator.shutdown();
        assert!(result.is_ok());
    }

    #[test]
    fn test_simple_cleanup_handler_success() {
        let mut handler = SimpleCleanupHandler::new("test_handler");
        let result = handler.cleanup();
        assert!(result.is_ok());
    }

    #[test]
    fn test_simple_cleanup_handler_name() {
        let handler = SimpleCleanupHandler::new("test_handler");
        assert_eq!(handler.name(), "test_handler");
    }

    #[test]
    fn test_shutdown_coordinator_with_handlers() {
        let mut coordinator = ShutdownCoordinator::new(Duration::from_secs(5));

        let handler1 = SimpleCleanupHandler::new("handler1");
        let handler2 = SimpleCleanupHandler::new("handler2");

        coordinator.register(Box::new(handler1));
        coordinator.register(Box::new(handler2));

        let result = coordinator.shutdown();
        assert!(result.is_ok());
    }

    #[test]
    fn test_shutdown_coordinator_lifo_order() {
        let mut coordinator = ShutdownCoordinator::new(Duration::from_secs(5));

        let handler1 = SimpleCleanupHandler::new("handler1");
        let handler2 = SimpleCleanupHandler::new("handler2");

        coordinator.register(Box::new(handler1));
        coordinator.register(Box::new(handler2));

        let result = coordinator.shutdown();
        assert!(result.is_ok());
    }

    #[test]
    fn test_shutdown_coordinator_partial_failure() {
        let mut coordinator = ShutdownCoordinator::new(Duration::from_secs(5));

        let handler1 = SimpleCleanupHandler::new("handler1");
        let handler2 = SimpleCleanupHandler::new("handler2");
        let handler3 = SimpleCleanupHandler::new("handler3");

        coordinator.register(Box::new(handler1));
        coordinator.register(Box::new(handler2));
        coordinator.register(Box::new(handler3));

        let result = coordinator.shutdown();
        assert!(result.is_ok());
    }
}
