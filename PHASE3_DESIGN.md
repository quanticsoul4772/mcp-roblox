# Phase 3 Design Specification: Advanced Features

## Critical Fixes Applied (from code review)

| # | Issue | Severity | Location | Fix |
|---|-------|----------|----------|-----|
| 1 | FileWatcher tokio runtime context | 🔴 HIGH | Section 3.2 | Capture `Handle::current()` before creating watcher |
| 2 | HTTP error handling pattern | 🟡 MEDIUM | Section 3.1 | Use existing `from_reqwest()` helper, not `From<reqwest::Error>` |
| 3 | Metrics unbounded Vec growth | 🟡 MEDIUM | Section 3.3 | Use `VecDeque` with `MAX_DURATION_SAMPLES = 1000` |
| 4 | OpenCloudClient per-request creation | 🟢 LOW | Section 3.1 | Store in `RobloxMcpServer` struct, create once at startup |

---

## Current State Analysis

### Completed (Phases 1-2)

| Component | Status | Coverage |
|-----------|--------|----------|
| Rust MCP Server (rmcp 0.8.x) | ✅ Complete | 86% |
| 6 Filesystem Tools | ✅ Complete | Tested |
| 8 Studio Bridge Tools | ✅ Complete | Tested |
| HTTP Bridge (Axum) | ✅ Complete | 93% |
| Studio Plugin (Luau) | ✅ Complete | 266 lines |
| Fast-Failure Error Handling | ✅ Complete | All paths |

### Phase 3 Scope

```
┌─────────────────────────────────────────────────────────────────┐
│                     PHASE 3: ADVANCED FEATURES                   │
├─────────────────────────────────────────────────────────────────┤
│  3.1 Open Cloud Integration (CI/CD Automation)                  │
│  3.2 State Tracking & File Watching                             │
│  3.3 Monitoring & Observability                                 │
│  3.4 Performance Optimization                                   │
│  3.5 Plugin Enhancements                                        │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3.1 Open Cloud Integration

### Purpose
Enable Claude Code to automate publishing workflows: publish places, upload assets, and manage datastores without manual intervention.

### Architecture

```
┌──────────────────────┐     ┌──────────────────────┐
│   MCP Server         │     │  Roblox Open Cloud   │
│   (Rust)             │────▶│  API                 │
├──────────────────────┤     ├──────────────────────┤
│  OpenCloudClient     │     │  POST /places/:id    │
│  - api_key: String   │     │  POST /assets        │
│  - universe_id: u64  │     │  GET/POST /datastores│
└──────────────────────┘     └──────────────────────┘
```

### New Module: `src/cloud/mod.rs`

```rust
// src/cloud/mod.rs
mod client;
mod publish;
mod assets;
mod datastores;

pub use client::OpenCloudClient;
pub use publish::*;
pub use assets::*;
pub use datastores::*;
```

### Open Cloud Client Design

```rust
// src/cloud/client.rs
use reqwest::Client;
use std::time::Duration;
use crate::error::RobloxMcpError;

pub struct OpenCloudClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenCloudClient {
    /// Create a new Open Cloud client with connection pooling
    ///
    /// FIX: This client should be created ONCE at server startup and stored
    /// in RobloxMcpServer struct to benefit from HTTP connection pooling.
    /// Creating per-request loses pooling benefits.
    pub fn new() -> Result<Self, RobloxMcpError> {
        let api_key = std::env::var("ROBLOX_OPEN_CLOUD_API_KEY")
            .map_err(|_| RobloxMcpError::ConfigError(
                "ROBLOX_OPEN_CLOUD_API_KEY environment variable not set".into()
            ))?;

        // Configure client with connection pooling for performance
        let client = Client::builder()
            .pool_max_idle_per_host(5)
            .pool_idle_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(RobloxMcpError::from_reqwest)?;

        Ok(Self {
            client,
            api_key,
            base_url: "https://apis.roblox.com".into(),
        })
    }

    /// Publish a place file to Roblox
    pub async fn publish_place(
        &self,
        universe_id: u64,
        place_id: u64,
        rbxl_path: &Path,
    ) -> Result<PublishResult, RobloxMcpError> {
        // Read .rbxl file
        let content = tokio::fs::read(rbxl_path).await
            .map_err(|e| RobloxMcpError::FileSystemError {
                path: rbxl_path.display().to_string(),
                source: e,
            })?;

        // POST to Open Cloud
        let url = format!(
            "{}/universes/v1/{}/places/{}/versions",
            self.base_url, universe_id, place_id
        );

        let response = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/octet-stream")
            .query(&[("versionType", "Published")])
            .body(content)
            .send()
            .await
            .map_err(RobloxMcpError::HttpError)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RobloxMcpError::OpenCloudError {
                status: status.as_u16(),
                message: body,
            });
        }

        response.json().await
            .map_err(|e| RobloxMcpError::SerializationError(e))
    }
}
```

### New MCP Tools (3 tools)

| Tool | Purpose | Parameters |
|------|---------|------------|
| `cloud_publish_place` | Publish .rbxl to Roblox | universe_id, place_id, rbxl_path |
| `cloud_upload_asset` | Upload image/model/audio | asset_type, file_path, name, description |
| `cloud_datastore_get` | Read from DataStore | universe_id, datastore_name, key |

### Server Struct Update

```rust
// src/mcp/server.rs - Update RobloxMcpServer struct

#[derive(Clone)]
pub struct RobloxMcpServer {
    tool_router: ToolRouter<Self>,
    bridge: Arc<PluginBridge>,
    project_root: PathBuf,
    // FIX: Store OpenCloudClient in server for connection pooling
    cloud_client: Option<Arc<OpenCloudClient>>,  // Optional - only if API key set
}

impl RobloxMcpServer {
    pub fn new(bridge: Arc<PluginBridge>, project_root: PathBuf) -> Self {
        // Try to create cloud client at startup (may fail if no API key)
        let cloud_client = OpenCloudClient::new()
            .map(Arc::new)
            .ok();  // Convert to Option - tools will check availability

        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client,
        }
    }
}
```

### Tool Implementation

```rust
// src/mcp/server.rs additions

#[tool(
    name = "cloud_publish_place",
    description = "Publish a place file (.rbxl) to Roblox via Open Cloud API"
)]
async fn cloud_publish_place(
    &self,
    #[doc = "Universe ID from Roblox Creator Dashboard"]
    universe_id: u64,
    #[doc = "Place ID to publish to"]
    place_id: u64,
    #[doc = "Path to .rbxl file"]
    rbxl_path: String,
) -> Result<CallToolResult, ErrorData> {
    // FIX: Use stored client instead of creating new one per request
    let client = self.cloud_client.as_ref()
        .ok_or_else(|| ErrorData::internal_error(
            "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY not set".to_string(),
            None
        ))?;

    let path = PathBuf::from(&rbxl_path);
    let result = client.publish_place(universe_id, place_id, &path).await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
    )]))
}
```

### Error Types Addition

```rust
// src/error.rs additions

#[derive(Error, Debug)]
pub enum RobloxMcpError {
    // ... existing variants ...

    #[error("Open Cloud API error (HTTP {status}): {message}")]
    OpenCloudError {
        status: u16,
        message: String,
    },

    #[error("Configuration error: {0}")]
    ConfigError(String),

    // NOTE: Do NOT add `#[from] reqwest::Error` - use existing from_reqwest() helper
    // which provides granular error categorization (client/server/timeout/connection)
}
```

### HTTP Error Handling (Use Existing Pattern)

```rust
// CORRECT: Use existing from_reqwest() helper for proper error categorization
let response = self.client
    .post(&url)
    .send()
    .await
    .map_err(RobloxMcpError::from_reqwest)?;  // ✅ Preserves error granularity

// WRONG: Don't add blanket From<reqwest::Error> - loses error categorization
// .map_err(RobloxMcpError::HttpError)?  // ❌ Would bypass from_reqwest()
```

---

## 3.2 State Tracking & File Watching

### Purpose
Track filesystem changes in real-time using the `notify` crate. Enable tools like `fs_watch_changes` for Claude Code to subscribe to file modifications.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        FileWatcher                               │
├─────────────────────────────────────────────────────────────────┤
│  watcher: RecommendedWatcher                                     │
│  file_index: Arc<RwLock<HashMap<PathBuf, FileMetadata>>>        │
│  change_queue: Arc<RwLock<VecDeque<FileChange>>>                │
└─────────────────────────────────────────────────────────────────┘
         │
         │ notify events
         ▼
┌─────────────────────────────────────────────────────────────────┐
│  on_file_change(path)                                           │
│  - Update file_index with new mtime/hash                        │
│  - Push to change_queue for polling                             │
└─────────────────────────────────────────────────────────────────┘
```

### New Module: `src/watcher/mod.rs`

```rust
// src/watcher/mod.rs
use notify::{Watcher, RecursiveMode, RecommendedWatcher, Event};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub mtime: Instant,
    pub size_bytes: u64,
    pub hash: u64, // xxhash for fast comparison
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed { from: String },
}

pub struct FileWatcher {
    watcher: RecommendedWatcher,
    file_index: Arc<RwLock<HashMap<PathBuf, FileMetadata>>>,
    change_queue: Arc<RwLock<VecDeque<FileChange>>>,
    project_root: PathBuf,
}

impl FileWatcher {
    /// Create a new FileWatcher
    ///
    /// CRITICAL: Must be called from within a tokio runtime context.
    /// The notify callback runs on a background thread, so we capture
    /// the runtime Handle to spawn async tasks correctly.
    pub fn new(project_root: PathBuf) -> Result<Self, RobloxMcpError> {
        let file_index = Arc::new(RwLock::new(HashMap::new()));
        let change_queue = Arc::new(RwLock::new(VecDeque::new()));

        let index_clone = file_index.clone();
        let queue_clone = change_queue.clone();
        let root_clone = project_root.clone();

        // CRITICAL FIX: Capture runtime handle BEFORE creating watcher
        // notify callbacks run on a background thread (not tokio runtime),
        // so tokio::spawn() would panic without explicit handle
        let runtime_handle = tokio::runtime::Handle::current();

        let watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                let index = index_clone.clone();
                let queue = queue_clone.clone();
                let root = root_clone.clone();

                // Use captured handle to spawn on tokio runtime
                runtime_handle.spawn(async move {
                    Self::handle_event(event, index, queue, root).await;
                });
            }
        }).map_err(|e| RobloxMcpError::WatcherError(e.to_string()))?;

        Ok(Self {
            watcher,
            file_index,
            change_queue,
            project_root,
        })
    }

    pub fn start_watching(&mut self) -> Result<(), RobloxMcpError> {
        self.watcher.watch(&self.project_root, RecursiveMode::Recursive)
            .map_err(|e| RobloxMcpError::WatcherError(e.to_string()))
    }

    pub async fn poll_changes(&self, limit: usize) -> Vec<FileChange> {
        let mut queue = self.change_queue.write().await;
        let mut changes = Vec::with_capacity(limit.min(queue.len()));

        for _ in 0..limit {
            if let Some(change) = queue.pop_front() {
                changes.push(change);
            } else {
                break;
            }
        }

        changes
    }

    async fn handle_event(
        event: Event,
        index: Arc<RwLock<HashMap<PathBuf, FileMetadata>>>,
        queue: Arc<RwLock<VecDeque<FileChange>>>,
        project_root: PathBuf,
    ) {
        use notify::EventKind;

        for path in event.paths {
            // Only track .luau files
            if path.extension() != Some(std::ffi::OsStr::new("luau")) {
                continue;
            }

            let relative_path = path.strip_prefix(&project_root)
                .unwrap_or(&path)
                .display()
                .to_string();

            let kind = match event.kind {
                EventKind::Create(_) => ChangeKind::Created,
                EventKind::Modify(_) => ChangeKind::Modified,
                EventKind::Remove(_) => ChangeKind::Deleted,
                _ => continue,
            };

            // Update index
            if matches!(kind, ChangeKind::Created | ChangeKind::Modified) {
                if let Ok(metadata) = tokio::fs::metadata(&path).await {
                    index.write().await.insert(path.clone(), FileMetadata {
                        mtime: Instant::now(),
                        size_bytes: metadata.len(),
                        hash: 0, // TODO: compute xxhash
                    });
                }
            } else if matches!(kind, ChangeKind::Deleted) {
                index.write().await.remove(&path);
            }

            // Queue change notification
            queue.write().await.push_back(FileChange {
                path: relative_path,
                kind,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            });
        }
    }
}
```

### New MCP Tool

```rust
#[tool(
    name = "fs_watch_changes",
    description = "Poll for recent file changes (returns up to 100 changes since last poll)"
)]
async fn fs_watch_changes(
    &self,
    #[doc = "Maximum number of changes to return (default: 100)"]
    limit: Option<usize>,
) -> Result<CallToolResult, ErrorData> {
    let limit = limit.unwrap_or(100).min(100);

    let changes = self.file_watcher.poll_changes(limit).await;

    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&json!({
            "changes": changes,
            "count": changes.len(),
        }))
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
    )]))
}
```

---

## 3.3 Monitoring & Observability

### Purpose
Provide metrics and health endpoints for monitoring MCP server performance and diagnosing issues.

### Metrics to Track

| Metric | Type | Description |
|--------|------|-------------|
| `mcp_tool_calls_total` | Counter | Total tool invocations by tool name |
| `mcp_tool_duration_seconds` | Histogram | Tool execution latency |
| `mcp_tool_errors_total` | Counter | Failed tool calls by error type |
| `plugin_heartbeat_age_seconds` | Gauge | Time since last plugin heartbeat |
| `file_watcher_events_total` | Counter | File change events processed |

### New Module: `src/metrics/mod.rs`

```rust
// src/metrics/mod.rs
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Maximum duration samples to keep per tool (prevents unbounded memory growth)
const MAX_DURATION_SAMPLES: usize = 1000;

#[derive(Default)]
pub struct Metrics {
    tool_calls: Arc<RwLock<HashMap<String, AtomicU64>>>,
    tool_errors: Arc<RwLock<HashMap<String, AtomicU64>>>,
    // FIX: Use VecDeque with bounded size instead of unbounded Vec
    tool_durations: Arc<RwLock<HashMap<String, VecDeque<f64>>>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn record_tool_call(&self, tool_name: &str, duration_secs: f64, success: bool) {
        // Increment call counter
        {
            let mut calls = self.tool_calls.write().await;
            calls.entry(tool_name.to_string())
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }

        // Record duration with BOUNDED storage (circular buffer behavior)
        {
            let mut durations = self.tool_durations.write().await;
            let samples = durations.entry(tool_name.to_string())
                .or_insert_with(|| VecDeque::with_capacity(MAX_DURATION_SAMPLES));

            // Evict oldest sample if at capacity
            if samples.len() >= MAX_DURATION_SAMPLES {
                samples.pop_front();
            }
            samples.push_back(duration_secs);
        }

        // Increment error counter if failed
        if !success {
            let mut errors = self.tool_errors.write().await;
            errors.entry(tool_name.to_string())
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    pub async fn get_summary(&self) -> MetricsSummary {
        let calls = self.tool_calls.read().await;
        let errors = self.tool_errors.read().await;
        let durations = self.tool_durations.read().await;

        let tools: Vec<ToolMetrics> = calls.iter().map(|(name, count)| {
            let error_count = errors.get(name)
                .map(|e| e.load(Ordering::Relaxed))
                .unwrap_or(0);

            let tool_durations: Vec<f64> = durations.get(name)
                .map(|d| d.iter().copied().collect())
                .unwrap_or_default();

            let avg_duration = if tool_durations.is_empty() {
                0.0
            } else {
                tool_durations.iter().sum::<f64>() / tool_durations.len() as f64
            };

            ToolMetrics {
                name: name.clone(),
                total_calls: count.load(Ordering::Relaxed),
                error_count,
                avg_duration_ms: avg_duration * 1000.0,
                sample_count: tool_durations.len(),  // Show how many samples in average
            }
        }).collect();

        MetricsSummary { tools }
    }
}

#[derive(Debug, Serialize)]
pub struct MetricsSummary {
    pub tools: Vec<ToolMetrics>,
}

#[derive(Debug, Serialize)]
pub struct ToolMetrics {
    pub name: String,
    pub total_calls: u64,
    pub error_count: u64,
    pub avg_duration_ms: f64,
    pub sample_count: usize,  // Number of samples in rolling average
}
```

### Health Endpoint

Add to HTTP bridge:

```rust
// src/bridge/http.rs additions

async fn health_handler(
    State(bridge): State<PluginBridge>,
) -> Json<HealthStatus> {
    let heartbeat_age = bridge.last_heartbeat.read().await.elapsed();

    Json(HealthStatus {
        status: if heartbeat_age < Duration::from_secs(10) { "healthy" } else { "degraded" },
        plugin_connected: heartbeat_age < Duration::from_secs(10),
        heartbeat_age_secs: heartbeat_age.as_secs_f64(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[derive(Serialize)]
struct HealthStatus {
    status: &'static str,
    plugin_connected: bool,
    heartbeat_age_secs: f64,
    version: String,
}

// Update router
pub fn create_router(bridge: PluginBridge) -> Router {
    Router::new()
        .route("/poll", get(poll_handler))
        .route("/result", post(result_handler))
        .route("/health", get(health_handler))  // NEW
        .with_state(bridge)
}
```

---

## 3.4 Performance Optimization

### Current Bottlenecks

1. **File tree building**: Synchronous walkdir blocks async runtime
2. **Large DataModel queries**: Can exceed context window
3. **No response compression**: Large JSON payloads

### Optimizations

#### 1. Parallel File Tree Building

```rust
// src/tools/filesystem.rs optimization

pub async fn build_tree_parallel(
    root: &Path,
    max_depth: usize,
) -> Result<TreeBuildResult, RobloxMcpError> {
    use tokio::task;

    // Use spawn_blocking for CPU-bound walkdir
    let root = root.to_path_buf();
    task::spawn_blocking(move || {
        build_tree_sync(&root, max_depth)
    })
    .await
    .map_err(|e| RobloxMcpError::FileSystemError {
        path: root.display().to_string(),
        source: std::io::Error::new(std::io::ErrorKind::Other, e),
    })?
}
```

#### 2. Response Pagination

```rust
#[tool(
    name = "studio_get_datamodel_paginated",
    description = "Get DataModel with pagination to avoid context window overflow"
)]
async fn studio_get_datamodel_paginated(
    &self,
    #[doc = "Maximum depth (default: 3)"]
    max_depth: Option<usize>,
    #[doc = "Start path for pagination (default: game)"]
    start_path: Option<String>,
    #[doc = "Maximum instances to return (default: 500)"]
    limit: Option<usize>,
) -> Result<CallToolResult, ErrorData> {
    let max_depth = max_depth.unwrap_or(3);
    let limit = limit.unwrap_or(500).min(1000);

    // ... implementation with early cutoff at limit
}
```

#### 3. Connection Pooling for Open Cloud

**Already addressed in Section 3.1**: The `OpenCloudClient` is created once at server startup
and stored in `RobloxMcpServer.cloud_client`. Client is configured with:
- `pool_max_idle_per_host(5)` - Keep 5 idle connections per host
- `pool_idle_timeout(30s)` - Close idle connections after 30 seconds
- `timeout(60s)` - Request timeout for large file uploads

See "Server Struct Update" in Section 3.1 for implementation.

---

## 3.5 Plugin Enhancements

### Current Limitations

1. No connection status indicator beyond button state
2. No reconnection logic on server restart
3. Limited error feedback to user

### Enhancements

```lua
-- plugin/init.lua additions

local RECONNECT_DELAY = 5
local MAX_RECONNECT_ATTEMPTS = 10
local reconnectAttempts = 0

-- Status indicator widget
local widgetInfo = DockWidgetPluginGuiInfo.new(
    Enum.InitialDockState.Float,
    false, false, 200, 100, 150, 80
)
local statusWidget = plugin:CreateDockWidgetPluginGui("MCPStatus", widgetInfo)
statusWidget.Title = "MCP Status"

local statusLabel = Instance.new("TextLabel")
statusLabel.Size = UDim2.new(1, 0, 1, 0)
statusLabel.Text = "Disconnected"
statusLabel.TextColor3 = Color3.fromRGB(255, 100, 100)
statusLabel.Parent = statusWidget

local function updateStatus(text, color)
    statusLabel.Text = text
    statusLabel.TextColor3 = color
end

local function attemptReconnect()
    while reconnectAttempts < MAX_RECONNECT_ATTEMPTS and not connected do
        reconnectAttempts = reconnectAttempts + 1
        updateStatus("Reconnecting... (" .. reconnectAttempts .. "/" .. MAX_RECONNECT_ATTEMPTS .. ")",
            Color3.fromRGB(255, 255, 100))

        local success = pcall(function()
            HttpService:RequestAsync({
                Url = SERVER_URL .. "/health",
                Method = "GET"
            })
        end)

        if success then
            connected = true
            reconnectAttempts = 0
            updateStatus("Connected", Color3.fromRGB(100, 255, 100))
            task.spawn(pollLoop)
            return
        end

        task.wait(RECONNECT_DELAY)
    end

    updateStatus("Failed to reconnect", Color3.fromRGB(255, 100, 100))
end
```

---

## Implementation Priority

| Task | Priority | Effort | Dependencies |
|------|----------|--------|--------------|
| Open Cloud Client | HIGH | 2 days | reqwest (already in Cargo.toml) |
| `cloud_publish_place` tool | HIGH | 1 day | Open Cloud Client |
| File Watcher module | MEDIUM | 2 days | notify (already in Cargo.toml) |
| `fs_watch_changes` tool | MEDIUM | 1 day | File Watcher |
| Health endpoint | MEDIUM | 0.5 days | None |
| Metrics module | LOW | 1 day | None |
| Plugin reconnection | LOW | 1 day | Health endpoint |
| Response pagination | LOW | 1 day | None |

### Recommended Order

```
Week 1:
├── Day 1-2: Open Cloud Client + publish_place tool
├── Day 3-4: File Watcher module + fs_watch_changes tool
└── Day 5: Health endpoint + testing

Week 2:
├── Day 1: Metrics module
├── Day 2: Plugin reconnection logic
├── Day 3: Response pagination
└── Day 4-5: Integration testing + documentation
```

---

## File Structure After Phase 3

```
src/
├── bridge/
│   └── http.rs              # + health endpoint
├── cloud/                    # NEW
│   ├── mod.rs
│   ├── client.rs            # OpenCloudClient
│   ├── publish.rs           # publish_place
│   ├── assets.rs            # upload_asset
│   └── datastores.rs        # datastore operations
├── error.rs                 # + new error variants
├── main.rs
├── mcp/
│   ├── mod.rs
│   ├── params.rs            # + cloud params
│   └── server.rs            # + cloud tools
├── metrics/                  # NEW
│   └── mod.rs
├── tools/
│   └── filesystem.rs
└── watcher/                  # NEW
    └── mod.rs

plugin/
└── init.lua                 # + reconnection, status widget
```

---

## Testing Strategy

### Unit Tests
- Open Cloud client mocking with `mockito`
- File watcher event simulation
- Metrics collection validation

### Integration Tests
- Full publish workflow (requires test universe)
- File change detection end-to-end
- Health endpoint responses

### Manual Testing
- Plugin reconnection after server restart
- DataModel pagination with large hierarchies
- Open Cloud error handling (invalid API key, rate limits)

---

## Security Considerations

| Risk | Mitigation |
|------|------------|
| API key in environment | Document secure handling, never log |
| Accidental publish | Require explicit confirmation in tool |
| DataStore writes | Implement dry-run mode |
| Rate limiting | Respect Roblox rate limits, implement backoff |

---

## Success Criteria

- [ ] `cloud_publish_place` successfully publishes a test place
- [ ] `fs_watch_changes` detects file modifications within 1 second
- [ ] Health endpoint returns accurate plugin connection status
- [ ] Metrics accurately track tool usage
- [ ] Plugin reconnects automatically after server restart
- [ ] All new code has >80% test coverage
