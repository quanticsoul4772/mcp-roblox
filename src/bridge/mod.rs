//! Bridge module for Roblox Studio communication
//!
//! Provides trait-based abstraction over Studio plugin communication
//! for testability while maintaining production HTTP-based implementation.

pub mod http;

#[cfg(test)]
pub mod mock;

use crate::error::RobloxMcpError;
use async_trait::async_trait;

/// Abstraction over Studio plugin communication for testability
///
/// This trait allows tests to inject mock implementations while
/// production code uses the HTTP-based PluginBridge.
#[async_trait]
pub trait StudioBridge: Send + Sync {
    /// Execute a command via the Studio plugin
    ///
    /// # Arguments
    /// * `action` - The action name to execute (e.g., "getSelection", "setProperty")
    /// * `params` - JSON parameters for the action
    ///
    /// # Returns
    /// * `Ok(Value)` - The plugin's response data
    /// * `Err(RobloxMcpError)` - If the plugin is disconnected, times out, or returns an error
    async fn execute_command(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RobloxMcpError>;

    /// Check if the Studio plugin is connected
    ///
    /// Returns true if the plugin has sent a heartbeat within the timeout window.
    async fn is_connected(&self) -> bool;
}

// Implement StudioBridge for the production PluginBridge
#[async_trait]
impl StudioBridge for http::PluginBridge {
    async fn execute_command(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RobloxMcpError> {
        // Delegate to the existing implementation
        self.execute_command(action, params).await
    }

    async fn is_connected(&self) -> bool {
        // Delegate to the existing implementation
        http::PluginBridge::is_connected(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::mock::MockBridge;

    #[tokio::test]
    async fn test_studio_bridge_trait_with_mock() {
        let mock = MockBridge::new();
        mock.set_response("getSelection", serde_json::json!({"selected": ["Part1"]}));

        let result = mock
            .execute_command("getSelection", serde_json::json!({}))
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap()["selected"][0], "Part1");
    }

    #[tokio::test]
    async fn test_studio_bridge_mock_disconnected() {
        let mock = MockBridge::new();
        mock.set_disconnected();

        assert!(!mock.is_connected().await);
    }

    #[tokio::test]
    async fn test_studio_bridge_mock_no_response() {
        let mock = MockBridge::new();

        let result = mock
            .execute_command("unknownAction", serde_json::json!({}))
            .await;

        assert!(result.is_err());
    }
}
