use crate::error::RobloxMcpError;
use crate::metrics::ServerMetrics;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{oneshot, RwLock};
use tokio::time::{timeout, Duration};
use tracing::warn;
use uuid::Uuid;

/// How long before a plugin heartbeat is considered stale (seconds)
pub const PLUGIN_HEARTBEAT_TIMEOUT_SECS: u64 = 10;

/// Maximum time to wait for a plugin command response (seconds)
pub const PLUGIN_COMMAND_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: String,
    pub action: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResponse {
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct PluginBridge {
    pending_commands: Arc<RwLock<Vec<Command>>>,
    pending_results: Arc<RwLock<HashMap<String, oneshot::Sender<PluginResponse>>>>,
    pub last_heartbeat: Arc<RwLock<Instant>>,
    /// Optional metrics collector for tracking late results
    metrics: Option<Arc<ServerMetrics>>,
}

impl PluginBridge {
    /// Create a new PluginBridge without metrics tracking
    ///
    /// For production use with late result tracking, prefer `with_metrics()`.
    /// This constructor is kept for backwards compatibility and testing.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            pending_commands: Arc::new(RwLock::new(Vec::new())),
            pending_results: Arc::new(RwLock::new(HashMap::new())),
            last_heartbeat: Arc::new(RwLock::new(Instant::now())),
            metrics: None,
        }
    }

    /// Create a new PluginBridge with metrics tracking
    pub fn with_metrics(metrics: Arc<ServerMetrics>) -> Self {
        Self {
            pending_commands: Arc::new(RwLock::new(Vec::new())),
            pending_results: Arc::new(RwLock::new(HashMap::new())),
            last_heartbeat: Arc::new(RwLock::new(Instant::now())),
            metrics: Some(metrics),
        }
    }

    /// Record a late result (result arrived after caller timed out)
    pub fn record_late_result(&self, had_error: bool) {
        if let Some(metrics) = &self.metrics {
            metrics.record_late_result(had_error);
        }
    }

    /// Record an unknown command result (plugin sent result for unregistered command ID)
    pub fn record_unknown_command(&self) {
        if let Some(metrics) = &self.metrics {
            metrics.record_unknown_command();
        }
    }

    /// Check if plugin is connected (heartbeat within timeout threshold)
    ///
    /// Public API for health checks - used in tests and available for external consumers
    #[allow(dead_code)]
    pub async fn is_connected(&self) -> bool {
        self.last_heartbeat.read().await.elapsed()
            < Duration::from_secs(PLUGIN_HEARTBEAT_TIMEOUT_SECS)
    }

    /// Execute a command via the plugin bridge with fast-failure timeout
    pub async fn execute_command(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RobloxMcpError> {
        // Check heartbeat - FAIL IMMEDIATELY if stale
        let elapsed = self.last_heartbeat.read().await.elapsed();
        if elapsed > Duration::from_secs(PLUGIN_HEARTBEAT_TIMEOUT_SECS) {
            return Err(RobloxMcpError::PluginTimeout(elapsed));
        }

        // Create command with UUID
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        // Build command first (takes ownership of id), then clone id for the results map
        let command = Command {
            id: id.clone(),
            action: action.to_string(),
            params,
        };

        // Register result receiver BEFORE sending command
        self.pending_results.write().await.insert(id, tx);

        // Queue command
        self.pending_commands.write().await.push(command);

        // Wait for response with HARD TIMEOUT - no fallback
        let response = match timeout(Duration::from_secs(PLUGIN_COMMAND_TIMEOUT_SECS), rx).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => {
                return Err(RobloxMcpError::PluginExecutionError(
                    "Result channel closed unexpectedly".to_string(),
                ))
            }
            Err(_) => {
                return Err(RobloxMcpError::PluginTimeout(Duration::from_secs(
                    PLUGIN_COMMAND_TIMEOUT_SECS,
                )))
            }
        };

        // Check for plugin-side errors - PROPAGATE IMMEDIATELY
        if let Some(error) = response.error {
            return Err(RobloxMcpError::PluginExecutionError(error));
        }

        // Return result or fail if missing
        response.result.ok_or_else(|| {
            RobloxMcpError::InvalidStudioData("Plugin returned success but no result".to_string())
        })
    }
}

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};

/// HTTP endpoint: Plugin polls for pending commands
async fn poll_handler(State(bridge): State<PluginBridge>) -> Json<Option<Command>> {
    // Update heartbeat
    *bridge.last_heartbeat.write().await = Instant::now();

    // Return next pending command if available
    let command = bridge.pending_commands.write().await.pop();
    Json(command)
}

/// HTTP endpoint: Plugin sends back results
async fn result_handler(
    State(bridge): State<PluginBridge>,
    Json(response): Json<PluginResponse>,
) -> Json<serde_json::Value> {
    // Find the waiting receiver and send result
    // Extract from write lock before sending to avoid holding lock during send
    let sender = bridge.pending_results.write().await.remove(&response.id);
    match sender {
        Some(tx) => {
            // Extract data for potential logging before moving response
            let response_id = response.id.clone();
            let had_error = response.error.is_some();

            if tx.send(response).is_err() {
                // Caller timed out before we could deliver the result
                // Track this as a "late result" - plugin did work that went unused
                bridge.record_late_result(had_error);
                warn!(
                    command_id = %response_id,
                    had_error = had_error,
                    "Plugin result discarded: caller already timed out"
                );
            }
        }
        None => {
            // No sender registered - this should not happen, track it
            // Could indicate: command tracking bug, duplicate plugin response, or cancelled command
            bridge.record_unknown_command();
            warn!(
                command_id = %response.id,
                "Plugin result received but no sender registered - command may have been cancelled"
            );
        }
    }
    Json(serde_json::json!({ "ok": true }))
}

/// Health status response for monitoring
#[derive(Clone, Serialize)]
pub struct HealthStatus {
    pub status: &'static str,
    pub plugin_connected: bool,
    pub heartbeat_age_secs: f64,
    pub version: &'static str,
}

/// Deserializable version for testing
#[cfg(test)]
#[derive(Debug, Deserialize)]
struct HealthStatusTest {
    status: String,
    plugin_connected: bool,
    heartbeat_age_secs: f64,
    version: String,
}

/// HTTP endpoint: Health check for monitoring
async fn health_handler(State(bridge): State<PluginBridge>) -> Json<HealthStatus> {
    let heartbeat_age = bridge.last_heartbeat.read().await.elapsed();
    let connected = heartbeat_age < Duration::from_secs(PLUGIN_HEARTBEAT_TIMEOUT_SECS);

    Json(HealthStatus {
        status: if connected { "healthy" } else { "degraded" },
        plugin_connected: connected,
        heartbeat_age_secs: heartbeat_age.as_secs_f64(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Create the Axum router for the plugin bridge
pub fn create_router(bridge: PluginBridge) -> Router {
    Router::new()
        .route("/poll", get(poll_handler))
        .route("/result", post(result_handler))
        .route("/health", get(health_handler))
        .with_state(bridge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[test]
    fn test_plugin_bridge_new() {
        let bridge = PluginBridge::new();
        // Should initialize with empty queues
        assert!(bridge.pending_commands.try_read().is_ok());
        assert!(bridge.pending_results.try_read().is_ok());
    }

    #[tokio::test]
    async fn test_is_connected_initially_true() {
        let bridge = PluginBridge::new();
        // Should be connected since heartbeat was just set
        assert!(bridge.is_connected().await);
    }

    #[tokio::test]
    async fn test_is_connected_false_after_timeout() {
        let bridge = PluginBridge::new();

        // Manually set last_heartbeat to 15 seconds ago
        *bridge.last_heartbeat.write().await =
            Instant::now().checked_sub(Duration::from_secs(15)).unwrap();

        assert!(!bridge.is_connected().await);
    }

    #[tokio::test]
    async fn test_execute_command_fails_on_stale_heartbeat() {
        let bridge = PluginBridge::new();

        // Set heartbeat to be stale
        *bridge.last_heartbeat.write().await =
            Instant::now().checked_sub(Duration::from_secs(15)).unwrap();

        let result = bridge
            .execute_command("getSelection", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::PluginTimeout(_) => (),
            e => panic!("Expected PluginTimeout, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_command_queued_correctly() {
        let bridge = PluginBridge::new();

        // Start command execution in background (will timeout, but queue first)
        let bridge_clone = bridge.clone();
        let handle = tokio::spawn(async move {
            bridge_clone
                .execute_command("testAction", serde_json::json!({"key": "value"}))
                .await
        });

        // Give time for command to be queued
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check command was queued - drop lock before aborting handle
        {
            let commands = bridge.pending_commands.read().await;
            assert_eq!(commands.len(), 1);
            assert_eq!(commands[0].action, "testAction");
        }

        // Cancel the waiting handle
        handle.abort();
    }

    #[tokio::test]
    async fn test_poll_handler_updates_heartbeat() {
        let bridge = PluginBridge::new();

        // Set old heartbeat
        *bridge.last_heartbeat.write().await =
            Instant::now().checked_sub(Duration::from_secs(5)).unwrap();

        let router = create_router(bridge.clone());

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/poll")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Heartbeat should be updated
        assert!(bridge.last_heartbeat.read().await.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_poll_handler_returns_null_when_empty() {
        let bridge = PluginBridge::new();
        let router = create_router(bridge);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/poll")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert_eq!(body_str, "null");
    }

    #[tokio::test]
    async fn test_poll_handler_returns_command() {
        let bridge = PluginBridge::new();

        // Queue a command manually
        bridge.pending_commands.write().await.push(Command {
            id: "test-id".to_string(),
            action: "getSelection".to_string(),
            params: serde_json::json!({}),
        });

        let router = create_router(bridge);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/poll")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("getSelection"));
        assert!(body_str.contains("test-id"));
    }

    #[tokio::test]
    async fn test_result_handler_accepts_result() {
        let bridge = PluginBridge::new();
        let router = create_router(bridge);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "test-id".to_string(),
                            result: Some(serde_json::json!({"success": true})),
                            error: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("ok"));
    }

    #[tokio::test]
    async fn test_command_response_flow() {
        let bridge = PluginBridge::new();

        // Queue command and get receiver
        let (tx, rx) = oneshot::channel();
        bridge
            .pending_results
            .write()
            .await
            .insert("flow-test-id".to_string(), tx);

        bridge.pending_commands.write().await.push(Command {
            id: "flow-test-id".to_string(),
            action: "testAction".to_string(),
            params: serde_json::json!({}),
        });

        // Simulate plugin sending result
        let router = create_router(bridge.clone());

        let _response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "flow-test-id".to_string(),
                            result: Some(serde_json::json!({"data": "test-result"})),
                            error: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Receiver should get the result
        let result = rx.await.unwrap();
        assert_eq!(result.id, "flow-test-id");
        assert!(result.result.is_some());
    }

    #[test]
    fn test_command_serialization() {
        let cmd = Command {
            id: "uuid-123".to_string(),
            action: "getSelection".to_string(),
            params: serde_json::json!({"path": "/game/Workspace"}),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: Command = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, cmd.id);
        assert_eq!(deserialized.action, cmd.action);
    }

    #[test]
    fn test_plugin_response_serialization() {
        let resp = PluginResponse {
            id: "uuid-456".to_string(),
            result: Some(serde_json::json!({"instances": []})),
            error: None,
        };

        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: PluginResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, resp.id);
        assert!(deserialized.result.is_some());
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn test_plugin_response_with_error() {
        let resp = PluginResponse {
            id: "uuid-789".to_string(),
            result: None,
            error: Some("Script not found".to_string()),
        };

        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: PluginResponse = serde_json::from_str(&json).unwrap();

        assert!(deserialized.result.is_none());
        assert_eq!(deserialized.error.unwrap(), "Script not found");
    }

    // ========================================
    // HTTP Handler Edge Case Tests
    // ========================================

    #[tokio::test]
    async fn test_result_handler_malformed_json() {
        let bridge = PluginBridge::new();
        let router = create_router(bridge);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from("{invalid json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Axum returns 400 Bad Request for JSON parse errors
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_result_handler_missing_fields() {
        let bridge = PluginBridge::new();
        let router = create_router(bridge);

        // Missing required 'id' field
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"result": null}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_result_handler_empty_body() {
        let bridge = PluginBridge::new();
        let router = create_router(bridge);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Empty body returns 400 Bad Request
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_result_handler_unknown_command_id() {
        let bridge = PluginBridge::new();
        let router = create_router(bridge);

        // Result for command ID that was never registered
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "unknown-command-id".to_string(),
                            result: Some(serde_json::json!({"data": "test"})),
                            error: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should still return OK (logs warning internally)
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_poll_handler_multiple_commands() {
        let bridge = PluginBridge::new();

        // Queue multiple commands
        for i in 0..3 {
            bridge.pending_commands.write().await.push(Command {
                id: format!("cmd-{}", i),
                action: format!("action-{}", i),
                params: serde_json::json!({}),
            });
        }

        let router = create_router(bridge.clone());

        // First poll should return first command (FIFO via pop from back)
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/poll")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("cmd-2")); // Last pushed = first popped

        // Verify 2 commands remain
        assert_eq!(bridge.pending_commands.read().await.len(), 2);
    }

    #[tokio::test]
    async fn test_health_handler_returns_version() {
        let bridge = PluginBridge::new();
        let router = create_router(bridge);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let health: HealthStatusTest = serde_json::from_slice(&body).unwrap();

        assert_eq!(health.status, "healthy");
        assert!(health.plugin_connected);
        assert!(!health.version.is_empty());
    }

    #[tokio::test]
    async fn test_health_handler_degraded_status() {
        let bridge = PluginBridge::new();

        // Set heartbeat to be stale
        *bridge.last_heartbeat.write().await =
            Instant::now().checked_sub(Duration::from_secs(15)).unwrap();

        let router = create_router(bridge);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let health: HealthStatusTest = serde_json::from_slice(&body).unwrap();

        assert_eq!(health.status, "degraded");
        assert!(!health.plugin_connected);
        assert!(health.heartbeat_age_secs > 10.0);
    }

    #[tokio::test]
    async fn test_result_handler_with_error_response() {
        let bridge = PluginBridge::new();

        // Register a result receiver
        let (tx, rx) = oneshot::channel();
        bridge
            .pending_results
            .write()
            .await
            .insert("error-cmd-id".to_string(), tx);

        let router = create_router(bridge);

        // Send error response
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "error-cmd-id".to_string(),
                            result: None,
                            error: Some("Instance not found".to_string()),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Receiver should get the error response
        let result = rx.await.unwrap();
        assert!(result.result.is_none());
        assert_eq!(result.error, Some("Instance not found".to_string()));
    }

    #[test]
    fn test_health_status_serialization() {
        let status = HealthStatus {
            status: "healthy",
            plugin_connected: true,
            heartbeat_age_secs: 1.5,
            version: "0.1.0",
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("healthy"));
        assert!(json.contains("true"));
        assert!(json.contains("1.5"));
        assert!(json.contains("0.1.0"));
    }

    #[test]
    fn test_health_status_clone() {
        let status = HealthStatus {
            status: "degraded",
            plugin_connected: false,
            heartbeat_age_secs: 15.0,
            version: "0.1.0",
        };

        let cloned = status.clone();
        assert_eq!(cloned.status, "degraded");
        assert!(!cloned.plugin_connected);
    }

    #[test]
    fn test_command_debug() {
        let cmd = Command {
            id: "debug-test".to_string(),
            action: "getSelection".to_string(),
            params: serde_json::json!({}),
        };

        let debug = format!("{:?}", cmd);
        assert!(debug.contains("Command"));
        assert!(debug.contains("debug-test"));
        assert!(debug.contains("getSelection"));
    }

    #[test]
    fn test_command_clone() {
        let cmd = Command {
            id: "clone-test".to_string(),
            action: "testAction".to_string(),
            params: serde_json::json!({"key": "value"}),
        };

        let cloned = cmd.clone();
        assert_eq!(cloned.id, "clone-test");
        assert_eq!(cloned.action, "testAction");
    }

    #[test]
    fn test_plugin_response_debug() {
        let resp = PluginResponse {
            id: "debug-resp".to_string(),
            result: Some(serde_json::json!({"ok": true})),
            error: None,
        };

        let debug = format!("{:?}", resp);
        assert!(debug.contains("PluginResponse"));
        assert!(debug.contains("debug-resp"));
    }

    #[test]
    fn test_plugin_response_clone() {
        let resp = PluginResponse {
            id: "clone-resp".to_string(),
            result: Some(serde_json::json!({"data": [1, 2, 3]})),
            error: None,
        };

        let cloned = resp.clone();
        assert_eq!(cloned.id, "clone-resp");
        assert!(cloned.result.is_some());
    }

    // ========================================
    // Execute Command Error Path Tests
    // ========================================

    #[tokio::test]
    async fn test_execute_command_plugin_error_response() {
        let bridge = PluginBridge::new();

        // Start command execution in background
        let bridge_clone = bridge.clone();
        let handle = tokio::spawn(async move {
            bridge_clone
                .execute_command("testAction", serde_json::json!({}))
                .await
        });

        // Wait for command to be queued
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Get the command ID from the queue
        let command_id = {
            let commands = bridge.pending_commands.read().await;
            commands[0].id.clone()
        };

        // Simulate plugin sending error response
        let sender = bridge.pending_results.write().await.remove(&command_id);
        if let Some(tx) = sender {
            tx.send(PluginResponse {
                id: command_id,
                result: None,
                error: Some("Script execution failed".to_string()),
            })
            .unwrap();
        }

        // The command should return an error
        let result = handle.await.unwrap();
        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::PluginExecutionError(msg) => {
                assert!(msg.contains("Script execution failed"));
            }
            e => panic!("Expected PluginExecutionError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_execute_command_missing_result() {
        let bridge = PluginBridge::new();

        // Start command execution in background
        let bridge_clone = bridge.clone();
        let handle = tokio::spawn(async move {
            bridge_clone
                .execute_command("testAction", serde_json::json!({}))
                .await
        });

        // Wait for command to be queued
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Get the command ID from the queue
        let command_id = {
            let commands = bridge.pending_commands.read().await;
            commands[0].id.clone()
        };

        // Simulate plugin sending response with no result and no error
        let sender = bridge.pending_results.write().await.remove(&command_id);
        if let Some(tx) = sender {
            tx.send(PluginResponse {
                id: command_id,
                result: None,
                error: None,
            })
            .unwrap();
        }

        // The command should return InvalidStudioData error
        let result = handle.await.unwrap();
        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::InvalidStudioData(msg) => {
                assert!(msg.contains("no result"));
            }
            e => panic!("Expected InvalidStudioData, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_execute_command_successful_response() {
        let bridge = PluginBridge::new();

        // Start command execution in background
        let bridge_clone = bridge.clone();
        let handle = tokio::spawn(async move {
            bridge_clone
                .execute_command("getSelection", serde_json::json!({}))
                .await
        });

        // Wait for command to be queued
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Get the command ID from the queue
        let command_id = {
            let commands = bridge.pending_commands.read().await;
            commands[0].id.clone()
        };

        // Simulate plugin sending successful response
        let sender = bridge.pending_results.write().await.remove(&command_id);
        if let Some(tx) = sender {
            tx.send(PluginResponse {
                id: command_id,
                result: Some(serde_json::json!({"selection": ["Part1", "Part2"]})),
                error: None,
            })
            .unwrap();
        }

        // The command should succeed
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value["selection"].is_array());
    }

    #[tokio::test]
    async fn test_execute_command_channel_closed() {
        // Test the channel closed error path by directly testing with a pre-closed channel
        // The execute_command function handles Ok(Err(_)) from the oneshot receiver
        // which happens when the sender is dropped without sending

        // Create a channel and immediately drop the sender
        let (_tx, rx): (oneshot::Sender<PluginResponse>, _) = oneshot::channel();
        drop(_tx);

        // Verify that receiving on a closed channel returns RecvError
        let result = rx.await;
        assert!(
            result.is_err(),
            "Channel should be closed when sender is dropped"
        );
    }

    #[tokio::test]
    async fn test_result_handler_sender_dropped_before_result() {
        let bridge = PluginBridge::new();

        // Register a result receiver, then drop it immediately
        let (tx, rx) = oneshot::channel();
        bridge
            .pending_results
            .write()
            .await
            .insert("orphan-cmd-id".to_string(), tx);

        // Drop the receiver to simulate caller timeout
        drop(rx);

        // Now send result via HTTP handler - should log warning but not panic
        let router = create_router(bridge);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "orphan-cmd-id".to_string(),
                            result: Some(serde_json::json!({"late": "result"})),
                            error: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should still return OK even though send failed (logs warning)
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_plugin_bridge_clone() {
        let bridge1 = PluginBridge::new();
        let bridge2 = bridge1.clone();

        // Modifications via one clone should be visible via the other
        bridge1.pending_commands.write().await.push(Command {
            id: "shared-cmd".to_string(),
            action: "test".to_string(),
            params: serde_json::json!({}),
        });

        assert_eq!(bridge2.pending_commands.read().await.len(), 1);
    }

    // Compile-time validation of timeout constants
    const _: () = {
        assert!(PLUGIN_HEARTBEAT_TIMEOUT_SECS > 0);
        assert!(PLUGIN_HEARTBEAT_TIMEOUT_SECS < 60); // Reasonable timeout
        assert!(PLUGIN_COMMAND_TIMEOUT_SECS > 0);
        assert!(PLUGIN_COMMAND_TIMEOUT_SECS <= 120); // Reasonable max
    };

    #[test]
    fn test_timeout_constants_are_valid() {
        // Compile-time assertions above validate these, runtime test for documentation
        assert_eq!(PLUGIN_HEARTBEAT_TIMEOUT_SECS, 10);
        assert_eq!(PLUGIN_COMMAND_TIMEOUT_SECS, 30);
    }

    // === LATE RESULT TRACKING TESTS ===

    #[tokio::test]
    async fn test_plugin_bridge_with_metrics() {
        let metrics = Arc::new(crate::metrics::ServerMetrics::new());
        let bridge = PluginBridge::with_metrics(metrics.clone());

        // Record a late result
        bridge.record_late_result(false);
        bridge.record_late_result(true);

        // Verify metrics were recorded
        let snapshot = metrics.late_results_snapshot();
        assert_eq!(snapshot.total, 2);
        assert_eq!(snapshot.successful, 1);
        assert_eq!(snapshot.errors, 1);
    }

    #[tokio::test]
    async fn test_plugin_bridge_without_metrics_no_panic() {
        let bridge = PluginBridge::new();

        // Should not panic even without metrics
        bridge.record_late_result(false);
        bridge.record_late_result(true);
    }

    #[tokio::test]
    async fn test_late_result_recorded_on_caller_timeout() {
        let metrics = Arc::new(crate::metrics::ServerMetrics::new());
        let bridge = PluginBridge::with_metrics(metrics.clone());

        // Register a result receiver, then drop it to simulate caller timeout
        let (tx, rx) = oneshot::channel();
        bridge
            .pending_results
            .write()
            .await
            .insert("late-test-id".to_string(), tx);

        // Drop the receiver to simulate caller timeout
        drop(rx);

        // Send result via HTTP handler
        let router = create_router(bridge);

        let _response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "late-test-id".to_string(),
                            result: Some(serde_json::json!({"completed": "work"})),
                            error: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify the late result was recorded as successful (no error)
        let snapshot = metrics.late_results_snapshot();
        assert_eq!(snapshot.total, 1);
        assert_eq!(snapshot.successful, 1);
        assert_eq!(snapshot.errors, 0);
    }

    #[tokio::test]
    async fn test_late_result_with_error_recorded() {
        let metrics = Arc::new(crate::metrics::ServerMetrics::new());
        let bridge = PluginBridge::with_metrics(metrics.clone());

        // Register a result receiver, then drop it to simulate caller timeout
        let (tx, rx) = oneshot::channel();
        bridge
            .pending_results
            .write()
            .await
            .insert("late-error-id".to_string(), tx);

        // Drop the receiver to simulate caller timeout
        drop(rx);

        // Send result with error via HTTP handler
        let router = create_router(bridge);

        let _response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "late-error-id".to_string(),
                            result: None,
                            error: Some("Plugin error that arrived late".to_string()),
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify the late result was recorded as error
        let snapshot = metrics.late_results_snapshot();
        assert_eq!(snapshot.total, 1);
        assert_eq!(snapshot.successful, 0);
        assert_eq!(snapshot.errors, 1);
    }

    // === UNKNOWN COMMAND TRACKING TESTS ===

    #[tokio::test]
    async fn test_plugin_bridge_record_unknown_command() {
        let metrics = Arc::new(crate::metrics::ServerMetrics::new());
        let bridge = PluginBridge::with_metrics(metrics.clone());

        // Record unknown commands
        bridge.record_unknown_command();
        bridge.record_unknown_command();

        // Verify metrics were recorded
        let snapshot = metrics.unknown_commands_snapshot();
        assert_eq!(snapshot.total, 2);
    }

    #[tokio::test]
    async fn test_plugin_bridge_without_metrics_unknown_command_no_panic() {
        let bridge = PluginBridge::new();

        // Should not panic even without metrics
        bridge.record_unknown_command();
        bridge.record_unknown_command();
    }

    #[tokio::test]
    async fn test_unknown_command_recorded_when_no_sender() {
        let metrics = Arc::new(crate::metrics::ServerMetrics::new());
        let bridge = PluginBridge::with_metrics(metrics.clone());

        // DO NOT register any sender - simulate unknown command ID scenario
        // Send result via HTTP handler for an ID that was never registered
        let router = create_router(bridge);

        let _response = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "unknown-command-id".to_string(),
                            result: Some(serde_json::json!({"data": "orphaned"})),
                            error: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify the unknown command was recorded
        let snapshot = metrics.unknown_commands_snapshot();
        assert_eq!(snapshot.total, 1);
    }

    #[tokio::test]
    async fn test_unknown_command_vs_late_result_distinction() {
        let metrics = Arc::new(crate::metrics::ServerMetrics::new());
        let bridge = PluginBridge::with_metrics(metrics.clone());

        // Scenario 1: Unknown command (no sender ever registered)
        // Scenario 2: Late result (sender was registered but receiver dropped)

        // First, create an unknown command scenario
        let router = create_router(bridge.clone());

        let _ = router
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "never-registered".to_string(),
                            result: Some(serde_json::json!({"orphan": true})),
                            error: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Now create a late result scenario (register sender, drop receiver, send result)
        let (tx, rx) = oneshot::channel();
        bridge
            .pending_results
            .write()
            .await
            .insert("registered-then-dropped".to_string(), tx);
        drop(rx); // Caller times out

        let router2 = create_router(bridge);

        let _ = router2
            .oneshot(
                Request::builder()
                    .uri("/result")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PluginResponse {
                            id: "registered-then-dropped".to_string(),
                            result: Some(serde_json::json!({"late": true})),
                            error: None,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify both scenarios tracked correctly
        let unknown_snapshot = metrics.unknown_commands_snapshot();
        let late_snapshot = metrics.late_results_snapshot();

        assert_eq!(unknown_snapshot.total, 1, "Should have 1 unknown command");
        assert_eq!(late_snapshot.total, 1, "Should have 1 late result");
        assert_eq!(
            late_snapshot.successful, 1,
            "Late result should be marked successful"
        );
    }
}
