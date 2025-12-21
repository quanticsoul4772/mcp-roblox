# Testing Patterns

This document describes the mock infrastructure and testing patterns used in the Roblox Studio MCP Server.

## Overview

The server uses dependency injection via Rust traits to enable comprehensive testing without requiring external dependencies (Roblox Studio, Open Cloud API).

**Coverage:** 499 tests, 86.7% line coverage

## Mock Infrastructure

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     RobloxMcpServer                              │
│  ┌──────────────────────┐  ┌──────────────────────┐              │
│  │  Arc<dyn StudioBridge>│  │  Arc<dyn CloudClient>│              │
│  └──────────┬───────────┘  └──────────┬───────────┘              │
│             │                          │                          │
└─────────────┼──────────────────────────┼──────────────────────────┘
              │                          │
    ┌─────────┴─────────┐      ┌─────────┴─────────┐
    │                   │      │                   │
┌───▼───┐         ┌─────▼────┐ ┌───▼───┐     ┌─────▼────────┐
│Plugin │         │ MockBridge│ │OpenCloud│   │MockCloudClient│
│Bridge │         │          │ │Client  │   │              │
└───────┘         └──────────┘ └────────┘   └──────────────┘
    │                  │            │              │
    │ HTTP             │ In-memory  │ HTTPS        │ Queue-based
    ▼                  ▼            ▼              ▼
 Studio            Test Data    Roblox API     Test Data
```

### Mock Components

| Component | Location | Purpose |
|-----------|----------|---------|
| `MockBridge` | `src/bridge/mock.rs` | Mock Studio plugin communication |
| `MockCloudClient` | `src/cloud/mock.rs` | Mock Open Cloud API |
| `MockHttpClient` | `src/http/mock.rs` | Mock HTTP responses |
| `MockLinter` | `src/tools/linting.rs` | Mock Selene linter |

---

## MockBridge

Simulates Studio plugin communication.

### Pattern: Response Map

Uses a `HashMap<String, Value>` to map action names to responses.

```rust
use crate::bridge::mock::MockBridge;

let mock = MockBridge::new();

// Set expected response for action
mock.set_response("getSelection", json!({
    "selected": ["Workspace.Part1", "Workspace.Part2"]
}));

// Execute command (returns the pre-set response)
let result = mock.execute_command("getSelection", json!({})).await;
assert_eq!(result.unwrap()["selected"][0], "Workspace.Part1");
```

### API

```rust
impl MockBridge {
    pub fn new() -> Self;

    /// Set response for an action
    pub fn set_response(&self, action: &str, response: serde_json::Value);

    /// Set error for an action
    pub fn set_error(&self, action: &str, error: RobloxMcpError);

    /// Simulate disconnected plugin
    pub fn set_disconnected(&self);
}
```

### Example: Testing Studio Tools

```rust
#[tokio::test]
async fn test_studio_get_selection() {
    let mock = MockBridge::new();
    mock.set_response("getSelection", json!({
        "selected": [
            {"Name": "Part", "ClassName": "Part", "Path": "Workspace.Part"}
        ]
    }));

    let server = create_test_server(PathBuf::from("/test"))
        .with_bridge(Arc::new(mock));

    // Call the tool
    let result = server.studio_get_selection().await;

    assert!(result.is_ok());
}
```

---

## MockCloudClient

Simulates Open Cloud API responses using a queue pattern.

### Pattern: Response Queue

Uses `Mutex<VecDeque<Result<T, E>>>` to queue responses. Each call pops the next response from the queue.

```rust
use crate::cloud::mock::MockCloudClient;

let mock = MockCloudClient::new();

// Queue success response
mock.queue_datastore_get(Ok(DataStoreEntry {
    value: json!({"coins": 100}),
    version: "v1".to_string(),
    created_time: "2024-01-01T00:00:00Z".to_string(),
    updated_time: "2024-01-01T00:00:00Z".to_string(),
}));

// First call returns queued response
let result = mock.datastore_get(123, "PlayerData", "user_1", None).await;
assert!(result.is_ok());

// Second call fails (queue empty)
let result2 = mock.datastore_get(123, "PlayerData", "user_2", None).await;
assert!(result2.is_err());
```

### API

```rust
impl MockCloudClient {
    pub fn new() -> Self;

    pub fn queue_publish_place(&self, response: Result<PublishResult, RobloxMcpError>);
    pub fn queue_datastore_get(&self, response: Result<DataStoreEntry, RobloxMcpError>);
    pub fn queue_datastore_set(&self, response: Result<DataStoreEntry, RobloxMcpError>);
    pub fn queue_messaging_publish(&self, response: Result<(), RobloxMcpError>);
    pub fn queue_upload_asset(&self, response: Result<AssetUploadResult, RobloxMcpError>);
}
```

### Example: Testing Cloud Tools

```rust
#[tokio::test]
async fn test_cloud_datastore_get_success() {
    let mock = Arc::new(MockCloudClient::new());
    mock.queue_datastore_get(Ok(DataStoreEntry {
        value: json!({"level": 5, "coins": 100}),
        version: "v123".to_string(),
        created_time: "2024-01-01T00:00:00Z".to_string(),
        updated_time: "2024-01-15T12:00:00Z".to_string(),
    }));

    let server = create_test_server(PathBuf::from("/test"))
        .with_cloud_client(mock);

    // Call cloud_datastore_get tool
    let result = server.cloud_datastore_get(CloudDatastoreGetParams {
        universe_id: 123456,
        datastore_name: "PlayerData".to_string(),
        key: "player_99".to_string(),
        scope: None,
    }).await;

    assert!(result.is_ok());
}
```

### Example: Testing Error Paths

```rust
#[tokio::test]
async fn test_cloud_api_error() {
    let mock = Arc::new(MockCloudClient::new());
    mock.queue_datastore_get(Err(RobloxMcpError::OpenCloudError {
        status: 404,
        message: "Key not found".to_string(),
    }));

    let server = create_test_server(PathBuf::from("/test"))
        .with_cloud_client(mock);

    let result = server.cloud_datastore_get(params).await;

    assert!(result.is_err());
    // Verify error code mapping
    let error: ErrorData = result.unwrap_err();
    assert_eq!(error.code, ErrorCode(-32600)); // Invalid Request for 4xx
}
```

---

## MockHttpClient

Simulates HTTP responses for testing OpenCloudClient.

### Pattern: URL Matching

Uses a `HashMap<String, HttpResponse>` to map URLs to responses.

```rust
use crate::http::mock::MockHttpClient;

let mock = MockHttpClient::new();

// Set response for URL
mock.set_response(
    "https://apis.roblox.com/datastores/v1/universes/123/standard-datastores/datastore/entries/entry",
    HttpResponse {
        status: 200,
        headers: Default::default(),
        body: br#"{"value": {"coins": 100}}"#.to_vec(),
    }
);

// HTTP call returns mocked response
let result = mock.get("https://apis.roblox.com/...", &[]).await;
```

### Example: Testing OpenCloudClient

```rust
#[tokio::test]
async fn test_datastore_get_with_mock_http() {
    let mock = MockHttpClient::new();
    mock.set_response(
        "https://apis.roblox.com/datastores/v1/universes/123/standard-datastores/datastore/entries/entry?datastoreName=PlayerData&entryKey=player_1",
        HttpResponse {
            status: 200,
            headers: Default::default(),
            body: br#"{"path":"...","value":{"coins":50}}"#.to_vec(),
        }
    );

    let client = OpenCloudClient::new("test-api-key".to_string(), mock);
    let result = client.datastore_get(123, "PlayerData", "player_1", None).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().value["coins"], 50);
}
```

---

## Testing Patterns

### 1. Unit Tests with Mocks

Test individual components in isolation.

```rust
#[tokio::test]
async fn test_unit_component() {
    // Arrange: Create mock with expected behavior
    let mock = MockBridge::new();
    mock.set_response("action", json!({"result": "success"}));

    // Act: Call component under test
    let result = component_under_test(&mock).await;

    // Assert: Verify behavior
    assert!(result.is_ok());
}
```

### 2. Server Integration Tests

Test MCP tools with injected mocks.

```rust
#[tokio::test]
async fn test_mcp_tool() {
    // Create server with mocks
    let mock_bridge = Arc::new(MockBridge::new());
    let mock_cloud = Arc::new(MockCloudClient::new());

    let server = create_test_server(PathBuf::from("/test"))
        .with_cloud_client(mock_cloud);

    // Setup expected responses
    mock_bridge.set_response("getDataModel", json!({...}));

    // Call MCP tool method directly
    let result = server.studio_get_datamodel(params).await;

    assert!(result.is_ok());
}
```

### 3. Error Path Testing

Test error handling by queuing errors.

```rust
#[tokio::test]
async fn test_handles_api_error() {
    let mock = Arc::new(MockCloudClient::new());
    mock.queue_datastore_get(Err(RobloxMcpError::OpenCloudError {
        status: 503,
        message: "Service Unavailable".to_string(),
    }));

    let server = create_test_server(root).with_cloud_client(mock);
    let result = server.cloud_datastore_get(params).await;

    // Verify error is properly converted to MCP error
    assert!(matches!(result, Err(ErrorData { code: ErrorCode(-32603), .. })));
}
```

### 4. Filesystem Tests with TempDir

Test filesystem operations in isolated directories.

```rust
#[tokio::test]
async fn test_filesystem_operations() {
    let temp = tempfile::tempdir().unwrap();
    let script_path = temp.path().join("test.luau");

    // Write file
    fs::write(&script_path, "print('hello')").unwrap();

    // Create server with temp directory
    let server = create_test_server(temp.path().to_path_buf());

    // Test read
    let content = server.fs_read_script(FsReadScriptParams {
        file_path: script_path.to_string_lossy().to_string(),
    }).await;

    assert!(content.is_ok());
}
```

---

## Test Utilities

### create_test_server

Factory function for creating test servers.

```rust
pub fn create_test_server(project_root: PathBuf) -> RobloxMcpServer {
    let mock_bridge = Arc::new(MockBridge::new());
    RobloxMcpServer::new(mock_bridge, project_root)
}
```

### Assertion Helpers

Common patterns for assertions:

```rust
// Verify success result
assert!(result.is_ok());

// Verify error type
assert!(matches!(result.unwrap_err(), RobloxMcpError::OpenCloudError { status: 404, .. }));

// Verify MCP error code
let mcp_err: ErrorData = error.into();
assert_eq!(mcp_err.code, ErrorCode(-32600));

// Verify content contains text
let content = result.unwrap().content;
assert!(content[0].as_text().unwrap().contains("expected text"));
```

---

## Coverage Goals

| Area | Target | Current |
|------|--------|---------|
| Overall | 85% | 86.7% |
| Error paths | 100% | ~95% |
| MCP tools | 90% | ~90% |
| Mock infrastructure | 100% | 100% |

### Running Coverage

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Run coverage
cargo tarpaulin --out Html

# View report
open tarpaulin-report.html
```

---

## Best Practices

### 1. Test Isolation

Each test should be independent and not share state.

```rust
#[tokio::test]
async fn test_isolated() {
    // Create fresh mocks for each test
    let mock = MockBridge::new();
    // ...
}
```

### 2. Queue Multiple Responses

For tests that make multiple calls, queue all responses upfront.

```rust
mock.queue_datastore_get(Ok(entry1));
mock.queue_datastore_get(Ok(entry2));
mock.queue_datastore_get(Err(error));

// Three calls will use these in order
```

### 3. Test Both Success and Error Paths

Always test happy path and error handling.

```rust
#[tokio::test]
async fn test_success() { ... }

#[tokio::test]
async fn test_error_404() { ... }

#[tokio::test]
async fn test_error_500() { ... }
```

### 4. Use Descriptive Test Names

```rust
#[tokio::test]
async fn test_datastore_get_returns_entry_when_key_exists() { ... }

#[tokio::test]
async fn test_datastore_get_returns_404_when_key_not_found() { ... }
```
