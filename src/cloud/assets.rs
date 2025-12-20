//! Open Cloud asset upload functionality
//!
//! Upload images, models, and audio to Roblox via Open Cloud API.

use crate::error::RobloxMcpError;
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

impl super::OpenCloudClient {
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
        let content = tokio::fs::read(file_path)
            .await
            .map_err(|e| RobloxMcpError::FileSystemError {
                path: file_path.display().to_string(),
                source: e,
            })?;

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("asset");

        // Build multipart form
        let form = reqwest::multipart::Form::new()
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
            .part(
                "fileContent",
                reqwest::multipart::Part::bytes(content)
                    .file_name(file_name.to_string())
                    .mime_str(asset_type.content_type())
                    .map_err(|e| RobloxMcpError::ConfigError(e.to_string()))?,
            );

        let url = format!("{}/assets/v1/assets", self.base_url());

        let response = self
            .client()
            .post(&url)
            .header("x-api-key", self.api_key())
            .multipart(form)
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

        response.json().await.map_err(RobloxMcpError::from_reqwest)
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
}
