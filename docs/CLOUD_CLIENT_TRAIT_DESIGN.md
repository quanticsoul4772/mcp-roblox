# CloudClient Trait Refactoring Design Document

## 1. Overview

This document describes the design for making `OpenCloudClient` injectable into `RobloxMcpServer` to enable testing cloud tool success paths.

**Goal:** Enable unit testing of cloud tool success paths in `src/mcp/server.rs` by making the cloud client injectable via a trait abstraction.

**Current Coverage Impact:** The cloud tools in `server.rs` have ~70% coverage. The success paths (lines 1056-1120, 1149-1214, 1243-1299) are untested because the cloud client is a concrete type that can't be mocked.

## 2. Current Architecture

```
RobloxMcpServer<B: StudioBridge, L: Linter>
    └── cloud_client: Option<Arc<OpenCloudClient>>  // Concrete type!
            └── OpenCloudClient<H: HttpClient>       // Generic over HTTP
                    └── http: H                       // ReqwestHttpClient in prod
```

**Problem:** `RobloxMcpServer` uses concrete `OpenCloudClient` type (with default `ReqwestHttpClient`), preventing injection of mock cloud clients for testing.

**Why not just use `OpenCloudClient<MockHttpClient>`?**
The server stores `Option<Arc<OpenCloudClient>>` without the generic parameter, which defaults to `OpenCloudClient<ReqwestHttpClient>`. Changing to `OpenCloudClient<MockHttpClient>` requires a generic parameter on the server itself.

## 3. Proposed Architecture

```
RobloxMcpServer<B: StudioBridge, L: Linter>
    └── cloud_client: Option<Arc<dyn CloudClient>>  // Trait object!
            ├── OpenCloudClient<H> implements CloudClient
            └── MockCloudClient implements CloudClient
```

Using a trait object (`dyn CloudClient`) allows runtime polymorphism without adding a generic parameter to `RobloxMcpServer`.

## 4. CloudClient Trait Definition

**File: `src/cloud/mod.rs`**

```rust
use std::path::Path;
use async_trait::async_trait;
use crate::error::RobloxMcpError;

/// Trait for cloud client operations, enabling dependency injection for testing.
/// 
/// This trait abstracts the Open Cloud API operations, allowing:
/// - Production code to use `OpenCloudClient<ReqwestHttpClient>`
/// - Tests to use `MockCloudClient` with predefined responses
#[async_trait]
pub trait CloudClient: Send + Sync {
    /// Publish a place file to Roblox
    async fn publish_place(
        &self,
        universe_id: u64,
        place_id: u64,
        file_path: &Path,
    ) -> Result<PublishResult, RobloxMcpError>;

    /// Get a DataStore entry
    async fn get_entry(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError>;

    /// Set a DataStore entry
    async fn set_entry(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        value: &serde_json::Value,
        scope: Option<&str>,
    ) -> Result<DataStoreSetResult, RobloxMcpError>;

    /// Publish a message to MessagingService
    async fn publish_message(
        &self,
        universe_id: u64,
        topic: &str,
        message: &serde_json::Value,
    ) -> Result<(), RobloxMcpError>;

    /// Upload an asset to Roblox
    async fn upload_asset(
        &self,
        asset_type: AssetType,
        file_path: &Path,
        name: &str,
        description: &str,
        creator_id: u64,
    ) -> Result<AssetUploadResult, RobloxMcpError>;
}
```

## 5. Implementation for OpenCloudClient

**File: `src/cloud/client.rs`**

```rust
use super::CloudClient;
use async_trait::async_trait;

#[async_trait]
impl<H: HttpClient> CloudClient for OpenCloudClient<H> {
    async fn publish_place(
        &self,
        universe_id: u64,
        place_id: u64,
        file_path: &Path,
    ) -> Result<PublishResult, RobloxMcpError> {
        // Call existing implementation
        self.publish_place(universe_id, place_id, file_path).await
    }

    async fn get_entry(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        self.get_entry(universe_id, datastore_name, key, scope).await
    }

    async fn set_entry(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        value: &serde_json::Value,
        scope: Option<&str>,
    ) -> Result<DataStoreSetResult, RobloxMcpError> {
        self.set_entry(universe_id, datastore_name, key, value, scope).await
    }

    async fn publish_message(
        &self,
        universe_id: u64,
        topic: &str,
        message: &serde_json::Value,
    ) -> Result<(), RobloxMcpError> {
        self.publish_message(universe_id, topic, message).await
    }

    async fn upload_asset(
        &self,
        asset_type: AssetType,
        file_path: &Path,
        name: &str,
        description: &str,
        creator_id: u64,
    ) -> Result<AssetUploadResult, RobloxMcpError> {
        self.upload_asset(asset_type, file_path, name, description, creator_id).await
    }
}
```

## 6. MockCloudClient for Testing

**File: `src/cloud/mock.rs`**

```rust
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;

use super::{AssetType, AssetUploadResult, CloudClient, DataStoreEntry, DataStoreSetResult, PublishResult};
use crate::error::RobloxMcpError;

/// Mock cloud client for testing cloud tool success and error paths
pub struct MockCloudClient {
    publish_place_responses: Mutex<VecDeque<Result<PublishResult, RobloxMcpError>>>,
    get_entry_responses: Mutex<VecDeque<Result<DataStoreEntry, RobloxMcpError>>>,
    set_entry_responses: Mutex<VecDeque<Result<DataStoreSetResult, RobloxMcpError>>>,
    publish_message_responses: Mutex<VecDeque<Result<(), RobloxMcpError>>>,
    upload_asset_responses: Mutex<VecDeque<Result<AssetUploadResult, RobloxMcpError>>>,
}

impl MockCloudClient {
    pub fn new() -> Self {
        Self {
            publish_place_responses: Mutex::new(VecDeque::new()),
            get_entry_responses: Mutex::new(VecDeque::new()),
            set_entry_responses: Mutex::new(VecDeque::new()),
            publish_message_responses: Mutex::new(VecDeque::new()),
            upload_asset_responses: Mutex::new(VecDeque::new()),
        }
    }

    /// Queue a response for the next publish_place call
    pub fn queue_publish_place(&self, response: Result<PublishResult, RobloxMcpError>) {
        self.publish_place_responses.lock().unwrap().push_back(response);
    }

    /// Queue a response for the next get_entry call
    pub fn queue_get_entry(&self, response: Result<DataStoreEntry, RobloxMcpError>) {
        self.get_entry_responses.lock().unwrap().push_back(response);
    }

    /// Queue a response for the next set_entry call
    pub fn queue_set_entry(&self, response: Result<DataStoreSetResult, RobloxMcpError>) {
        self.set_entry_responses.lock().unwrap().push_back(response);
    }

    /// Queue a response for the next publish_message call
    pub fn queue_publish_message(&self, response: Result<(), RobloxMcpError>) {
        self.publish_message_responses.lock().unwrap().push_back(response);
    }

    /// Queue a response for the next upload_asset call
    pub fn queue_upload_asset(&self, response: Result<AssetUploadResult, RobloxMcpError>) {
        self.upload_asset_responses.lock().unwrap().push_back(response);
    }
}

impl Default for MockCloudClient {
    fn default() -> Self {
        Self::new()
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
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued for publish_place".into())))
    }

    async fn get_entry(
        &self,
        _universe_id: u64,
        _datastore_name: &str,
        _key: &str,
        _scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        self.get_entry_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued for get_entry".into())))
    }

    async fn set_entry(
        &self,
        _universe_id: u64,
        _datastore_name: &str,
        _key: &str,
        _value: &serde_json::Value,
        _scope: Option<&str>,
    ) -> Result<DataStoreSetResult, RobloxMcpError> {
        self.set_entry_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued for set_entry".into())))
    }

    async fn publish_message(
        &self,
        _universe_id: u64,
        _topic: &str,
        _message: &serde_json::Value,
    ) -> Result<(), RobloxMcpError> {
        self.publish_message_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued for publish_message".into())))
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
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued for upload_asset".into())))
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
    async fn test_mock_publish_place_no_response_queued() {
        let mock = MockCloudClient::new();
        
        let result = mock.publish_place(123, 456, Path::new("test.rbxl")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_multiple_responses() {
        let mock = MockCloudClient::new();
        mock.queue_publish_place(Ok(PublishResult { version_number: 1 }));
        mock.queue_publish_place(Ok(PublishResult { version_number: 2 }));
        
        let result1 = mock.publish_place(123, 456, Path::new("test.rbxl")).await.unwrap();
        let result2 = mock.publish_place(123, 456, Path::new("test.rbxl")).await.unwrap();
        
        assert_eq!(result1.version_number, 1);
        assert_eq!(result2.version_number, 2);
    }
}
```

## 7. Server Changes

**File: `src/mcp/server.rs`**

```rust
use crate::cloud::CloudClient;  // Add trait import

pub struct RobloxMcpServer<B: StudioBridge, L: Linter> {
    tool_router: ToolRouter<Self>,
    pub bridge: Arc<B>,
    pub project_root: PathBuf,
    // CHANGE: From Option<Arc<OpenCloudClient>> to Option<Arc<dyn CloudClient>>
    cloud_client: Option<Arc<dyn CloudClient>>,
    file_watcher: Option<Arc<FileWatcher>>,
    metrics: Arc<ServerMetrics>,
    linter: L,
}

impl<B: StudioBridge, L: Linter> RobloxMcpServer<B, L> {
    pub fn new(bridge: Arc<B>, project_root: PathBuf) -> Self {
        // CHANGE: Cast to trait object
        let cloud_client: Option<Arc<dyn CloudClient>> = OpenCloudClient::new()
            .ok()
            .map(|c| Arc::new(c) as Arc<dyn CloudClient>);

        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client,
            file_watcher: None,
            metrics: Arc::new(ServerMetrics::new()),
            linter: L::default(),
        }
    }

    /// Set a custom cloud client (for testing)
    #[cfg(test)]
    pub fn with_cloud_client(mut self, client: Arc<dyn CloudClient>) -> Self {
        self.cloud_client = Some(client);
        self
    }
}
```

## 8. Required Type Exports

**File: `src/cloud/datastores.rs`**

```rust
// Add new struct for set_entry result (currently uses DataStoreEntry)
#[derive(Debug, Clone)]
pub struct DataStoreSetResult {
    pub version: String,
    pub created_time: Option<String>,
    pub updated_time: Option<String>,
}
```

**File: `src/cloud/mod.rs`**

```rust
mod assets;
mod client;
mod datastores;
mod messaging;
pub mod mock;  // NEW

// Re-export types needed by the trait
pub use assets::{AssetType, AssetUploadResult};
pub use client::{OpenCloudClient, PublishResult};
pub use datastores::{DataStoreEntry, DataStoreSetResult};  // Add DataStoreSetResult
pub use messaging::MessagePublishResult;

// Export the trait
mod trait_def;
pub use trait_def::CloudClient;
```

## 9. Files to Modify

| File | Changes | Lines Est. |
|------|---------|------------|
| `Cargo.toml` | Add `async-trait = "0.1"` dependency | 1 |
| `src/cloud/mod.rs` | Add `CloudClient` trait, export `mock` module | 50 |
| `src/cloud/client.rs` | Add `impl CloudClient for OpenCloudClient<H>` | 40 |
| `src/cloud/mock.rs` | **NEW** - `MockCloudClient` implementation | 150 |
| `src/cloud/datastores.rs` | Add `DataStoreSetResult` struct | 10 |
| `src/mcp/server.rs` | Change field type, add `with_cloud_client()` | 20 |
| `src/main.rs` | No changes (backward compatible) | 0 |
| **Total** | | ~270 |

## 10. Migration Steps

1. **Add dependency:** Add `async-trait = "0.1"` to `Cargo.toml`
2. **Create trait:** Add `CloudClient` trait to `src/cloud/mod.rs`
3. **Export types:** Add `DataStoreSetResult` to datastores.rs, update mod.rs exports
4. **Implement trait:** Add `impl CloudClient for OpenCloudClient<H>` in client.rs
5. **Create mock:** Add `src/cloud/mock.rs` with `MockCloudClient`
6. **Update server:** Change `cloud_client` field type to `Option<Arc<dyn CloudClient>>`
7. **Add helper:** Add `with_cloud_client()` test helper method
8. **Add tests:** Write tests for cloud tool success paths

## 11. Example Tests

```rust
#[tokio::test]
async fn test_cloud_publish_place_success() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().to_path_buf();
    
    // Create test file
    let rbxl_path = project_root.join("game.rbxl");
    std::fs::write(&rbxl_path, b"fake content").unwrap();
    
    // Setup mock
    let mock_cloud = Arc::new(MockCloudClient::new());
    mock_cloud.queue_publish_place(Ok(PublishResult { version_number: 42 }));
    
    // Create server with mock
    let server = create_test_server(project_root)
        .with_cloud_client(mock_cloud);
    
    let params = CloudPublishPlaceParams {
        universe_id: 123,
        place_id: 456,
        rbxl_path: rbxl_path.display().to_string(),
    };
    
    let result = server.cloud_publish_place(Parameters(params)).await;
    assert!(result.is_ok());
    
    let call_result = result.unwrap();
    if let RawContent::Text(text) = &*call_result.content[0] {
        assert!(text.text.contains("42"));  // Version number
        assert!(text.text.contains("success"));
    }
}

#[tokio::test]
async fn test_cloud_datastore_get_success() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().to_path_buf();
    
    let mock_cloud = Arc::new(MockCloudClient::new());
    mock_cloud.queue_get_entry(Ok(DataStoreEntry {
        value: serde_json::json!({"coins": 100}),
        version: "v1".to_string(),
        created_time: Some("2024-01-01T00:00:00Z".to_string()),
        updated_time: Some("2024-01-02T00:00:00Z".to_string()),
    }));
    
    let server = create_test_server(project_root)
        .with_cloud_client(mock_cloud);
    
    let params = CloudDatastoreGetParams {
        universe_id: 123,
        datastore_name: "PlayerData".to_string(),
        key: "user_123".to_string(),
        scope: None,
    };
    
    let result = server.cloud_datastore_get(Parameters(params)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_cloud_messaging_publish_success() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path().to_path_buf();
    
    let mock_cloud = Arc::new(MockCloudClient::new());
    mock_cloud.queue_publish_message(Ok(()));
    
    let server = create_test_server(project_root)
        .with_cloud_client(mock_cloud);
    
    let params = CloudMessagingPublishParams {
        universe_id: 123,
        topic: "game-events".to_string(),
        message: serde_json::json!({"event": "player_joined"}),
    };
    
    let result = server.cloud_messaging_publish(Parameters(params)).await;
    assert!(result.is_ok());
}
```

## 12. Estimated Impact

| Metric | Before | After | Change |
|--------|--------|-------|--------|
| Test coverage | 82% | ~87% | +5% |
| Cloud tool coverage | ~30% | ~90% | +60% |
| Tests | 510 | ~530 | +20 |
| Lines of code | - | +270 | - |

## 13. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Trait object overhead | Negligible for cloud operations (network latency dominates) |
| Breaking changes | None - existing API unchanged, trait is additive |
| Complexity increase | Well-documented, follows existing patterns (StudioBridge, HttpClient) |

## 14. Alternatives Considered

### Alternative 1: Generic parameter on server
```rust
pub struct RobloxMcpServer<B, L, C: CloudClient>
```
**Rejected:** Too invasive, requires changes to all generic bounds.

### Alternative 2: Keep concrete type, test via MockHttpClient
```rust
OpenCloudClient::with_http(MockHttpClient::new(), "key")
```
**Rejected:** Requires third generic parameter or type erasure at HTTP level.

### Alternative 3: Integration tests only
**Rejected:** Slow, requires real API credentials, doesn't improve unit test coverage.

## 15. Conclusion

The CloudClient trait refactoring is a clean, well-bounded change that:
- Follows existing patterns in the codebase (StudioBridge, HttpClient traits)
- Enables comprehensive testing of cloud tool success paths
- Maintains backward compatibility
- Has clear migration steps
- Provides significant coverage improvement (~5%)

The refactoring should be implemented in a single PR with comprehensive tests.
