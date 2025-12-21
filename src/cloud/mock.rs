//! Mock cloud client for testing cloud tool success paths
//!
//! Provides a queue-based mock implementation of [`CloudClient`] that allows
//! tests to inject predetermined responses for cloud operations.
//!
//! # Example
//!
//! ```ignore
//! let mock = MockCloudClient::new();
//! mock.queue_datastore_get(Ok(DataStoreEntry {
//!     value: serde_json::json!({"coins": 100}),
//!     version: "v1".to_string(),
//!     created_time: "2024-01-01T00:00:00Z".to_string(),
//!     updated_time: "2024-01-01T00:00:00Z".to_string(),
//! }));
//!
//! let server = create_test_server(root).with_cloud_client(Arc::new(mock));
//! ```

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;

use super::{AssetType, AssetUploadResult, CloudClient, DataStoreEntry, PublishResult};
use crate::error::RobloxMcpError;

/// Mock cloud client with queue-based responses for testing
///
/// Each operation has a queue of responses. When an operation is called,
/// the next response is popped from the queue and returned. If the queue
/// is empty, an error is returned.
#[derive(Debug, Default)]
pub struct MockCloudClient {
    publish_place_responses: Mutex<VecDeque<Result<PublishResult, RobloxMcpError>>>,
    datastore_get_responses: Mutex<VecDeque<Result<DataStoreEntry, RobloxMcpError>>>,
    datastore_set_responses: Mutex<VecDeque<Result<DataStoreEntry, RobloxMcpError>>>,
    messaging_publish_responses: Mutex<VecDeque<Result<(), RobloxMcpError>>>,
    upload_asset_responses: Mutex<VecDeque<Result<AssetUploadResult, RobloxMcpError>>>,
}

impl MockCloudClient {
    /// Create a new mock cloud client with empty response queues
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a response for the next `publish_place` call
    pub fn queue_publish_place(&self, response: Result<PublishResult, RobloxMcpError>) {
        self.publish_place_responses
            .lock()
            .unwrap()
            .push_back(response);
    }

    /// Queue a response for the next `datastore_get` call
    pub fn queue_datastore_get(&self, response: Result<DataStoreEntry, RobloxMcpError>) {
        self.datastore_get_responses
            .lock()
            .unwrap()
            .push_back(response);
    }

    /// Queue a response for the next `datastore_set` call
    pub fn queue_datastore_set(&self, response: Result<DataStoreEntry, RobloxMcpError>) {
        self.datastore_set_responses
            .lock()
            .unwrap()
            .push_back(response);
    }

    /// Queue a response for the next `messaging_publish` call
    pub fn queue_messaging_publish(&self, response: Result<(), RobloxMcpError>) {
        self.messaging_publish_responses
            .lock()
            .unwrap()
            .push_back(response);
    }

    /// Queue a response for the next `upload_asset` call
    pub fn queue_upload_asset(&self, response: Result<AssetUploadResult, RobloxMcpError>) {
        self.upload_asset_responses
            .lock()
            .unwrap()
            .push_back(response);
    }
}

#[async_trait]
impl CloudClient for MockCloudClient {
    async fn publish_place(
        &self,
        _universe_id: u64,
        _place_id: u64,
        _file_path: &Path,
    ) -> Result<PublishResult, RobloxMcpError> {
        self.publish_place_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Err(RobloxMcpError::InvalidStudioData(
                    "MockCloudClient: No response queued for publish_place".into(),
                ))
            })
    }

    async fn datastore_get(
        &self,
        _universe_id: u64,
        _datastore_name: &str,
        _key: &str,
        _scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        self.datastore_get_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Err(RobloxMcpError::InvalidStudioData(
                    "MockCloudClient: No response queued for datastore_get".into(),
                ))
            })
    }

    async fn datastore_set(
        &self,
        _universe_id: u64,
        _datastore_name: &str,
        _key: &str,
        _value: serde_json::Value,
        _scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        self.datastore_set_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Err(RobloxMcpError::InvalidStudioData(
                    "MockCloudClient: No response queued for datastore_set".into(),
                ))
            })
    }

    async fn messaging_publish(
        &self,
        _universe_id: u64,
        _topic: &str,
        _message: serde_json::Value,
    ) -> Result<(), RobloxMcpError> {
        self.messaging_publish_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Err(RobloxMcpError::InvalidStudioData(
                    "MockCloudClient: No response queued for messaging_publish".into(),
                ))
            })
    }

    async fn upload_asset(
        &self,
        _asset_type: AssetType,
        _file_path: &Path,
        _name: &str,
        _description: &str,
        _creator_id: u64,
    ) -> Result<AssetUploadResult, RobloxMcpError> {
        self.upload_asset_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Err(RobloxMcpError::InvalidStudioData(
                    "MockCloudClient: No response queued for upload_asset".into(),
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_publish_place_success() {
        let mock = MockCloudClient::new();
        mock.queue_publish_place(Ok(PublishResult { version_number: 42 }));

        let result = mock.publish_place(123, 456, Path::new("test.rbxl")).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().version_number, 42);
    }

    #[tokio::test]
    async fn test_mock_publish_place_no_response() {
        let mock = MockCloudClient::new();

        let result = mock.publish_place(123, 456, Path::new("test.rbxl")).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::InvalidStudioData(_)
        ));
    }

    #[tokio::test]
    async fn test_mock_multiple_responses() {
        let mock = MockCloudClient::new();
        mock.queue_publish_place(Ok(PublishResult { version_number: 1 }));
        mock.queue_publish_place(Ok(PublishResult { version_number: 2 }));

        let r1 = mock.publish_place(1, 1, Path::new("a.rbxl")).await.unwrap();
        let r2 = mock.publish_place(1, 1, Path::new("b.rbxl")).await.unwrap();

        assert_eq!(r1.version_number, 1);
        assert_eq!(r2.version_number, 2);
    }

    #[tokio::test]
    async fn test_mock_datastore_get_success() {
        let mock = MockCloudClient::new();
        mock.queue_datastore_get(Ok(DataStoreEntry {
            value: serde_json::json!({"coins": 100}),
            version: "v1".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-01T00:00:00Z".to_string(),
        }));

        let result = mock
            .datastore_get(123, "PlayerData", "user_123", None)
            .await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.value["coins"], 100);
        assert_eq!(entry.version, "v1");
    }

    #[tokio::test]
    async fn test_mock_datastore_set_success() {
        let mock = MockCloudClient::new();
        mock.queue_datastore_set(Ok(DataStoreEntry {
            value: serde_json::json!({"coins": 500}),
            version: "v2".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-02T00:00:00Z".to_string(),
        }));

        let result = mock
            .datastore_set(
                123,
                "PlayerData",
                "user_456",
                serde_json::json!({"coins": 500}),
                None,
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_messaging_publish_success() {
        let mock = MockCloudClient::new();
        mock.queue_messaging_publish(Ok(()));

        let result = mock
            .messaging_publish(
                123,
                "game-events",
                serde_json::json!({"event": "player_joined"}),
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_upload_asset_success() {
        let mock = MockCloudClient::new();
        mock.queue_upload_asset(Ok(AssetUploadResult {
            path: "assets/v1/operations/12345".to_string(),
            done: true,
        }));

        let result = mock
            .upload_asset(
                AssetType::Image,
                Path::new("icon.png"),
                "Test Icon",
                "A test icon",
                999,
            )
            .await;

        assert!(result.is_ok());
        let upload = result.unwrap();
        assert!(upload.done);
    }

    #[tokio::test]
    async fn test_mock_queued_error() {
        let mock = MockCloudClient::new();
        mock.queue_datastore_get(Err(RobloxMcpError::OpenCloudError {
            status: 404,
            message: "Key not found".to_string(),
        }));

        let result = mock
            .datastore_get(123, "PlayerData", "nonexistent", None)
            .await;

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
    async fn test_mock_default() {
        let mock = MockCloudClient::default();

        // All queues should be empty
        let result = mock.publish_place(1, 1, Path::new("test.rbxl")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_debug() {
        let mock = MockCloudClient::new();
        let debug = format!("{:?}", mock);
        assert!(debug.contains("MockCloudClient"));
    }
}
