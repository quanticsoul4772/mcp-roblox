//! CloudClient trait for dependency injection
//!
//! This trait enables testing cloud tool success paths by allowing injection
//! of mock implementations into RobloxMcpServer.

use std::path::Path;

use async_trait::async_trait;

use super::{AssetType, AssetUploadResult, DataStoreEntry, PublishResult};
use crate::error::RobloxMcpError;

/// Trait for cloud client operations, enabling dependency injection for testing.
///
/// This trait abstracts the Open Cloud API operations, allowing:
/// - Production code to use `OpenCloudClient<ReqwestHttpClient>`
/// - Tests to use `MockCloudClient` with predefined responses
///
/// # Example
///
/// ```ignore
/// // Production usage (implicit)
/// let server = RobloxMcpServer::new(bridge, project_root);
///
/// // Test usage with mock
/// let mock = Arc::new(MockCloudClient::new());
/// mock.queue_datastore_get(Ok(entry));
/// let server = create_test_server(root).with_cloud_client(mock);
/// ```
#[async_trait]
pub trait CloudClient: Send + Sync {
    /// Publish a place file to Roblox
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID to publish to
    /// * `place_id` - Place ID within the universe
    /// * `file_path` - Path to the .rbxl file
    async fn publish_place(
        &self,
        universe_id: u64,
        place_id: u64,
        file_path: &Path,
    ) -> Result<PublishResult, RobloxMcpError>;

    /// Get a value from a DataStore
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID containing the DataStore
    /// * `datastore_name` - Name of the DataStore
    /// * `key` - Entry key to retrieve
    /// * `scope` - Optional scope (default: "global")
    async fn datastore_get(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError>;

    /// Set a value in a DataStore
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID containing the DataStore
    /// * `datastore_name` - Name of the DataStore
    /// * `key` - Entry key to set
    /// * `value` - JSON value to store
    /// * `scope` - Optional scope (default: "global")
    async fn datastore_set(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        value: serde_json::Value,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError>;

    /// Publish a message to MessagingService
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID to publish to
    /// * `topic` - Topic name
    /// * `message` - JSON message payload
    async fn messaging_publish(
        &self,
        universe_id: u64,
        topic: &str,
        message: serde_json::Value,
    ) -> Result<(), RobloxMcpError>;

    /// Upload an asset to Roblox
    ///
    /// # Arguments
    /// * `asset_type` - Type of asset (Image, Model, Audio)
    /// * `file_path` - Path to the asset file
    /// * `name` - Display name for the asset
    /// * `description` - Asset description
    /// * `creator_id` - Creator user ID
    async fn upload_asset(
        &self,
        asset_type: AssetType,
        file_path: &Path,
        name: &str,
        description: &str,
        creator_id: u64,
    ) -> Result<AssetUploadResult, RobloxMcpError>;
}
