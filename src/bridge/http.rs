use crate::error::RobloxMcpError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{oneshot, RwLock};
use tokio::time::{timeout, Duration};
use tracing::warn;
use uuid::Uuid;

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
}

impl PluginBridge {
    pub fn new() -> Self {
        Self {
            pending_commands: Arc::new(RwLock::new(Vec::new())),
            pending_results: Arc::new(RwLock::new(HashMap::new())),
            last_heartbeat: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// Check if plugin is connected (heartbeat within 10 seconds)
    ///
    /// Public API for health checks - used in tests and available for external consumers
    #[allow(dead_code)]
    pub async fn is_connected(&self) -> bool {
        self.last_heartbeat.read().await.elapsed() < Duration::from_secs(10)
    }

    /// Execute a command via the plugin bridge with fast-failure timeout
    pub async fn execute_command(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RobloxMcpError> {
        // Check heartbeat - FAIL IMMEDIATELY if stale
        let elapsed = self.last_heartbeat.read().await.elapsed();
        if elapsed > Duration::from_secs(10) {
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
        let response = match timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => {
                return Err(RobloxMcpError::PluginExecutionError(
                    "Result channel closed unexpectedly".to_string(),
                ))
            }
            Err(_) => return Err(RobloxMcpError::PluginTimeout(Duration::from_secs(30))),
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
                // Caller timed out before we could deliver the result - LOG THIS
                warn!(
                    command_id = %response_id,
                    had_error = had_error,
                    "Plugin result discarded: caller already timed out"
                );
            }
        }
        None => {
            // No sender registered - this should not happen, LOG IT
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

/// HTTP endpoint: Health check for monitoring
async fn health_handler(State(bridge): State<PluginBridge>) -> Json<HealthStatus> {
    let heartbeat_age = bridge.last_heartbeat.read().await.elapsed();
    let connected = heartbeat_age < Duration::from_secs(10);

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
}
