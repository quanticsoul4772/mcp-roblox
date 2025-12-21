//! Open Cloud asset upload functionality
//!
//! Upload images, models, and audio to Roblox via Open Cloud API.

use crate::error::RobloxMcpError;
use crate::http::{HttpClient, MultipartForm};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Result from uploading an asset
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUploadResult {
    /// The operation path for tracking upload status
    pub path: String,
    /// Whether the operation is complete
    #[serde(default)]
    pub done: bool,
}

/// Supported asset types for upload
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetType {
    Image,
    Model,
    Audio,
}

impl AssetType {
    /// Parse asset type from string
    pub fn from_str(s: &str) -> Result<Self, RobloxMcpError> {
        match s.to_lowercase().as_str() {
            "image" | "decal" | "png" | "jpg" | "jpeg" => Ok(AssetType::Image),
            "model" | "rbxm" | "rbxmx" => Ok(AssetType::Model),
            "audio" | "ogg" | "mp3" => Ok(AssetType::Audio),
            _ => Err(RobloxMcpError::ConfigError(format!(
                "Invalid asset type: '{}'. Valid types: image, model, audio",
                s
            ))),
        }
    }

    /// Get the content type for HTTP upload
    pub fn content_type(&self) -> &'static str {
        match self {
            AssetType::Image => "image/png",
            AssetType::Model => "application/octet-stream",
            AssetType::Audio => "audio/ogg",
        }
    }

    /// Get the API asset type string
    pub fn api_type(&self) -> &'static str {
        match self {
            AssetType::Image => "Decal",
            AssetType::Model => "Model",
            AssetType::Audio => "Audio",
        }
    }
}

impl<H: HttpClient> super::OpenCloudClient<H> {
    /// Upload an asset to Roblox via Open Cloud API
    ///
    /// # Arguments
    /// * `asset_type` - Type of asset (image, model, audio)
    /// * `file_path` - Path to the asset file
    /// * `name` - Display name for the asset
    /// * `description` - Asset description
    /// * `creator_id` - Creator user ID (for user-created assets)
    ///
    /// # Errors
    /// Returns error if file cannot be read or API call fails
    pub async fn upload_asset(
        &self,
        asset_type: AssetType,
        file_path: &Path,
        name: &str,
        description: &str,
        creator_id: u64,
    ) -> Result<AssetUploadResult, RobloxMcpError> {
        // Read asset file
        let content =
            tokio::fs::read(file_path)
                .await
                .map_err(|e| RobloxMcpError::FileSystemError {
                    path: file_path.display().to_string(),
                    source: e,
                })?;

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("asset");

        // Build multipart form using our abstraction
        let form = MultipartForm::new()
            .text(
                "request",
                serde_json::json!({
                    "assetType": asset_type.api_type(),
                    "displayName": name,
                    "description": description,
                    "creationContext": {
                        "creator": {
                            "userId": creator_id.to_string()
                        }
                    }
                })
                .to_string(),
            )
            .file(
                "fileContent",
                file_name.to_string(),
                asset_type.content_type().to_string(),
                content,
            );

        let url = format!("{}/assets/v1/assets", self.base_url());

        let response = self
            .http()
            .post_multipart(&url, &[("x-api-key", self.api_key())], form)
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

    #[test]
    fn test_asset_type_from_str() {
        assert_eq!(AssetType::from_str("image").unwrap(), AssetType::Image);
        assert_eq!(AssetType::from_str("IMAGE").unwrap(), AssetType::Image);
        assert_eq!(AssetType::from_str("model").unwrap(), AssetType::Model);
        assert_eq!(AssetType::from_str("audio").unwrap(), AssetType::Audio);
        assert!(AssetType::from_str("invalid").is_err());
    }

    #[test]
    fn test_asset_type_content_type() {
        assert_eq!(AssetType::Image.content_type(), "image/png");
        assert_eq!(AssetType::Model.content_type(), "application/octet-stream");
        assert_eq!(AssetType::Audio.content_type(), "audio/ogg");
    }

    #[test]
    fn test_asset_type_api_type() {
        assert_eq!(AssetType::Image.api_type(), "Decal");
        assert_eq!(AssetType::Model.api_type(), "Model");
        assert_eq!(AssetType::Audio.api_type(), "Audio");
    }

    // Additional tests for edge cases

    #[test]
    fn test_asset_type_from_str_all_variants() {
        // Image variants
        assert_eq!(AssetType::from_str("decal").unwrap(), AssetType::Image);
        assert_eq!(AssetType::from_str("png").unwrap(), AssetType::Image);
        assert_eq!(AssetType::from_str("jpg").unwrap(), AssetType::Image);
        assert_eq!(AssetType::from_str("jpeg").unwrap(), AssetType::Image);

        // Model variants
        assert_eq!(AssetType::from_str("rbxm").unwrap(), AssetType::Model);
        assert_eq!(AssetType::from_str("rbxmx").unwrap(), AssetType::Model);

        // Audio variants
        assert_eq!(AssetType::from_str("ogg").unwrap(), AssetType::Audio);
        assert_eq!(AssetType::from_str("mp3").unwrap(), AssetType::Audio);
    }

    #[test]
    fn test_asset_type_from_str_case_insensitive() {
        assert_eq!(AssetType::from_str("IMAGE").unwrap(), AssetType::Image);
        assert_eq!(AssetType::from_str("Image").unwrap(), AssetType::Image);
        assert_eq!(AssetType::from_str("iMaGe").unwrap(), AssetType::Image);
        assert_eq!(AssetType::from_str("MODEL").unwrap(), AssetType::Model);
        assert_eq!(AssetType::from_str("AUDIO").unwrap(), AssetType::Audio);
    }

    #[test]
    fn test_asset_type_from_str_error_message() {
        let result = AssetType::from_str("unknown_type");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown_type"));
        assert!(msg.contains("Invalid asset type"));
    }

    #[test]
    fn test_asset_upload_result_deserialize() {
        let json = r#"{"path": "operations/123", "done": true}"#;
        let result: AssetUploadResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.path, "operations/123");
        assert!(result.done);
    }

    #[test]
    fn test_asset_upload_result_deserialize_not_done() {
        let json = r#"{"path": "operations/456", "done": false}"#;
        let result: AssetUploadResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.path, "operations/456");
        assert!(!result.done);
    }

    #[test]
    fn test_asset_upload_result_deserialize_missing_done() {
        let json = r#"{"path": "operations/789"}"#;
        let result: AssetUploadResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.path, "operations/789");
        assert!(!result.done); // default value
    }

    #[test]
    fn test_asset_upload_result_serialize() {
        let result = AssetUploadResult {
            path: "test/path".to_string(),
            done: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test/path"));
        assert!(json.contains("true"));
    }

    #[test]
    fn test_asset_upload_result_clone() {
        let original = AssetUploadResult {
            path: "clone/test".to_string(),
            done: true,
        };
        let cloned = original.clone();
        assert_eq!(cloned.path, original.path);
        assert_eq!(cloned.done, original.done);
    }

    #[test]
    fn test_asset_upload_result_debug() {
        let result = AssetUploadResult {
            path: "debug/test".to_string(),
            done: false,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("AssetUploadResult"));
        assert!(debug.contains("debug/test"));
    }

    #[test]
    fn test_asset_type_clone() {
        let original = AssetType::Image;
        let cloned = original;
        assert_eq!(cloned, original);
    }

    #[test]
    fn test_asset_type_copy() {
        let original = AssetType::Model;
        let copied: AssetType = original; // Copy trait
        assert_eq!(copied, original);
    }

    #[test]
    fn test_asset_type_debug() {
        let debug = format!("{:?}", AssetType::Audio);
        assert!(debug.contains("Audio"));
    }

    #[test]
    fn test_asset_type_eq() {
        assert_eq!(AssetType::Image, AssetType::Image);
        assert_ne!(AssetType::Image, AssetType::Model);
        assert_ne!(AssetType::Model, AssetType::Audio);
    }

    #[test]
    fn test_asset_upload_result_roundtrip() {
        let original = AssetUploadResult {
            path: "roundtrip/test/path".to_string(),
            done: true,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: AssetUploadResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.path, original.path);
        assert_eq!(parsed.done, original.done);
    }

    // ========================================
    // Mock-based tests for upload_asset
    // ========================================
    use crate::cloud::OpenCloudClient;
    use crate::http::mock::{MockHttpClient, MockResponse};

    #[tokio::test]
    async fn test_upload_asset_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({"path": "operations/test-op-123", "done": false}),
        ));

        let client = OpenCloudClient::with_http(mock, "test-api-key");

        // Create temp file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_image.png");
        std::fs::write(&file_path, b"fake image content").unwrap();

        let result = client
            .upload_asset(
                AssetType::Image,
                &file_path,
                "Test Image",
                "A test image description",
                12345,
            )
            .await;

        assert!(result.is_ok());
        let upload_result = result.unwrap();
        assert_eq!(upload_result.path, "operations/test-op-123");
        assert!(!upload_result.done);
    }

    #[tokio::test]
    async fn test_upload_asset_api_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(
            403,
            b"Forbidden: Insufficient permissions",
        ));

        let client = OpenCloudClient::with_http(mock, "bad-key");

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.png");
        std::fs::write(&file_path, b"content").unwrap();

        let result = client
            .upload_asset(AssetType::Image, &file_path, "Test", "Desc", 123)
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, message } => {
                assert_eq!(status, 403);
                assert!(message.contains("Forbidden"));
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_upload_asset_file_not_found() {
        let mock = MockHttpClient::new();
        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .upload_asset(
                AssetType::Model,
                std::path::Path::new("/nonexistent/file.rbxm"),
                "Test",
                "Desc",
                123,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::FileSystemError { .. }
        ));
    }

    #[tokio::test]
    async fn test_upload_asset_sends_correct_headers() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({"path": "ops/123", "done": true}),
        ));

        let client = OpenCloudClient::with_http(mock.clone(), "my-secret-key");

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("audio.ogg");
        std::fs::write(&file_path, b"audio content").unwrap();

        client
            .upload_asset(AssetType::Audio, &file_path, "Sound", "A sound", 999)
            .await
            .unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.contains("/assets/v1/assets"));
        assert!(requests[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "my-secret-key"));
    }

    #[tokio::test]
    async fn test_upload_asset_connection_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::error("Network unreachable"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("model.rbxm");
        std::fs::write(&file_path, b"model data").unwrap();

        let result = client
            .upload_asset(AssetType::Model, &file_path, "Model", "Desc", 123)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::HttpConnectionError(_)
        ));
    }

    #[tokio::test]
    async fn test_upload_asset_all_types() {
        for (asset_type, expected_content_type) in [
            (AssetType::Image, "image/png"),
            (AssetType::Model, "application/octet-stream"),
            (AssetType::Audio, "audio/ogg"),
        ] {
            let mock = MockHttpClient::new();
            mock.queue_response(MockResponse::json(
                200,
                serde_json::json!({"path": "ops/1", "done": true}),
            ));

            let client = OpenCloudClient::with_http(mock, "key");

            let temp_dir = tempfile::tempdir().unwrap();
            let file_path = temp_dir.path().join("test_file");
            std::fs::write(&file_path, b"content").unwrap();

            let result = client
                .upload_asset(asset_type, &file_path, "Test", "Desc", 1)
                .await;

            assert!(
                result.is_ok(),
                "Failed for {:?} with content_type {}",
                asset_type,
                expected_content_type
            );
        }
    }
}
