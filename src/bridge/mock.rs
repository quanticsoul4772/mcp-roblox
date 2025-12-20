//! Mock Studio bridge for testing
//!
//! Provides a mock implementation of StudioBridge that returns pre-configured responses.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::RobloxMcpError;
use super::StudioBridge;

/// Internal shared state for MockBridge
struct MockState {
    /// Pre-configured responses by action name
    responses: HashMap<String, serde_json::Value>,
    /// Recorded command calls for verification
    calls: Vec<MockCall>,
}

/// Recorded command call for verification
#[derive(Debug, Clone)]
pub struct MockCall {
    pub action: String,
    pub params: serde_json::Value,
}

/// Mock Studio bridge for testing
///
/// Allows tests to pre-configure responses and verify command calls
/// without needing an actual Roblox Studio connection.
///
/// Clone is cheap - all clones share the same internal state via Arc.
#[derive(Clone)]
pub struct MockBridge {
    state: Arc<Mutex<MockState>>,
    connected: Arc<AtomicBool>,
}

impl MockBridge {
    /// Create a new mock bridge in connected state
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                responses: HashMap::new(),
                calls: Vec::new(),
            })),
            connected: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Set the response for a specific action
    ///
    /// When execute_command is called with this action, it will return this response.
    pub fn set_response(&self, action: &str, response: serde_json::Value) {
        self.state
            .lock()
            .unwrap()
            .responses
            .insert(action.to_string(), response);
    }

    /// Set multiple responses at once
    pub fn set_responses<'a>(&self, responses: impl IntoIterator<Item = (&'a str, serde_json::Value)>) {
        let mut state = self.state.lock().unwrap();
        for (action, response) in responses {
            state.responses.insert(action.to_string(), response);
        }
    }

    /// Mark the bridge as disconnected
    ///
    /// After calling this, is_connected() will return false and
    /// execute_command() will return a PluginTimeout error.
    pub fn set_disconnected(&self) {
        self.connected.store(false, Ordering::SeqCst);
    }

    /// Mark the bridge as connected
    pub fn set_connected(&self) {
        self.connected.store(true, Ordering::SeqCst);
    }

    /// Get all recorded command calls
    pub fn calls(&self) -> Vec<MockCall> {
        self.state.lock().unwrap().calls.clone()
    }

    /// Get the last recorded call
    pub fn last_call(&self) -> Option<MockCall> {
        self.state.lock().unwrap().calls.last().cloned()
    }

    /// Clear all recorded calls
    pub fn clear_calls(&self) {
        self.state.lock().unwrap().calls.clear();
    }

    /// Check if a specific action was called
    pub fn was_called(&self, action: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c.action == action)
    }

    /// Count how many times a specific action was called
    pub fn call_count(&self, action: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c.action == action)
            .count()
    }
}

impl Default for MockBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MockBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap();
        f.debug_struct("MockBridge")
            .field("connected", &self.connected.load(Ordering::SeqCst))
            .field("configured_responses", &state.responses.len())
            .field("recorded_calls", &state.calls.len())
            .finish()
    }
}

#[async_trait]
impl StudioBridge for MockBridge {
    async fn execute_command(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RobloxMcpError> {
        // Check connection status first
        if !self.connected.load(Ordering::SeqCst) {
            return Err(RobloxMcpError::PluginTimeout(Duration::from_secs(10)));
        }

        // Record the call
        self.state.lock().unwrap().calls.push(MockCall {
            action: action.to_string(),
            params: params.clone(),
        });

        // Return configured response or error
        self.state
            .lock()
            .unwrap()
            .responses
            .get(action)
            .cloned()
            .ok_or_else(|| {
                RobloxMcpError::ConfigError(format!(
                    "MockBridge: No response configured for action '{}'",
                    action
                ))
            })
    }

    async fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_bridge_returns_configured_response() {
        let mock = MockBridge::new();
        mock.set_response("getSelection", serde_json::json!({"items": [1, 2, 3]}));

        let result = mock
            .execute_command("getSelection", serde_json::json!({}))
            .await
            .unwrap();

        assert_eq!(result["items"], serde_json::json!([1, 2, 3]));
    }

    #[tokio::test]
    async fn test_mock_bridge_records_calls() {
        let mock = MockBridge::new();
        mock.set_response("setProperty", serde_json::json!({"ok": true}));

        mock.execute_command(
            "setProperty",
            serde_json::json!({"path": "/game/Workspace", "property": "Name", "value": "Test"}),
        )
        .await
        .unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].action, "setProperty");
        assert_eq!(calls[0].params["property"], "Name");
    }

    #[tokio::test]
    async fn test_mock_bridge_disconnected_returns_timeout() {
        let mock = MockBridge::new();
        mock.set_disconnected();

        let result = mock
            .execute_command("getSelection", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::PluginTimeout(_)
        ));
    }

    #[tokio::test]
    async fn test_mock_bridge_no_response_returns_error() {
        let mock = MockBridge::new();

        let result = mock
            .execute_command("unconfiguredAction", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::ConfigError(_)
        ));
    }

    #[tokio::test]
    async fn test_mock_bridge_is_connected() {
        let mock = MockBridge::new();

        assert!(mock.is_connected().await);

        mock.set_disconnected();
        assert!(!mock.is_connected().await);

        mock.set_connected();
        assert!(mock.is_connected().await);
    }

    #[tokio::test]
    async fn test_mock_bridge_was_called() {
        let mock = MockBridge::new();
        mock.set_response("action1", serde_json::json!({}));
        mock.set_response("action2", serde_json::json!({}));

        mock.execute_command("action1", serde_json::json!({}))
            .await
            .unwrap();

        assert!(mock.was_called("action1"));
        assert!(!mock.was_called("action2"));
    }

    #[tokio::test]
    async fn test_mock_bridge_call_count() {
        let mock = MockBridge::new();
        mock.set_response("repeated", serde_json::json!({}));

        for _ in 0..5 {
            mock.execute_command("repeated", serde_json::json!({}))
                .await
                .unwrap();
        }

        assert_eq!(mock.call_count("repeated"), 5);
        assert_eq!(mock.call_count("other"), 0);
    }

    #[tokio::test]
    async fn test_mock_bridge_set_responses() {
        let mock = MockBridge::new();
        mock.set_responses([
            ("action1", serde_json::json!({"result": 1})),
            ("action2", serde_json::json!({"result": 2})),
        ]);

        let r1 = mock
            .execute_command("action1", serde_json::json!({}))
            .await
            .unwrap();
        let r2 = mock
            .execute_command("action2", serde_json::json!({}))
            .await
            .unwrap();

        assert_eq!(r1["result"], 1);
        assert_eq!(r2["result"], 2);
    }

    #[tokio::test]
    async fn test_mock_bridge_clone_shares_state() {
        let mock1 = MockBridge::new();
        mock1.set_response("shared", serde_json::json!({"shared": true}));

        let mock2 = mock1.clone();

        mock2
            .execute_command("shared", serde_json::json!({}))
            .await
            .unwrap();

        // Verify that mock1 can see the recorded call (shared state)
        assert_eq!(mock1.calls().len(), 1);
        assert!(mock1.was_called("shared"));
    }

    #[test]
    fn test_mock_bridge_debug() {
        let mock = MockBridge::new();
        mock.set_response("test", serde_json::json!({}));

        let debug = format!("{:?}", mock);

        assert!(debug.contains("MockBridge"));
        assert!(debug.contains("connected"));
        assert!(debug.contains("configured_responses"));
    }

    #[tokio::test]
    async fn test_mock_bridge_last_call() {
        let mock = MockBridge::new();
        mock.set_response("first", serde_json::json!({}));
        mock.set_response("second", serde_json::json!({}));

        mock.execute_command("first", serde_json::json!({}))
            .await
            .unwrap();
        mock.execute_command("second", serde_json::json!({}))
            .await
            .unwrap();

        let last = mock.last_call().unwrap();
        assert_eq!(last.action, "second");
    }

    #[tokio::test]
    async fn test_mock_bridge_clear_calls() {
        let mock = MockBridge::new();
        mock.set_response("test", serde_json::json!({}));

        mock.execute_command("test", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(mock.calls().len(), 1);

        mock.clear_calls();
        assert_eq!(mock.calls().len(), 0);
    }
}
