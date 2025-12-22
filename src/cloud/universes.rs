//! Open Cloud Universe API functionality
//!
//! Manage Roblox universes (games) via Open Cloud API.
//! Includes getting universe info and restarting game servers.

use crate::error::RobloxMcpError;
use crate::http::HttpClient;
use serde::{Deserialize, Serialize};

/// Information about a Roblox universe (game)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UniverseInfo {
    /// Full path to the universe resource
    pub path: String,
    /// UTC timestamp when the universe was created
    pub create_time: String,
    /// UTC timestamp when the universe was last updated
    pub update_time: String,
    /// Display name of the universe
    pub display_name: String,
    /// Description of the universe
    pub description: String,
    /// User who owns the universe (if individual ownership)
    #[serde(default)]
    pub user: Option<String>,
    /// Group that owns the universe (if group ownership)
    #[serde(default)]
    pub group: Option<String>,
    /// Visibility of the universe (e.g., "PUBLIC", "PRIVATE")
    #[serde(default)]
    pub visibility: Option<String>,
    /// Whether voice chat is enabled
    #[serde(default)]
    pub voice_chat_enabled: bool,
    /// Age rating for the universe
    #[serde(default)]
    pub age_rating: Option<String>,
    /// Whether desktop is supported
    #[serde(default)]
    pub desktop_enabled: bool,
    /// Whether mobile is supported
    #[serde(default)]
    pub mobile_enabled: bool,
    /// Whether tablet is supported
    #[serde(default)]
    pub tablet_enabled: bool,
    /// Whether VR is supported
    #[serde(default)]
    pub vr_enabled: bool,
    /// Whether console is supported
    #[serde(default)]
    pub console_enabled: bool,
}

impl<H: HttpClient> super::OpenCloudClient<H> {
    /// Get information about a universe
    ///
    /// Returns metadata about the specified universe including name,
    /// description, ownership, platform support, and more.
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID to get info for
    pub async fn get_universe(&self, universe_id: u64) -> Result<UniverseInfo, RobloxMcpError> {
        let url = format!("{}/cloud/v2/universes/{}", self.base_url(), universe_id);

        let response = self
            .http()
            .get(&url, &[("x-api-key", self.api_key())])
            .await?;

        if !response.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| "[failed to read body]".into());
            return Err(RobloxMcpError::OpenCloudError {
                status: response.status,
                message: body,
            });
        }

        response.json()
    }

    /// Restart all game servers for a universe
    ///
    /// Triggers a graceful restart of all servers for the specified universe.
    /// Players will be disconnected and can rejoin once servers restart.
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID to restart servers for
    pub async fn restart_universe_servers(&self, universe_id: u64) -> Result<(), RobloxMcpError> {
        let url = format!(
            "{}/cloud/v2/universes/{}:restartServers",
            self.base_url(),
            universe_id
        );

        let response = self
            .http()
            .post_json(
                &url,
                &[
                    ("x-api-key", self.api_key()),
                    ("Content-Type", "application/json"),
                ],
                serde_json::json!({}),
            )
            .await?;

        if !response.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| "[failed to read body]".into());
            return Err(RobloxMcpError::OpenCloudError {
                status: response.status,
                message: body,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::OpenCloudClient;
    use crate::http::mock::{MockHttpClient, MockResponse};

    #[test]
    fn test_universe_info_deserialize() {
        let json = r#"{
            "path": "universes/123456",
            "createTime": "2024-01-01T00:00:00Z",
            "updateTime": "2024-06-15T12:00:00Z",
            "displayName": "My Awesome Game",
            "description": "A fun game for everyone",
            "user": "users/12345",
            "visibility": "PUBLIC",
            "voiceChatEnabled": true,
            "desktopEnabled": true,
            "mobileEnabled": true,
            "tabletEnabled": true,
            "vrEnabled": false,
            "consoleEnabled": false
        }"#;

        let info: UniverseInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.display_name, "My Awesome Game");
        assert_eq!(info.description, "A fun game for everyone");
        assert_eq!(info.user, Some("users/12345".to_string()));
        assert!(info.voice_chat_enabled);
        assert!(info.desktop_enabled);
        assert!(!info.vr_enabled);
    }

    #[test]
    fn test_universe_info_with_group_ownership() {
        let json = r#"{
            "path": "universes/789",
            "createTime": "2023-05-01T00:00:00Z",
            "updateTime": "2024-06-01T00:00:00Z",
            "displayName": "Group Game",
            "description": "Made by our group",
            "group": "groups/67890"
        }"#;

        let info: UniverseInfo = serde_json::from_str(json).unwrap();
        assert!(info.user.is_none());
        assert_eq!(info.group, Some("groups/67890".to_string()));
    }

    #[test]
    fn test_universe_info_minimal() {
        let json = r#"{
            "path": "universes/1",
            "createTime": "2024-01-01T00:00:00Z",
            "updateTime": "2024-01-01T00:00:00Z",
            "displayName": "Test",
            "description": ""
        }"#;

        let info: UniverseInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.display_name, "Test");
        assert!(info.description.is_empty());
        assert!(info.visibility.is_none());
        assert!(!info.voice_chat_enabled);
    }

    #[tokio::test]
    async fn test_get_universe_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({
                "path": "universes/123456",
                "createTime": "2024-01-01T00:00:00Z",
                "updateTime": "2024-06-15T12:00:00Z",
                "displayName": "Test Game",
                "description": "A test game",
                "visibility": "PUBLIC"
            }),
        ));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client.get_universe(123456).await;

        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.display_name, "Test Game");
        assert_eq!(info.visibility, Some("PUBLIC".to_string()));
    }

    #[tokio::test]
    async fn test_get_universe_not_found() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(404, b"Universe not found"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client.get_universe(999999).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, message } => {
                assert_eq!(status, 404);
                assert!(message.contains("not found"));
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_get_universe_url_format() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({
                "path": "universes/777",
                "createTime": "2024-01-01T00:00:00Z",
                "updateTime": "2024-01-01T00:00:00Z",
                "displayName": "Test",
                "description": ""
            }),
        ));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client.get_universe(777).await.unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.contains("/cloud/v2/universes/777"));
        assert!(requests[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "test-key"));
    }

    #[tokio::test]
    async fn test_restart_universe_servers_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(200, b""));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client.restart_universe_servers(123456).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_restart_universe_servers_unauthorized() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(401, b"Unauthorized"));

        let client = OpenCloudClient::with_http(mock, "bad-key");

        let result = client.restart_universe_servers(123).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, .. } => {
                assert_eq!(status, 401);
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_restart_universe_servers_url_format() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(200, b""));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client.restart_universe_servers(888).await.unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .url
            .contains("/cloud/v2/universes/888:restartServers"));
    }

    #[tokio::test]
    async fn test_restart_universe_servers_connection_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::error("Connection refused"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client.restart_universe_servers(123).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::HttpConnectionError(_)
        ));
    }
}
