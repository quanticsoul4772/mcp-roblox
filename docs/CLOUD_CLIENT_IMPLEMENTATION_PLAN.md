# CloudClient Trait Implementation Plan

## Summary

This plan implements the `CloudClient` trait refactoring described in `CLOUD_CLIENT_TRAIT_DESIGN.md`. The refactoring enables testing cloud tool success paths by making `OpenCloudClient` injectable via a trait abstraction.

**Estimated Impact:**
- Coverage: 81.42% → ~87% (+5.5%)
- New tests: ~20 for cloud tool success paths
- New code: ~250 lines

---

## Phase 1: Foundation (No Breaking Changes)

### Step 1.1: Create CloudClient Trait

**File:** `src/cloud/traits.rs` (NEW)

```rust
//! CloudClient trait for dependency injection
use std::path::Path;
use async_trait::async_trait;
use crate::error::RobloxMcpError;
use super::{AssetType, AssetUploadResult, DataStoreEntry, PublishResult};

/// Trait for cloud client operations, enabling dependency injection for testing.
#[async_trait]
pub trait CloudClient: Send + Sync {
    async fn publish_place(
        &self,
        universe_id: u64,
        place_id: u64,
        file_path: &Path,
    ) -> Result<PublishResult, RobloxMcpError>;

    async fn datastore_get(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError>;

    async fn datastore_set(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        value: &serde_json::Value,
        scope: Option<&str>,
    ) -> Result<(), RobloxMcpError>;

    async fn messaging_publish(
        &self,
        universe_id: u64,
        topic: &str,
        message: &serde_json::Value,
    ) -> Result<(), RobloxMcpError>;

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

**Actions:**
1. Create `src/cloud/traits.rs` with trait definition
2. Update `src/cloud/mod.rs` to add `mod traits; pub use traits::CloudClient;`

**Validation:** `cargo check` - no errors

---

### Step 1.2: Implement CloudClient for OpenCloudClient

**File:** `src/cloud/client.rs` (MODIFY)

Add at end of file:

```rust
use super::CloudClient;

#[async_trait::async_trait]
impl<H: HttpClient> CloudClient for OpenCloudClient<H> {
    async fn publish_place(
        &self,
        universe_id: u64,
        place_id: u64,
        file_path: &Path,
    ) -> Result<PublishResult, RobloxMcpError> {
        self.publish_place(universe_id, place_id, file_path).await
    }

    async fn datastore_get(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        OpenCloudClient::datastore_get(self, universe_id, datastore_name, key, scope).await
    }

    async fn datastore_set(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        value: &serde_json::Value,
        scope: Option<&str>,
    ) -> Result<(), RobloxMcpError> {
        OpenCloudClient::datastore_set(self, universe_id, datastore_name, key, value, scope).await
    }

    async fn messaging_publish(
        &self,
        universe_id: u64,
        topic: &str,
        message: &serde_json::Value,
    ) -> Result<(), RobloxMcpError> {
        OpenCloudClient::messaging_publish(self, universe_id, topic, message).await
    }

    async fn upload_asset(
        &self,
        asset_type: AssetType,
        file_path: &Path,
        name: &str,
        description: &str,
        creator_id: u64,
    ) -> Result<AssetUploadResult, RobloxMcpError> {
        OpenCloudClient::upload_asset(self, asset_type, file_path, name, description, creator_id).await
    }
}
```

**Note:** Method names differ between trait and impl:
- Trait: `datastore_get` / Impl: `datastore_get` ✓
- Trait: `messaging_publish` / Impl: `messaging_publish` ✓

**Validation:** `cargo check` - OpenCloudClient now implements CloudClient

---

## Phase 2: MockCloudClient

### Step 2.1: Create Mock Implementation

**File:** `src/cloud/mock.rs` (NEW)

```rust
//! Mock cloud client for testing cloud tool success paths
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use async_trait::async_trait;
use super::{AssetType, AssetUploadResult, CloudClient, DataStoreEntry, PublishResult};
use crate::error::RobloxMcpError;

/// Mock cloud client with queue-based responses
#[derive(Debug, Default)]
pub struct MockCloudClient {
    publish_place_responses: Mutex<VecDeque<Result<PublishResult, RobloxMcpError>>>,
    datastore_get_responses: Mutex<VecDeque<Result<DataStoreEntry, RobloxMcpError>>>,
    datastore_set_responses: Mutex<VecDeque<Result<(), RobloxMcpError>>>,
    messaging_publish_responses: Mutex<VecDeque<Result<(), RobloxMcpError>>>,
    upload_asset_responses: Mutex<VecDeque<Result<AssetUploadResult, RobloxMcpError>>>,
}

impl MockCloudClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue_publish_place(&self, response: Result<PublishResult, RobloxMcpError>) {
        self.publish_place_responses.lock().unwrap().push_back(response);
    }

    pub fn queue_datastore_get(&self, response: Result<DataStoreEntry, RobloxMcpError>) {
        self.datastore_get_responses.lock().unwrap().push_back(response);
    }

    pub fn queue_datastore_set(&self, response: Result<(), RobloxMcpError>) {
        self.datastore_set_responses.lock().unwrap().push_back(response);
    }

    pub fn queue_messaging_publish(&self, response: Result<(), RobloxMcpError>) {
        self.messaging_publish_responses.lock().unwrap().push_back(response);
    }

    pub fn queue_upload_asset(&self, response: Result<AssetUploadResult, RobloxMcpError>) {
        self.upload_asset_responses.lock().unwrap().push_back(response);
    }
}

#[async_trait]
impl CloudClient for MockCloudClient {
    async fn publish_place(&self, _: u64, _: u64, _: &Path) -> Result<PublishResult, RobloxMcpError> {
        self.publish_place_responses
            .lock().unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued".into())))
    }

    async fn datastore_get(&self, _: u64, _: &str, _: &str, _: Option<&str>) -> Result<DataStoreEntry, RobloxMcpError> {
        self.datastore_get_responses
            .lock().unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued".into())))
    }

    async fn datastore_set(&self, _: u64, _: &str, _: &str, _: &serde_json::Value, _: Option<&str>) -> Result<(), RobloxMcpError> {
        self.datastore_set_responses
            .lock().unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued".into())))
    }

    async fn messaging_publish(&self, _: u64, _: &str, _: &serde_json::Value) -> Result<(), RobloxMcpError> {
        self.messaging_publish_responses
            .lock().unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued".into())))
    }

    async fn upload_asset(&self, _: AssetType, _: &Path, _: &str, _: &str, _: u64) -> Result<AssetUploadResult, RobloxMcpError> {
        self.upload_asset_responses
            .lock().unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(RobloxMcpError::InternalError("No mock response queued".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_publish_place_success() {
        let mock = MockCloudClient::new();
        mock.queue_publish_place(Ok(PublishResult { version_number: 42 }));
        let result = mock.publish_place(1, 2, Path::new("test.rbxl")).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version_number, 42);
    }

    #[tokio::test]
    async fn test_mock_no_response_queued() {
        let mock = MockCloudClient::new();
        let result = mock.publish_place(1, 2, Path::new("test.rbxl")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_multiple_responses() {
        let mock = MockCloudClient::new();
        mock.queue_publish_place(Ok(PublishResult { version_number: 1 }));
        mock.queue_publish_place(Ok(PublishResult { version_number: 2 }));

        assert_eq!(mock.publish_place(1, 2, Path::new("a.rbxl")).await.unwrap().version_number, 1);
        assert_eq!(mock.publish_place(1, 2, Path::new("b.rbxl")).await.unwrap().version_number, 2);
    }
}
```

**Actions:**
1. Create `src/cloud/mock.rs`
2. Update `src/cloud/mod.rs`:
   ```rust
   #[cfg(test)]
   pub mod mock;
   ```

**Validation:** `cargo test cloud::mock` - all mock tests pass

---

## Phase 3: Server Integration

### Step 3.1: Change Server Field Type

**File:** `src/mcp/server.rs` (MODIFY)

**Change 1:** Import trait
```rust
use crate::cloud::CloudClient;  // Add this
```

**Change 2:** Field type (line 72)
```rust
// FROM:
cloud_client: Option<Arc<OpenCloudClient>>,

// TO:
cloud_client: Option<Arc<dyn CloudClient>>,
```

**Change 3:** Production constructor (line 86-87)
```rust
// FROM:
let cloud_client = match OpenCloudClient::new() {
    Ok(client) => Some(Arc::new(client)),
    ...
};

// TO:
let cloud_client: Option<Arc<dyn CloudClient>> = match OpenCloudClient::new() {
    Ok(client) => Some(Arc::new(client) as Arc<dyn CloudClient>),
    ...
};
```

**Change 4:** Add test helper method (after line 146)
```rust
/// Create a test server with a mock cloud client
#[cfg(test)]
pub fn with_cloud_client(mut self, client: Arc<dyn CloudClient>) -> Self {
    self.cloud_client = Some(client);
    self
}
```

**Validation:** `cargo test` - all existing tests pass

---

### Step 3.2: Update Cloud Tool Implementations

The cloud tools in `server.rs` currently call methods directly on `OpenCloudClient`. They need to use the trait methods instead.

**Check Current Implementation:**
```rust
// Example from cloud_datastore_get (around line 1056):
let entry = self.cloud_client.as_ref().unwrap().datastore_get(...).await?;
```

This should work as-is because:
1. `cloud_client` is now `Option<Arc<dyn CloudClient>>`
2. `dyn CloudClient` has `datastore_get` method
3. Method signatures match

**Validation:** `cargo check` - verify trait method dispatch works

---

## Phase 4: Cloud Tool Success Path Tests

### Step 4.1: Add Success Path Tests

**File:** `src/mcp/server.rs` (MODIFY - tests module)

Add new test functions in the `#[cfg(test)] mod tests` section:

```rust
// === Cloud Tool Success Path Tests ===

#[tokio::test]
async fn test_cloud_datastore_get_success() {
    use crate::cloud::mock::MockCloudClient;
    use crate::cloud::DataStoreEntry;

    let temp_dir = TempDir::new().unwrap();
    let mock_cloud = Arc::new(MockCloudClient::new());
    mock_cloud.queue_datastore_get(Ok(DataStoreEntry {
        value: serde_json::json!({"coins": 100, "level": 5}),
        version: "v1".to_string(),
        created_time: "2024-01-01T00:00:00Z".to_string(),
        updated_time: "2024-01-02T00:00:00Z".to_string(),
    }));

    let server = create_test_server(temp_dir.path().to_path_buf())
        .with_cloud_client(mock_cloud);

    let params = CloudDatastoreGetParams {
        universe_id: 123,
        datastore_name: "PlayerData".to_string(),
        key: "user_123".to_string(),
        scope: None,
    };

    let result = server.cloud_datastore_get(Parameters(params)).await;
    assert!(result.is_ok());

    let call_result = result.unwrap();
    if let RawContent::Text(text) = &*call_result.content[0] {
        assert!(text.text.contains("coins"));
        assert!(text.text.contains("100"));
    }
}

#[tokio::test]
async fn test_cloud_datastore_set_success() {
    use crate::cloud::mock::MockCloudClient;

    let temp_dir = TempDir::new().unwrap();
    let mock_cloud = Arc::new(MockCloudClient::new());
    mock_cloud.queue_datastore_set(Ok(()));

    let server = create_test_server(temp_dir.path().to_path_buf())
        .with_cloud_client(mock_cloud);

    let params = CloudDatastoreSetParams {
        universe_id: 123,
        datastore_name: "PlayerData".to_string(),
        key: "user_456".to_string(),
        value: serde_json::json!({"coins": 500}),
        scope: None,
    };

    let result = server.cloud_datastore_set(Parameters(params)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_cloud_messaging_publish_success() {
    use crate::cloud::mock::MockCloudClient;

    let temp_dir = TempDir::new().unwrap();
    let mock_cloud = Arc::new(MockCloudClient::new());
    mock_cloud.queue_messaging_publish(Ok(()));

    let server = create_test_server(temp_dir.path().to_path_buf())
        .with_cloud_client(mock_cloud);

    let params = CloudMessagingPublishParams {
        universe_id: 123,
        topic: "game-events".to_string(),
        message: serde_json::json!({"event": "player_joined", "player_id": 789}),
    };

    let result = server.cloud_messaging_publish(Parameters(params)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_cloud_publish_place_success() {
    use crate::cloud::mock::MockCloudClient;
    use crate::cloud::PublishResult;

    let temp_dir = TempDir::new().unwrap();
    let rbxl_path = temp_dir.path().join("game.rbxl");
    std::fs::write(&rbxl_path, b"fake rbxl content").unwrap();

    let mock_cloud = Arc::new(MockCloudClient::new());
    mock_cloud.queue_publish_place(Ok(PublishResult { version_number: 42 }));

    let server = create_test_server(temp_dir.path().to_path_buf())
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
        assert!(text.text.contains("42"));  // version number
    }
}

#[tokio::test]
async fn test_cloud_upload_asset_success() {
    use crate::cloud::mock::MockCloudClient;
    use crate::cloud::AssetUploadResult;

    let temp_dir = TempDir::new().unwrap();
    let image_path = temp_dir.path().join("icon.png");
    std::fs::write(&image_path, b"fake png content").unwrap();

    let mock_cloud = Arc::new(MockCloudClient::new());
    mock_cloud.queue_upload_asset(Ok(AssetUploadResult {
        asset_id: 12345678,
        revision_id: Some(1),
    }));

    let server = create_test_server(temp_dir.path().to_path_buf())
        .with_cloud_client(mock_cloud);

    let params = CloudUploadAssetParams {
        asset_type: "image".to_string(),
        file_path: image_path.display().to_string(),
        name: "Test Icon".to_string(),
        description: "A test icon".to_string(),
        creator_id: 999,
    };

    let result = server.cloud_upload_asset(Parameters(params)).await;
    assert!(result.is_ok());

    let call_result = result.unwrap();
    if let RawContent::Text(text) = &*call_result.content[0] {
        assert!(text.text.contains("12345678"));  // asset ID
    }
}
```

**Validation:** `cargo test cloud` - all cloud tests pass

---

## Phase 5: Documentation & Cleanup

### Step 5.1: Update Module Documentation

**File:** `src/cloud/mod.rs` (MODIFY)

```rust
//! Open Cloud integration for Roblox Studio MCP Server
//!
//! Provides CI/CD automation capabilities:
//! - Publish places to Roblox
//! - Upload assets (images, models, audio)
//! - Manage DataStores
//! - Publish messages via MessagingService
//!
//! # Architecture
//!
//! The [`CloudClient`] trait enables dependency injection for testing:
//! - Production: [`OpenCloudClient`] with real HTTP client
//! - Testing: [`mock::MockCloudClient`] with queued responses

mod assets;
mod client;
mod datastores;
mod messaging;
mod traits;

#[cfg(test)]
pub mod mock;

// Re-export public API types
pub use assets::{AssetType, AssetUploadResult};
pub use client::{OpenCloudClient, PublishResult};
pub use datastores::DataStoreEntry;
pub use messaging::MessagePublishResult;
pub use traits::CloudClient;
```

### Step 5.2: Update README and CLAUDE.md

Update test counts after implementation.

---

## Execution Checklist

| Step | Description | Validation |
|------|-------------|------------|
| 1.1 | Create `traits.rs` with CloudClient trait | `cargo check` |
| 1.2 | Implement CloudClient for OpenCloudClient | `cargo check` |
| 2.1 | Create `mock.rs` with MockCloudClient | `cargo test cloud::mock` |
| 3.1 | Change server field to `Arc<dyn CloudClient>` | `cargo test` |
| 3.2 | Add `with_cloud_client()` test helper | `cargo check` |
| 4.1 | Add cloud tool success path tests | `cargo test cloud` |
| 5.1 | Update documentation | Manual review |
| 5.2 | Update README/CLAUDE.md test counts | Manual |

**Final Validation:** `cargo test` - all 500+ tests pass

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Trait object overhead | Negligible - cloud ops are network-bound |
| Breaking existing tests | Phase 3.1 includes existing test validation |
| Method name mismatches | Design doc verified against actual code |

---

## Expected Outcomes

| Metric | Before | After |
|--------|--------|-------|
| Unit tests | 482 | ~502 |
| Line coverage | 81.42% | ~87% |
| Cloud tool coverage | ~30% | ~90% |
| server.rs coverage | 76.3% | ~85% |
