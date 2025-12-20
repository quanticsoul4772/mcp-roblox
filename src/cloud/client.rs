//! Open Cloud API client with connection pooling
//!
//! This client should be created ONCE at server startup and stored in
//! RobloxMcpServer struct to benefit from HTTP connection pooling.

use crate::error::RobloxMcpError;
use crate::http::{HttpClient, ReqwestHttpClient};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tracing::instrument;

/// Result from publishing a place to Roblox
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub version_number: u64,
}

/// Open Cloud API client with connection pooling
///
/// Generic over HttpClient to allow mocking in tests.
/// Default type parameter uses ReqwestHttpClient for production.
pub struct OpenCloudClient<H: HttpClient = ReqwestHttpClient> {
    http: Arc<H>,
    api_key: String,
    base_url: String,
}

// Manual Debug implementation to avoid exposing API key in logs
impl<H: HttpClient> std::fmt::Debug for OpenCloudClient<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenCloudClient")
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl OpenCloudClient<ReqwestHttpClient> {
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

        let http = ReqwestHttpClient::new()?;

        Ok(Self {
            http: Arc::new(http),
            api_key,
            base_url: "https://apis.roblox.com".into(),
        })
    }
}

impl<H: HttpClient> OpenCloudClient<H> {
    /// Create a client with a custom HTTP implementation (for testing)
    #[cfg(test)]
    pub fn with_http(http: H, api_key: impl Into<String>) -> Self {
        Self {
            http: Arc::new(http),
            api_key: api_key.into(),
            base_url: "https://apis.roblox.com".into(),
        }
    }

    /// Get the API key (for use by extension modules)
    pub(crate) fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get the base URL (for use by extension modules)
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the HTTP client (for use by extension modules)
    pub(crate) fn http(&self) -> &H {
        &self.http
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
        let content =
            tokio::fs::read(rbxl_path)
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
            .http
            .post_binary(
                &url,
                &[
                    ("x-api-key", self.api_key.as_str()),
                    ("Content-Type", "application/octet-stream"),
                ],
                content,
                Some(&[("versionType", "Published")]),
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

        response.json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::{MockHttpClient, MockResponse};

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

    #[test]
    fn test_publish_result_serialize() {
        let result = PublishResult {
            version_number: 123,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("versionNumber"));
        assert!(json.contains("123"));
    }

    #[test]
    fn test_publish_result_clone() {
        let result = PublishResult { version_number: 99 };
        let cloned = result.clone();
        assert_eq!(cloned.version_number, 99);
    }

    #[test]
    fn test_publish_result_debug() {
        let result = PublishResult { version_number: 50 };
        let debug = format!("{:?}", result);
        assert!(debug.contains("PublishResult"));
        assert!(debug.contains("50"));
    }

    #[test]
    fn test_publish_result_roundtrip() {
        let original = PublishResult {
            version_number: 777,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: PublishResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version_number, original.version_number);
    }

    #[test]
    fn test_publish_result_large_version() {
        let json = r#"{"versionNumber": 999999999}"#;
        let result: PublishResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.version_number, 999999999);
    }

    #[test]
    fn test_publish_result_zero_version() {
        let json = r#"{"versionNumber": 0}"#;
        let result: PublishResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.version_number, 0);
    }

    #[test]
    fn test_open_cloud_client_debug_redacts_api_key() {
        let mock = MockHttpClient::new();
        let client = OpenCloudClient::with_http(mock, "secret-api-key-12345");
        let debug = format!("{:?}", client);

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("secret-api-key-12345"));
        assert!(debug.contains("OpenCloudClient"));
    }

    #[tokio::test]
    async fn test_publish_place_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({"versionNumber": 42}),
        ));

        let client = OpenCloudClient::with_http(mock, "test-api-key");

        // Create temp file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.rbxl");
        std::fs::write(&file_path, b"fake rbxl content").unwrap();

        let result = client.publish_place(123, 456, &file_path).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().version_number, 42);
    }

    #[tokio::test]
    async fn test_publish_place_api_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(401, b"Unauthorized"));

        let client = OpenCloudClient::with_http(mock, "bad-api-key");

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.rbxl");
        std::fs::write(&file_path, b"fake rbxl content").unwrap();

        let result = client.publish_place(123, 456, &file_path).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, message } => {
                assert_eq!(status, 401);
                assert!(message.contains("Unauthorized"));
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_publish_place_file_not_found() {
        let mock = MockHttpClient::new();
        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .publish_place(123, 456, std::path::Path::new("/nonexistent/file.rbxl"))
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::FileSystemError { .. }
        ));
    }

    #[tokio::test]
    async fn test_publish_place_sends_correct_headers() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({"versionNumber": 1}),
        ));

        let client = OpenCloudClient::with_http(mock.clone(), "my-api-key");

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.rbxl");
        std::fs::write(&file_path, b"content").unwrap();

        client.publish_place(111, 222, &file_path).await.unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .url
            .contains("/universes/v1/111/places/222/versions"));
        assert!(requests[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "my-api-key"));
        assert!(requests[0]
            .headers
            .iter()
            .any(|(k, v)| k == "Content-Type" && v == "application/octet-stream"));
    }

    #[tokio::test]
    async fn test_publish_place_connection_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::error("Connection refused"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.rbxl");
        std::fs::write(&file_path, b"content").unwrap();

        let result = client.publish_place(123, 456, &file_path).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::HttpConnectionError(_)
        ));
    }
}
