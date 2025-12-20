//! Open Cloud API client with connection pooling
//!
//! This client should be created ONCE at server startup and stored in
//! RobloxMcpServer struct to benefit from HTTP connection pooling.

use crate::error::RobloxMcpError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tracing::instrument;

/// Result from publishing a place to Roblox
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub version_number: u64,
}

/// Open Cloud API client with connection pooling
#[derive(Debug)]
pub struct OpenCloudClient {
    client: Client,
    #[allow(dead_code)] // API key is used in publish_place, but not in Debug output
    api_key: String,
    base_url: String,
}

impl OpenCloudClient {
    /// Create a new Open Cloud client with connection pooling
    ///
    /// # Errors
    /// Returns `ConfigError` if `ROBLOX_OPEN_CLOUD_API_KEY` environment variable is not set
    pub fn new() -> Result<Self, RobloxMcpError> {
        let api_key = std::env::var("ROBLOX_OPEN_CLOUD_API_KEY").map_err(|_| {
            RobloxMcpError::ConfigError(
                "ROBLOX_OPEN_CLOUD_API_KEY environment variable not set".into(),
            )
        })?;

        // Configure client with connection pooling for performance
        let client = Client::builder()
            .pool_max_idle_per_host(5)
            .pool_idle_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(RobloxMcpError::from_reqwest)?;

        Ok(Self {
            client,
            api_key,
            base_url: "https://apis.roblox.com".into(),
        })
    }

    /// Publish a place file (.rbxl) to Roblox
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID from Roblox Creator Dashboard
    /// * `place_id` - Place ID to publish to
    /// * `rbxl_path` - Path to .rbxl file
    ///
    /// # Errors
    /// Returns error if file cannot be read or API call fails
    #[instrument(skip(self), fields(universe_id, place_id, path = %rbxl_path.display()))]
    pub async fn publish_place(
        &self,
        universe_id: u64,
        place_id: u64,
        rbxl_path: &Path,
    ) -> Result<PublishResult, RobloxMcpError> {
        // Read .rbxl file
        let content = tokio::fs::read(rbxl_path)
            .await
            .map_err(|e| RobloxMcpError::FileSystemError {
                path: rbxl_path.display().to_string(),
                source: e,
            })?;

        // POST to Open Cloud
        let url = format!(
            "{}/universes/v1/{}/places/{}/versions",
            self.base_url, universe_id, place_id
        );

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/octet-stream")
            .query(&[("versionType", "Published")])
            .body(content)
            .send()
            .await
            .map_err(RobloxMcpError::from_reqwest)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RobloxMcpError::OpenCloudError {
                status: status.as_u16(),
                message: body,
            });
        }

        response
            .json()
            .await
            .map_err(RobloxMcpError::from_reqwest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_publish_result_deserialize() {
        let json = r#"{"versionNumber": 42}"#;
        let result: PublishResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.version_number, 42);
    }

    #[test]
    fn test_new_without_api_key() {
        // Ensure env var is not set for this test
        std::env::remove_var("ROBLOX_OPEN_CLOUD_API_KEY");

        let result = OpenCloudClient::new();
        assert!(result.is_err());

        match result.unwrap_err() {
            RobloxMcpError::ConfigError(msg) => {
                assert!(msg.contains("ROBLOX_OPEN_CLOUD_API_KEY"));
            }
            e => panic!("Expected ConfigError, got {e:?}"),
        }
    }
}
