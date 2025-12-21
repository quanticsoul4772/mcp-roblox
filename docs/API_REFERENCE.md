# API Reference

This document provides a detailed reference for the public APIs in the Roblox Studio MCP Server.

## Core Abstractions

The server uses trait-based abstractions to enable dependency injection and testability.

### StudioBridge Trait

**Location:** `src/bridge/mod.rs`

Abstraction over Roblox Studio plugin communication.

```rust
#[async_trait]
pub trait StudioBridge: Send + Sync {
    async fn execute_command(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RobloxMcpError>;

    async fn is_connected(&self) -> bool;
}
```

#### Methods

| Method | Description | Returns |
|--------|-------------|---------|
| `execute_command(action, params)` | Execute a command via Studio plugin | `Result<Value, RobloxMcpError>` |
| `is_connected()` | Check if plugin has recent heartbeat | `bool` |

#### Implementations

| Type | Use Case |
|------|----------|
| `PluginBridge` | Production HTTP-based communication |
| `MockBridge` | Testing with predefined responses |

---

### CloudClient Trait

**Location:** `src/cloud/traits.rs`

Abstraction over Roblox Open Cloud API operations.

```rust
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
        value: serde_json::Value,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError>;

    async fn messaging_publish(
        &self,
        universe_id: u64,
        topic: &str,
        message: serde_json::Value,
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

#### Methods

| Method | Description | Returns |
|--------|-------------|---------|
| `publish_place(universe_id, place_id, file_path)` | Publish .rbxl to Roblox | `Result<PublishResult, RobloxMcpError>` |
| `datastore_get(universe_id, name, key, scope)` | Read from DataStore | `Result<DataStoreEntry, RobloxMcpError>` |
| `datastore_set(universe_id, name, key, value, scope)` | Write to DataStore | `Result<DataStoreEntry, RobloxMcpError>` |
| `messaging_publish(universe_id, topic, message)` | Publish to MessagingService | `Result<(), RobloxMcpError>` |
| `upload_asset(type, path, name, desc, creator_id)` | Upload asset to Roblox | `Result<AssetUploadResult, RobloxMcpError>` |

#### Implementations

| Type | Use Case |
|------|----------|
| `OpenCloudClient<H>` | Production with real HTTP client |
| `MockCloudClient` | Testing with queue-based responses |

---

### HttpClient Trait

**Location:** `src/http/mod.rs`

Abstraction over HTTP operations for testability.

```rust
#[async_trait]
pub trait HttpClient: Send + Sync + 'static {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, RobloxMcpError>;

    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> Result<HttpResponse, RobloxMcpError>;

    async fn post_binary(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
        query: Option<&[(&str, &str)]>,
    ) -> Result<HttpResponse, RobloxMcpError>;

    async fn post_multipart(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        form: MultipartForm,
    ) -> Result<HttpResponse, RobloxMcpError>;
}
```

#### Implementations

| Type | Use Case |
|------|----------|
| `ReqwestHttpClient` | Production with reqwest |
| `MockHttpClient` | Testing with URL-based response matching |

---

## Data Types

### PublishResult

**Location:** `src/cloud/client.rs`

Result of publishing a place to Roblox.

```rust
pub struct PublishResult {
    pub version_number: u64,
}
```

---

### DataStoreEntry

**Location:** `src/cloud/datastores.rs`

Entry from a Roblox DataStore.

```rust
pub struct DataStoreEntry {
    pub value: serde_json::Value,
    pub version: String,
    pub created_time: String,
    pub updated_time: String,
}
```

#### DataStore API Endpoint Details

The DataStore tools use Roblox Open Cloud **v1 API** with query parameters:

```
GET  https://apis.roblox.com/datastores/v1/universes/{id}/standard-datastores/datastore/entries/entry
     ?datastoreName={name}&entryKey={key}&scope={scope}

POST https://apis.roblox.com/datastores/v1/universes/{id}/standard-datastores/datastore/entries/entry
     ?datastoreName={name}&entryKey={key}&scope={scope}
```

**Required headers for POST (datastore_set):**
- `x-api-key`: From `ROBLOX_OPEN_CLOUD_API_KEY` environment variable
- `content-type`: `application/json`
- `content-md5`: Base64-encoded MD5 hash of the JSON body (required by Roblox v1 API)

---

### AssetType

**Location:** `src/cloud/assets.rs`

Type of asset for upload.

```rust
pub enum AssetType {
    Image,
    Model,
    Audio,
}
```

---

### AssetUploadResult

**Location:** `src/cloud/assets.rs`

Result of an asset upload operation.

```rust
pub struct AssetUploadResult {
    pub path: String,
    pub done: bool,
}
```

---

### HttpResponse

**Location:** `src/http/mod.rs`

HTTP response abstraction.

```rust
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool;
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, RobloxMcpError>;
    pub fn text(&self) -> Result<String, RobloxMcpError>;
}
```

---

### MultipartForm

**Location:** `src/http/mod.rs`

Builder for multipart form data.

```rust
impl MultipartForm {
    pub fn new() -> Self;
    pub fn text(self, name: impl Into<String>, value: impl Into<String>) -> Self;
    pub fn file(
        self,
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: Vec<u8>,
    ) -> Self;
}
```

---

## Error Types

### RobloxMcpError

**Location:** `src/error.rs`

All error types used by the server.

| Variant | MCP Code | Description |
|---------|----------|-------------|
| `PluginTimeout(Duration)` | -32002 | Plugin heartbeat expired |
| `PluginExecutionError(String)` | -32603 | Plugin returned error |
| `RojoSyncFailure(String)` | -32002 | Rojo sync failed |
| `InvalidStudioData(String)` | -32600 | Invalid Studio response |
| `FileSystemError { path, source }` | -32603 | File operation failed |
| `HttpClientError { status, message }` | -32600 | HTTP 4xx error |
| `HttpServerError { status, message }` | -32603 | HTTP 5xx error |
| `HttpConnectionError(String)` | -32002 | Connection failed |
| `HttpTimeoutError(String)` | -32002 | Request timed out |
| `SerializationError(serde_json::Error)` | -32603 | JSON parse failed |
| `PathTraversal(String)` | -32600 | Security violation |
| `InvalidPath(String)` | -32600 | Invalid file path |
| `WatcherError(notify::Error)` | -32603 | File watcher error |
| `OpenCloudError { status, message }` | -32600/-32603 | Open Cloud API error |
| `ConfigError(String)` | -32600 | Configuration error |

#### MCP Error Code Mapping

| Code | Meaning | Error Types |
|------|---------|-------------|
| -32600 | Invalid Request | Client errors, bad input, path issues |
| -32603 | Internal Error | Server errors, infrastructure failures |
| -32002 | Resource Unavailable | Connection/timeout errors |

---

## Server Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ROBLOX_OPEN_CLOUD_API_KEY` | No | - | API key for cloud tools |
| `ROBLOX_MCP_PORT` | No | 8080 | HTTP bridge port |
| `RUST_LOG` | No | `roblox_studio_mcp=info` | Log level |

---

## MCP Tools

The server exposes 25 MCP tools across four categories:

### Filesystem Tools (8)

| Tool | Description |
|------|-------------|
| `fs_get_tree` | List project file structure |
| `fs_read_script` | Read .luau file |
| `fs_write_script` | Write .luau file |
| `fs_delete_script` | Delete .luau file |
| `fs_search_content` | Regex search in scripts |
| `fs_get_changes` | Get file modification times |
| `fs_lint_script` | Run Selene linter |
| `fs_watch_changes` | Poll for file changes |

### Studio Tools (11)

| Tool | Description |
|------|-------------|
| `studio_health_check` | Check plugin connection |
| `studio_get_selection` | Get selected instances |
| `studio_get_datamodel` | Get DataModel hierarchy |
| `studio_get_datamodel_paginated` | Paginated DataModel |
| `studio_get_script_source` | Read script source |
| `studio_modify_script` | Update script source |
| `studio_create_instance` | Create new instance |
| `studio_set_property` | Set instance property |
| `studio_delete_instance` | Delete instance |
| `studio_find_instances` | Find by class name |
| `studio_get_output` | Get Output logs |

#### Property Type Mappings

When using `studio_create_instance` or `studio_set_property`, properties are serialized as JSON:

| Roblox Type | JSON Format | Example |
|-------------|-------------|---------|
| `Vector3` | `[x, y, z]` array | `[0, 5, 0]` |
| `Color3` | `[r, g, b]` array (0-1) | `[1, 0, 0]` for red |
| `BrickColor` | String name | `"Bright red"`, `"Cyan"` |
| `Material` | String name | `"Neon"`, `"Concrete"` |
| `Enum` | String value | `"Ball"` for Shape, `"Bottom"` for Face |
| `boolean` | Boolean | `true`, `false` |
| `number` | Number | `0.5`, `100` |
| `string` | String | `"MyName"` |

**Known Limitations:**
- `UDim2` properties not supported (use script-based modification)
- Complex Roblox types may require direct Luau script manipulation

#### Script Modification Note

Use `record_undo: false` parameter when calling `studio_modify_script` to avoid "script document not available" errors that can occur when the script editor is not open for that script.

### Cloud Tools (5)

| Tool | Description |
|------|-------------|
| `cloud_publish_place` | Publish .rbxl file |
| `cloud_upload_asset` | Upload asset |
| `cloud_datastore_get` | Read DataStore |
| `cloud_datastore_set` | Write DataStore |
| `cloud_messaging_publish` | Publish message |

### Monitoring Tools (1)

| Tool | Description |
|------|-------------|
| `server_get_metrics` | Tool execution metrics |

---

## Test Helpers

### create_test_server

Creates a server with mock bridge for testing.

```rust
pub fn create_test_server(project_root: PathBuf) -> RobloxMcpServer {
    let mock_bridge = Arc::new(MockBridge::new());
    RobloxMcpServer::new(mock_bridge, project_root)
}
```

### with_cloud_client

Injects a mock cloud client for testing cloud tools.

```rust
impl RobloxMcpServer {
    pub fn with_cloud_client(mut self, client: Arc<dyn CloudClient>) -> Self;
}
```

#### Example Usage

```rust
use crate::cloud::mock::MockCloudClient;

let mock = Arc::new(MockCloudClient::new());
mock.queue_datastore_get(Ok(DataStoreEntry {
    value: json!({"coins": 100}),
    version: "v1".to_string(),
    created_time: "2024-01-01T00:00:00Z".to_string(),
    updated_time: "2024-01-01T00:00:00Z".to_string(),
}));

let server = create_test_server(root).with_cloud_client(mock);
```
