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

    async fn ordered_datastore_list(
        &self,
        params: OrderedDataStoreListParams,
    ) -> Result<OrderedDataStoreListResult, RobloxMcpError>;

    async fn ordered_datastore_set(
        &self,
        universe_id: u64,
        datastore_name: &str,
        scope: Option<&str>,
        entry_id: &str,
        value: i64,
    ) -> Result<OrderedDataStoreEntry, RobloxMcpError>;

    async fn ordered_datastore_increment(
        &self,
        universe_id: u64,
        datastore_name: &str,
        scope: Option<&str>,
        entry_id: &str,
        increment: i64,
    ) -> Result<OrderedDataStoreEntry, RobloxMcpError>;

    async fn ordered_datastore_delete(
        &self,
        universe_id: u64,
        datastore_name: &str,
        scope: Option<&str>,
        entry_id: &str,
    ) -> Result<(), RobloxMcpError>;

    async fn get_universe(
        &self,
        universe_id: u64,
    ) -> Result<UniverseInfo, RobloxMcpError>;

    async fn restart_servers(
        &self,
        universe_id: u64,
    ) -> Result<(), RobloxMcpError>;
}
```

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

    async fn delete(
        &self,
        url: &str,
        headers: &[(&str, &str)],
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

---

### OrderedDataStoreEntry

**Location:** `src/cloud/ordered_datastores.rs`

Entry from a Roblox OrderedDataStore (used for leaderboards).

```rust
pub struct OrderedDataStoreEntry {
    pub id: String,
    pub value: i64,
}
```

---

### OrderedDataStoreListResult

**Location:** `src/cloud/ordered_datastores.rs`

Result of listing OrderedDataStore entries.

```rust
pub struct OrderedDataStoreListResult {
    pub entries: Vec<OrderedDataStoreEntry>,
    pub next_page_token: Option<String>,
}
```

---

### UniverseInfo

**Location:** `src/cloud/universes.rs`

Information about a Roblox universe (game).

```rust
pub struct UniverseInfo {
    pub path: String,
    pub create_time: String,
    pub update_time: String,
    pub display_name: String,
    pub description: String,
    pub user: Option<String>,
    pub group: Option<String>,
    pub visibility: Option<String>,
}
```

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
| `ToolNotInstalled { tool, install_hint }` | -32002 | External tool not found |
| `ToolExecutionError { tool, message }` | -32603 | External tool failed |

#### MCP Error Code Mapping

| Code | Meaning | Error Types |
|------|---------|-------------|
| -32600 | Invalid Request | Client errors, bad input, path issues |
| -32603 | Internal Error | Server errors, infrastructure failures |
| -32002 | Resource Unavailable | Connection/timeout errors, missing tools |

---

## Server Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ROBLOX_OPEN_CLOUD_API_KEY` | No | - | API key for cloud tools (protected with secrecy) |
| `ROBLOX_MCP_PORT` | No | 8080 | HTTP bridge port |
| `RUST_LOG` | No | `roblox_studio_mcp=info` | Log level |

---

## Resource Limits

**Location:** `src/limits.rs`

| Constant | Value | Description |
|----------|-------|-------------|
| `MAX_SEARCH_RESULTS` | 1000 | Max lines returned by fs_search_content |
| `MAX_FILE_ENTRIES` | 10000 | Max files tracked by fs_get_changes |
| `MAX_TREE_ENTRIES` | 10000 | Max entries in build_tree output |

---

## Security Functions

### validate_regex_safety

**Location:** `src/regex_safety.rs`

Validates regex patterns before compilation to prevent DoS attacks.

```rust
pub fn validate_regex_safety(pattern: &str) -> Result<regex::Regex, RobloxMcpError>
```

Rejects patterns containing:
- `(.*)*`, `(.+)+` - Nested quantifiers
- `(a+)+`, `(a*)*` - Catastrophic backtracking patterns
- `(a|aa)+`, `(a|a?)+` - Alternation with overlap
- `(.*)\1`, `(.+)\1+` - Backreferences with quantifiers

Also enforces a 1MB size limit via `RegexBuilder::size_limit()`.

### execute_with_timeout

**Location:** `src/tools/timeout.rs`

Executes external tools with timeout protection.

```rust
pub async fn execute_with_timeout(
    cmd: Command,
    tool_name: &str,
    timeout_duration: Option<Duration>,
) -> Result<Output, RobloxMcpError>
```

Default timeout: 30 seconds. Used for StyLua, Selene, Rojo, Wally, and Moonwave.

---

## MCP Tools

The server exposes 47 MCP tools across six categories:

### Filesystem Tools (8)

| Tool | Description |
|------|-------------|
| `fs_get_tree` | List project file structure |
| `fs_read_script` | Read .luau file |
| `fs_write_script` | Write .luau file |
| `fs_delete_script` | Delete .luau file |
| `fs_search_content` | Regex search in scripts (with DoS protection) |
| `fs_get_changes` | Get file modification times |
| `fs_lint_script` | Run Selene linter |
| `fs_watch_changes` | Poll for file changes |

### Studio Tools (14)

| Tool | Description |
|------|-------------|
| `studio_health_check` | Check plugin connection |
| `studio_get_selection` | Get selected instances |
| `studio_get_datamodel` | Get DataModel hierarchy |
| `studio_get_datamodel_paginated` | Paginated DataModel |
| `studio_get_script_source` | Read script source |
| `studio_get_properties` | Read instance properties |
| `studio_get_bounds` | Get bounding box of Part/Model |
| `studio_modify_script` | Update script source |
| `studio_create_instance` | Create new instance |
| `studio_insert_r15_rig` | Insert complete R15 humanoid rig |
| `studio_set_property` | Set instance property |
| `studio_delete_instance` | Delete instance |
| `studio_find_instances` | Find by class name |
| `studio_get_output` | Get Output logs |
| `studio_generate_mesh` | Generate 3D mesh from text using TRELLIS |

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

#### studio_get_properties

Read properties from any instance in Studio.

```
studio_get_properties(path, properties?)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `path` | string | Instance path (e.g., `"Workspace.MyPart"`) |
| `properties` | string[] | Optional list of property names. If omitted, returns common properties for the class |

**Response:**
```json
{
  "className": "Part",
  "path": "Workspace.MyPart",
  "properties": {
    "Position": {"type": "Vector3", "value": [0, 5, 0]},
    "Size": {"type": "Vector3", "value": [4, 1, 2]},
    "Anchored": true,
    "Material": "Plastic"
  }
}
```

#### studio_get_bounds

Get the bounding box of a BasePart or Model.

```
studio_get_bounds(path)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `path` | string | Instance path to a BasePart or Model |

**Response:**
```json
{
  "center": {"X": 100, "Y": 5, "Z": -20},
  "size": {"X": 10, "Y": 8, "Z": 12},
  "min": {"X": 95, "Y": 1, "Z": -26},
  "max": {"X": 105, "Y": 9, "Z": -14},
  "orientation": {"X": 0, "Y": 0, "Z": 0}
}
```

Useful for calculating furniture placement, collision detection, and verifying build dimensions.

#### studio_generate_mesh

Generate 3D meshes from text prompts using AI.

```
studio_generate_mesh(prompt, parent?, name?, record_undo?)
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `prompt` | string | Text description of the 3D object (e.g., "wooden treasure chest") |
| `parent` | string | Optional parent path (default: "game.Workspace") |
| `name` | string | Optional name for the MeshPart (default: "GeneratedMesh") |
| `record_undo` | boolean | Create undo waypoint (default: true) |

**Response:**
```json
{
  "success": true,
  "path": "game.Workspace.TreasureChest",
  "vertexCount": 1024,
  "faceCount": 2048,
  "provider": "trellis"
}
```

**Environment Variables:**
- `RUNPOD_API_KEY` + `TRELLIS_ENDPOINT_ID` + `HF_TOKEN`

**Timing:** 1-3 minutes depending on GPU availability and cold start state.

#### Script Modification Note

Use `record_undo: false` parameter when calling `studio_modify_script` to avoid "script document not available" errors that can occur when the script editor is not open for that script.

### Cloud Tools (11)

| Tool | Description |
|------|-------------|
| `cloud_publish_place` | Publish .rbxl file |
| `cloud_upload_asset` | Upload asset |
| `cloud_datastore_get` | Read DataStore |
| `cloud_datastore_set` | Write DataStore |
| `cloud_ordered_datastore_list` | List OrderedDataStore entries (leaderboards) |
| `cloud_ordered_datastore_set` | Set OrderedDataStore entry |
| `cloud_ordered_datastore_increment` | Atomically increment entry |
| `cloud_ordered_datastore_delete` | Delete OrderedDataStore entry |
| `cloud_get_universe` | Get universe metadata |
| `cloud_restart_servers` | Restart all game servers |
| `cloud_messaging_publish` | Publish message |

#### DataStore API Endpoint Details

Uses Roblox Open Cloud **v1 API** with query parameters:

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

#### OrderedDataStore API

For leaderboards and ranked data:

```
GET https://apis.roblox.com/ordered-data-stores/v1/universes/{id}/orderedDataStores/{name}/scopes/{scope}/entries
    ?max_page_size={limit}&order_by={desc|asc}&filter={expression}
```

### Toolchain Tools (9)

| Tool | Description |
|------|-------------|
| `stylua_format` | Format Luau with StyLua |
| `rojo_build` | Build Roblox project |
| `rojo_sourcemap` | Generate sourcemap |
| `wally_install` | Install packages |
| `wally_update` | Update packages |
| `moonwave_build` | Build documentation |
| `lune_run` | Run Luau scripts using Lune runtime |
| `lune_eval` | Evaluate inline Luau code using Lune |
| `luau_lsp_analyze` | Static type analysis using luau-lsp |

All toolchain tools have 30-second timeout protection via `execute_with_timeout()`.

#### LuneRunner Trait

**Location:** `src/tools/lune.rs`

Abstraction over Lune runtime execution for testability.

```rust
#[async_trait]
pub trait LuneRunner: Send + Sync {
    async fn run(
        &self,
        script_path: &Path,
        args: &[String],
        timeout: Option<Duration>,
    ) -> Result<LuneRunResult, RobloxMcpError>;

    async fn eval(
        &self,
        code: &str,
        timeout: Option<Duration>,
    ) -> Result<LuneRunResult, RobloxMcpError>;
}
```

**Response Types:**

```rust
pub struct LuneRunResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub duration_ms: u64,
}
```

| Implementation | Use Case |
|----------------|----------|
| `DefaultLuneRunner` | Production with real Lune binary |
| `MockLuneRunner` | Testing with queue-based responses |

#### LuauLspRunner Trait

**Location:** `src/tools/luau_lsp.rs`

Abstraction over luau-lsp static analysis for testability.

```rust
#[async_trait]
pub trait LuauLspRunner: Send + Sync {
    async fn analyze(
        &self,
        path: &Path,
        sourcemap_path: Option<&Path>,
        definitions: &[&Path],
    ) -> Result<AnalyzeResult, RobloxMcpError>;
}
```

**Response Types:**

```rust
pub struct AnalyzeResult {
    pub path: String,
    pub diagnostics: Vec<AnalyzeDiagnostic>,
    pub error_count: usize,
    pub warning_count: usize,
    pub files_analyzed: usize,
}

pub struct AnalyzeDiagnostic {
    pub severity: String,      // "Error", "Warning", "Information", "Hint"
    pub code: String,          // e.g., "TypeError", "UnknownGlobal"
    pub message: String,
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
}
```

| Implementation | Use Case |
|----------------|----------|
| `DefaultLuauLspRunner` | Production with real luau-lsp binary |
| `MockLuauLspRunner` | Testing with queue-based responses |

### AI Tools (4)

Requires `VOYAGE_API_KEY`, `NEO4J_URI`, and `NEO4J_PASSWORD` environment variables.

| Tool | Description |
|------|-------------|
| `ai_index_project` | Index Luau scripts for AI-powered search |
| `ai_search_codebase` | Semantic search using natural language |
| `ai_find_related` | Find scripts through code relationships |
| `ai_get_context` | Get relevant context within token budget |

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
