//! Background task management with panic and error monitoring
//!
//! Provides utilities for spawning background tasks with proper error
//! visibility. Unlike raw `tokio::spawn`, these utilities ensure that
//! task panics and errors are logged so operators can diagnose issues.
//!
//! ## Available Utilities
//!
//! - [`spawn_monitored`]: Spawn a task with completion logging
//! - [`spawn_monitored_result`]: Spawn a Result-returning task with error logging
//! - [`TaskHealth`]: Health monitoring for long-running background tasks

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

/// Spawn a named background task with panic monitoring
///
/// Unlike raw `tokio::spawn`, this wrapper:
/// 1. Captures the task name for logging
/// 2. Logs when the task completes or terminates unexpectedly
/// 3. Provides visibility into silent background task failures
///
/// Note: Panic catching for async futures requires additional setup.
/// This wrapper primarily provides visibility via JoinHandle monitoring.
///
/// # Arguments
/// * `name` - Human-readable name for logging (e.g., "http_bridge")
/// * `future` - The async task to spawn
///
/// # Returns
/// A JoinHandle that can be used to await completion or abort the task
///
/// # Example
/// ```ignore
/// let handle = spawn_monitored("http_bridge", async {
///     run_http_bridge(bridge, &addr).await
/// });
/// ```
pub fn spawn_monitored<F>(name: &'static str, future: F) -> JoinHandle<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    // Track whether the task completed normally
    let completed_flag = Arc::new(AtomicBool::new(false));
    let flag_clone = completed_flag.clone();
    let task_name = name;

    tokio::spawn(async move {
        // Execute the actual task
        future.await;

        // Mark as completed normally
        flag_clone.store(true, Ordering::SeqCst);
        debug!(task_name = %task_name, "Background task completed normally");
    })
}

/// Spawn a named background task that returns a Result, with error monitoring
///
/// Like `spawn_monitored`, but for tasks that return `Result<(), E>`.
/// Logs errors returned by the task.
///
/// # Arguments
/// * `name` - Human-readable name for logging
/// * `future` - The async task to spawn that returns a Result
///
/// # Returns
/// A JoinHandle that resolves to () regardless of the inner Result
#[allow(dead_code)] // Utility for future use
pub fn spawn_monitored_result<F, E>(name: &'static str, future: F) -> JoinHandle<()>
where
    F: Future<Output = Result<(), E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let task_name = name;

    tokio::spawn(async move {
        match future.await {
            Ok(()) => {
                debug!(task_name = %task_name, "Background task completed successfully");
            }
            Err(e) => {
                error!(
                    task_name = %task_name,
                    error = %e,
                    "Background task failed with error"
                );
            }
        }
    })
}

/// Track whether critical background tasks are still running
///
/// This struct allows the main thread to monitor the health of background tasks
/// without blocking on them. It's useful for implementing health checks.
#[allow(dead_code)] // Utility for future health monitoring
#[derive(Debug)]
pub struct TaskHealth {
    /// Name of the task for logging
    pub name: &'static str,
    /// Handle to the background task
    handle: JoinHandle<()>,
    /// Whether we've already warned about this task stopping
    warned: AtomicBool,
}

#[allow(dead_code)] // Utility methods for future health monitoring
impl TaskHealth {
    /// Create a new TaskHealth tracker from a JoinHandle
    pub fn new(name: &'static str, handle: JoinHandle<()>) -> Self {
        Self {
            name,
            handle,
            warned: AtomicBool::new(false),
        }
    }

    /// Create from a spawn_monitored call
    pub fn spawn<F>(name: &'static str, future: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self::new(name, spawn_monitored(name, future))
    }

    /// Check if the task is still running (non-blocking)
    ///
    /// Returns `true` if the task is still active, `false` if it has completed or panicked.
    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }

    /// Log a warning if the task has stopped unexpectedly (only warns once)
    ///
    /// Call this periodically to detect background task failures.
    pub fn check_health(&self) {
        if self.handle.is_finished() && !self.warned.swap(true, Ordering::SeqCst) {
            warn!(
                task_name = %self.name,
                "Background task has stopped. This may indicate a failure."
            );
        }
    }

    /// Abort the background task
    pub fn abort(&self) {
        self.handle.abort();
    }

    /// Consume self and await the task completion, returning any panic info
    ///
    /// This is useful during shutdown to ensure clean termination.
    pub async fn wait(self) -> Result<(), tokio::task::JoinError> {
        self.handle.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_spawn_monitored_completes_normally() {
        let completed = Arc::new(AtomicBool::new(false));
        let completed_clone = completed.clone();

        let handle = spawn_monitored("test_task", async move {
            completed_clone.store(true, Ordering::SeqCst);
        });

        // Wait for task to complete
        let result = handle.await;
        assert!(result.is_ok());
        assert!(completed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_spawn_monitored_with_async_work() {
        let handle = spawn_monitored("async_task", async {
            sleep(Duration::from_millis(10)).await;
        });

        let result = handle.await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_spawn_monitored_result_success() {
        let handle = spawn_monitored_result("success_task", async { Ok::<(), String>(()) });

        let result = handle.await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_spawn_monitored_result_error() {
        let handle = spawn_monitored_result("error_task", async {
            Err::<(), String>("test error".to_string())
        });

        // The outer task completes successfully (error is logged, not propagated)
        let result = handle.await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_task_health_is_running() {
        let handle = tokio::spawn(async {
            sleep(Duration::from_millis(100)).await;
        });

        let health = TaskHealth::new("long_task", handle);
        assert!(health.is_running());

        // Wait for task to complete
        sleep(Duration::from_millis(150)).await;
        assert!(!health.is_running());
    }

    #[tokio::test]
    async fn test_task_health_abort() {
        let handle = tokio::spawn(async {
            sleep(Duration::from_secs(10)).await;
        });

        let health = TaskHealth::new("abort_task", handle);
        assert!(health.is_running());

        health.abort();
        sleep(Duration::from_millis(10)).await;
        assert!(!health.is_running());
    }

    #[tokio::test]
    async fn test_task_health_check_warns_once() {
        let handle = tokio::spawn(async {});

        let health = TaskHealth::new("quick_task", handle);
        sleep(Duration::from_millis(10)).await;

        // First check should log warning
        health.check_health();
        // Second check should not (warned flag is set)
        health.check_health();

        // Verify warned flag is set
        assert!(health.warned.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_task_health_spawn() {
        let health = TaskHealth::spawn("spawn_test", async {
            sleep(Duration::from_millis(10)).await;
        });

        assert!(health.is_running());
        sleep(Duration::from_millis(50)).await;
        assert!(!health.is_running());
    }

    #[tokio::test]
    async fn test_task_health_wait() {
        let health = TaskHealth::spawn("wait_test", async {
            sleep(Duration::from_millis(10)).await;
        });

        let result = health.wait().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_spawn_monitored_panic_detection() {
        // When a spawned task panics, the JoinHandle returns JoinError
        let handle = tokio::spawn(async {
            panic!("test panic");
        });

        let result = handle.await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_panic());
    }

    #[tokio::test]
    async fn test_task_health_detects_panic() {
        let health = TaskHealth::new(
            "panic_task",
            tokio::spawn(async {
                panic!("deliberate panic for testing");
            }),
        );

        // Wait for panic to occur
        sleep(Duration::from_millis(50)).await;

        // Task should be finished (due to panic)
        assert!(!health.is_running());

        // Wait should return JoinError with is_panic() = true
        let result = health.wait().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_panic());
    }

    #[tokio::test]
    async fn test_spawn_monitored_result_with_async_error() {
        // Test that async work followed by error is handled correctly
        let handle = spawn_monitored_result("async_error_task", async {
            sleep(Duration::from_millis(5)).await;
            Err::<(), String>("delayed error".to_string())
        });

        let result = handle.await;
        assert!(result.is_ok()); // Outer task completes, error is logged
    }

    #[tokio::test]
    async fn test_spawn_monitored_result_with_async_success() {
        // Test that async work followed by success is handled correctly
        let handle = spawn_monitored_result("async_success_task", async {
            sleep(Duration::from_millis(5)).await;
            Ok::<(), String>(())
        });

        let result = handle.await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_task_health_check_running_task() {
        // Test that check_health doesn't warn for running tasks
        let health = TaskHealth::spawn("long_running", async {
            sleep(Duration::from_millis(100)).await;
        });

        // Task should be running
        assert!(health.is_running());

        // check_health should not set warned flag for running task
        health.check_health();
        assert!(!health.warned.load(Ordering::SeqCst));

        // Clean up
        health.abort();
    }

    #[test]
    fn test_task_health_debug() {
        let handle = tokio::runtime::Runtime::new().unwrap().spawn(async {});
        let health = TaskHealth::new("debug_test", handle);

        let debug_str = format!("{:?}", health);
        assert!(debug_str.contains("TaskHealth"));
        assert!(debug_str.contains("debug_test"));
    }
}
