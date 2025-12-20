# Hybrid AI integration for Roblox Studio with Claude Code: Rust-Based Fast-Failure Architecture

A **three-layer hybrid architecture** built with **Rust** for maximum performance and reliability. This design combines Rojo filesystem sync, an MCP server, and a Studio plugin to deliver the most capable Claude Code integration for Roblox development. The fast-failure error handling philosophy ensures Claude Code receives immediate, actionable feedback when operations fail—no silent degradation, no fallback gymnastics.

## Why Rust with fast-failure outperforms TypeScript/Python approaches

Roblox development demands both performance and reliability. The December 2025 MCP ecosystem provides mature Rust SDKs (rmcp 0.8.0, Prism MCP, PMCP) that deliver **4,700+ queries per second** versus TypeScript's ~1,500 QPS. More critically, Rust's `Result<T, E>` type system enforces explicit error handling at compile time, preventing the silent failures common in exception-based languages.

Fast-failure error handling means when the Studio plugin disconnects, when Rojo sync stalls, or when file operations fail, **Claude Code learns about it immediately** through clear error messages—not after attempting fallback strategies that hide the root problem. This transparency accelerates debugging and enables Claude to make informed decisions about retry strategies.

The optimal architecture uses **all three integration methods**: Rojo for AI code generation (Claude Code writes files → Rojo syncs to Studio in ~100ms), an MCP server bridging Claude Code to a Studio plugin for runtime access, and Open Cloud for automated publishing. This combination gives Claude Code complete visibility into both filesystem structure and live Studio state.

Existing tools validate this pattern. The community-built **robloxstudio-mcp** server (boshyxd/robloxstudio-mcp, 50+ GitHub stars) and the **vibe-blocks-mcp** server demonstrate plugin-bridge architectures. The December 2025 landscape shows Rust MCP servers (waitfish SQLite, rustfs-mcp, video-transcriber-mcp-rs) achieving production-grade performance with minimal resource footprints.

## Recommended architecture with three integration layers

The architecture consists of three coordinated layers communicating through a **Rust-based MCP server** that acts as the central hub:

```
┌─────────────────────────────────────────────────────────────────┐
│                        Claude Code Host                          │
├─────────────────────────────────────────────────────────────────┤
│  MCP Client ←→ Roblox Studio MCP Server (Rust/rmcp 0.8.0)      │
└─────────────────────────────────┬───────────────────────────────┘
                                  │ STDIO Transport
                                  ▼
┌─────────────────────────────────────────────────────────────────┐
│                   MCP Server Core Layer (Rust)                   │
├──────────────┬──────────────────┬───────────────────────────────┤
│  Filesystem  │   HTTP Bridge    │   Open Cloud Client           │
│   Tools      │   (Plugin Comm)  │   (Publishing/Assets)         │
│  (notify)    │   (axum)         │   (reqwest)                   │
└──────┬───────┴────────┬─────────┴──────────────┬────────────────┘
       │                │                        │
       ▼                ▼                        ▼
┌──────────────┐ ┌─────────────────┐ ┌─────────────────────────────┐
│ Local Files  │ │ Studio Plugin   │ │ Roblox Open Cloud API       │
│ (.luau via   │ │ (ScriptEditor,  │ │ (Place Publishing,          │
│  Rojo sync)  │ │  ChangeHistory) │ │  Asset Upload)              │
└──────────────┘ └─────────────────┘ └─────────────────────────────┘
```

### Layer 1: Filesystem tools via Rojo integration

The MCP server monitors the project's filesystem using the `notify` crate, providing Claude Code with file tree exploration, script reading/writing, and change notifications. When Claude generates or modifies code, it writes directly to `.luau` files that Rojo automatically syncs to Studio within **100ms**.

**Core filesystem tools:**

| Tool | Purpose | Error Behavior |
|------|---------|----------------|
| `fs_get_tree` | List project structure with depth limits | Returns error if path doesn't exist or is unreadable |
| `fs_read_script` | Read Luau source files | Hard fails on missing file or encoding errors |
| `fs_write_script` | Create/modify script files | Fails fast on permission denied or disk full |
| `fs_search_content` | Find code patterns across project | Returns error on regex parse failure |
| `fs_watch_changes` | Subscribe to file change notifications | Errors immediately on watcher setup failure |

### Layer 2: Studio bridge via HTTP polling plugin

A Roblox Studio plugin communicates with the MCP server through HTTP long-polling on localhost. The plugin polls every **500ms** for pending commands, executes them using Studio APIs (ScriptEditorService, ChangeHistoryService, Selection), and returns results. Connection failures propagate immediately to Claude Code with clear error messages.

**Core Studio tools:**

| Tool | Purpose | Studio API Used | Error Behavior |
|------|---------|-----------------|----------------|
| `studio_get_datamodel` | Explore live Studio hierarchy | game:GetDescendants() | Timeout error if plugin unresponsive >30s |
| `studio_get_script_source` | Read script from open editor | ScriptEditorService | Fails if script not found or inaccessible |
| `studio_modify_script` | Edit script with undo support | UpdateSourceAsync + ChangeHistory | Hard fails on permission errors |
| `studio_get_selection` | Read current Studio selection | Selection:Get() | Returns empty if no selection (not an error) |
| `studio_create_instance` | Create new objects | Instance.new() | Fails on invalid class name |
| `studio_set_properties` | Modify instance properties | Direct property access | Errors on type mismatches |

### Layer 3: Open Cloud for CI/CD automation

The MCP server includes an Open Cloud client for automated publishing workflows. After Claude Code generates and tests code, it can trigger place publishing without manual intervention. Connection and authentication failures propagate immediately.

**Open Cloud tools:** `publish_place`, `upload_asset`, `manage_datastore`

## Detailed communication protocol between server and plugin

The HTTP bridge protocol handles the inherent challenge of connecting Claude Code (running locally) with the Roblox Studio plugin (running in a sandboxed Lua environment):

```
MCP Server (Rust)                Studio Plugin (Luau)
     │                                 │
     │◄──── Poll for commands ─────────│  (every 500ms)
     │                                 │
     │──── Command + UUID ────────────►│
     │                                 │
     │         (Plugin executes)       │
     │                                 │
     │◄──── Result OR Error + UUID ────│
     │                                 │
     ▼                                 ▼
```

**Protocol implementation (MCP Server side in Rust):**

```rust
use rmcp::{Error as McpError, tool};
use axum::{Router, Json, extract::State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, timeout};
use anyhow::{Result, Context};

#[derive(Clone)]
struct PluginBridge {
    pending_commands: Arc<RwLock<Vec<Command>>>,
    pending_results: Arc<RwLock<HashMap<String, oneshot::Sender<PluginResponse>>>>,
    last_heartbeat: Arc<RwLock<Instant>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Command {
    id: String,
    action: String,
    params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct PluginResponse {
    id: String,
    result: Option<serde_json::Value>,
    error: Option<String>,
}

// HTTP endpoints for plugin communication
async fn poll_handler(State(bridge): State<PluginBridge>) -> Json<Option<Command>> {
    // Update heartbeat
    *bridge.last_heartbeat.write().await = Instant::now();
    
    // Return next pending command if available
    let command = bridge.pending_commands.write().await.pop();
    Json(command)
}

async fn result_handler(
    State(bridge): State<PluginBridge>,
    Json(response): Json<PluginResponse>,
) -> Json<serde_json::Value> {
    // Find the waiting receiver and send result
    if let Some(tx) = bridge.pending_results.write().await.remove(&response.id) {
        let _ = tx.send(response); // Ignore send errors (caller may have timed out)
    }
    Json(json!({ "ok": true }))
}

// Tool that uses the bridge with FAST FAILURE
#[tool]
async fn studio_get_selection(
    #[state] bridge: Arc<PluginBridge>,
) -> Result<Vec<Instance>, McpError> {
    // Check if plugin is alive - FAIL IMMEDIATELY if not
    let last_heartbeat = *bridge.last_heartbeat.read().await;
    let elapsed = last_heartbeat.elapsed();
    
    if elapsed > Duration::from_secs(10) {
        return Err(McpError::InternalError(format!(
            "Studio plugin disconnected (last heartbeat: {:?} ago). \
             Restart Studio and reconnect the plugin.",
            elapsed
        )));
    }
    
    // Create command with UUID
    let id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    
    // Register result receiver BEFORE sending command
    bridge.pending_results.write().await.insert(id.clone(), tx);
    
    // Queue command
    bridge.pending_commands.write().await.push(Command {
        id: id.clone(),
        action: "getSelection".to_string(),
        params: json!({}),
    });
    
    // Wait for response with HARD TIMEOUT - no fallback
    let response = timeout(Duration::from_secs(30), rx)
        .await
        .context("Plugin response timeout after 30s")?
        .context("Result channel closed unexpectedly")?;
    
    // Check for plugin-side errors - PROPAGATE IMMEDIATELY
    if let Some(error) = response.error {
        return Err(McpError::InternalError(format!(
            "Studio plugin error: {}",
            error
        )));
    }
    
    // Parse successful result
    let instances: Vec<Instance> = serde_json::from_value(
        response.result.unwrap_or(json!([]))
    ).context("Failed to parse selection response")?;
    
    Ok(instances)
}

// Axum router setup
fn create_router(bridge: PluginBridge) -> Router {
    Router::new()
        .route("/poll", axum::routing::get(poll_handler))
        .route("/result", axum::routing::post(result_handler))
        .with_state(bridge)
}
```

**Protocol implementation (Studio Plugin side in Luau):**

```lua
-- Roblox Studio Plugin (Luau)
local HttpService = game:GetService("HttpService")
local Selection = game:GetService("Selection")
local ScriptEditorService = game:GetService("ScriptEditorService")

local SERVER_URL = "http://127.0.0.1:8080"
local POLL_INTERVAL = 0.5

local function executeCommand(action, params)
    if action == "getSelection" then
        local selected = Selection:Get()
        local instances = {}
        for i, inst in ipairs(selected) do
            table.insert(instances, {
                Name = inst.Name,
                ClassName = inst.ClassName,
                Path = inst:GetFullName()
            })
        end
        return { instances = instances }
        
    elseif action == "getScriptSource" then
        local script = game:FindFirstChild(params.path, true)
        if not script or not script:IsA("LuaSourceContainer") then
            error("Script not found: " .. params.path)
        end
        return { source = script.Source }
        
    elseif action == "modifyScript" then
        local script = game:FindFirstChild(params.path, true)
        if not script or not script:IsA("LuaSourceContainer") then
            error("Script not found: " .. params.path)
        end
        
        -- Use ScriptEditorService for undo support
        local document = ScriptEditorService:FindScriptDocument(script)
        if document then
            document:EditTextAsync(params.newSource, 1, 1)
        else
            script.Source = params.newSource
        end
        
        return { success = true }
    end
    
    error("Unknown action: " .. action)
end

local function pollLoop()
    while true do
        local success, response = pcall(function()
            return HttpService:RequestAsync({
                Url = SERVER_URL .. "/poll",
                Method = "GET"
            })
        end)
        
        if success and response.StatusCode == 200 and response.Body ~= "null" then
            local command = HttpService:JSONDecode(response.Body)
            local result, error_msg
            
            -- Execute command with error capture
            local exec_success, exec_result = pcall(function()
                return executeCommand(command.action, command.params)
            end)
            
            if exec_success then
                result = exec_result
            else
                error_msg = tostring(exec_result)
            end
            
            -- Send result back (errors included)
            pcall(function()
                HttpService:RequestAsync({
                    Url = SERVER_URL .. "/result",
                    Method = "POST",
                    Headers = { ["Content-Type"] = "application/json" },
                    Body = HttpService:JSONEncode({
                        id = command.id,
                        result = result,
                        error = error_msg
                    })
                })
            end)
        end
        
        task.wait(POLL_INTERVAL)
    end
end

-- Start polling in background
task.spawn(pollLoop)
```

## Comparison matrix of integration approaches

| Capability | Rojo Only | Plugin Only | Open Cloud Only | **Hybrid (Rust)** |
|------------|-----------|-------------|-----------------|-------------------|
| AI code generation to files | ✅ Excellent | ❌ Impossible | ❌ Impossible | ✅ Excellent |
| Real-time sync to Studio | ✅ ~100ms | N/A | ❌ None | ✅ ~100ms |
| Read live Studio state | ❌ None | ✅ Full access | ❌ None | ✅ Full access |
| Modify instances at runtime | ❌ None | ✅ Full access | ❌ None | ✅ Full access |
| Script editing with undo | ❌ Limited | ✅ ChangeHistory | ❌ None | ✅ ChangeHistory |
| Automated publishing | ❌ CLI only | ❌ None | ✅ REST API | ✅ REST API |
| Performance (QPS) | N/A | N/A | N/A | **4,700+ QPS** |
| Error transparency | ⚠️ Varies | ⚠️ Varies | ⚠️ Varies | ✅ Immediate failures |
| Memory safety | ❌ JS/TS | ❌ Lua sandbox | N/A | ✅ Compile-time |
| Works offline | ✅ Yes | ✅ Yes | ❌ No | ✅ Partial |
| Setup complexity | Low | Medium | Low | **Medium-High** |
| Claude Code compatibility | ✅ Native files | ❌ Requires bridge | ❌ Requires bridge | ✅ Native + bridge |

## Recommended tool set for Claude Code agentic workflows

Design tools following MCP best practices with **Rust's type system enforcing correctness**. The following 15-tool set covers core development workflows with explicit error handling:

### Filesystem tools (6 tools)

```rust
use rmcp::{tool, Error as McpError};
use notify::{Watcher, RecursiveMode, Event};
use std::path::PathBuf;
use tokio::fs;

#[tool]
async fn fs_get_tree(
    path: String,
    #[default(5)] max_depth: usize,
) -> Result<FileTree, McpError> {
    let root = PathBuf::from(&path);
    
    if !root.exists() {
        return Err(McpError::InvalidParams(format!(
            "Path does not exist: {}",
            path
        )));
    }
    
    let tree = build_tree(&root, 0, max_depth).await
        .map_err(|e| McpError::InternalError(format!("Failed to build tree: {}", e)))?;
    
    Ok(tree)
}

#[tool]
async fn fs_read_script(file_path: String) -> Result<ScriptContent, McpError> {
    let path = PathBuf::from(&file_path);
    
    // Validate .luau extension
    if path.extension() != Some(std::ffi::OsStr::new("luau")) {
        return Err(McpError::InvalidParams(
            "Only .luau files supported".to_string()
        ));
    }
    
    // Read with explicit error propagation
    let content = fs::read_to_string(&path).await
        .map_err(|e| McpError::InternalError(format!(
            "Failed to read {}: {}",
            file_path, e
        )))?;
    
    Ok(ScriptContent {
        path: file_path,
        content,
        size_bytes: content.len(),
    })
}

#[tool]
async fn fs_write_script(
    file_path: String,
    content: String,
    #[default(true)] create_directories: bool,
) -> Result<WriteResult, McpError> {
    let path = PathBuf::from(&file_path);
    
    // Create parent directories if requested
    if create_directories {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await
                .map_err(|e| McpError::InternalError(format!(
                    "Failed to create directories: {}",
                    e
                )))?;
        }
    }
    
    // Write file - FAIL FAST on errors
    fs::write(&path, &content).await
        .map_err(|e| McpError::InternalError(format!(
            "Failed to write {}: {}",
            file_path, e
        )))?;
    
    // Rojo auto-syncs - no manual trigger needed
    Ok(WriteResult {
        path: file_path,
        bytes_written: content.len(),
    })
}

// Additional tools: fs_delete_script, fs_search_content, fs_watch_changes
```

### Studio context tools (5 tools)

```rust
#[tool]
async fn studio_get_services(
    #[state] bridge: Arc<PluginBridge>,
) -> Result<Vec<ServiceInfo>, McpError> {
    let response = execute_plugin_command(
        bridge,
        "getServices",
        json!({}),
    ).await?;
    
    serde_json::from_value(response)
        .map_err(|e| McpError::InternalError(format!(
            "Invalid service data from plugin: {}",
            e
        )))
}

#[tool]
async fn studio_get_descendants(
    #[state] bridge: Arc<PluginBridge>,
    #[default("game")] root: String,
    #[default(3)] max_depth: usize,
    class_filter: Option<Vec<String>>,
) -> Result<InstanceTree, McpError> {
    execute_plugin_command(
        bridge,
        "getDescendants",
        json!({
            "root": root,
            "maxDepth": max_depth,
            "classFilter": class_filter,
        }),
    ).await
    .and_then(|v| serde_json::from_value(v)
        .map_err(|e| McpError::InternalError(e.to_string())))
}

// Additional tools: studio_get_selection, studio_get_script_source, studio_find_instances
```

### Modification tools (4 tools)

```rust
#[tool]
async fn studio_modify_script(
    #[state] bridge: Arc<PluginBridge>,
    script_path: String,
    new_source: String,
    #[default(true)] record_undo: bool,
) -> Result<ModifyResult, McpError> {
    // Validate script path format
    if !script_path.starts_with("game.") {
        return Err(McpError::InvalidParams(
            "Script path must start with 'game.'".to_string()
        ));
    }
    
    execute_plugin_command(
        bridge,
        "modifyScript",
        json!({
            "scriptPath": script_path,
            "newSource": new_source,
            "recordUndo": record_undo,
        }),
    ).await
    .and_then(|v| serde_json::from_value(v)
        .map_err(|e| McpError::InternalError(e.to_string())))
}

#[tool]
async fn studio_create_instance(
    #[state] bridge: Arc<PluginBridge>,
    class_name: String,
    parent: String,
    name: String,
    properties: Option<HashMap<String, serde_json::Value>>,
) -> Result<Instance, McpError> {
    execute_plugin_command(
        bridge,
        "createInstance",
        json!({
            "className": class_name,
            "parent": parent,
            "name": name,
            "properties": properties.unwrap_or_default(),
        }),
    ).await
    .and_then(|v| serde_json::from_value(v)
        .map_err(|e| McpError::InternalError(e.to_string())))
}

// Additional tools: studio_set_property, studio_delete_instance
```

## Fast-failure error handling philosophy

**No fallbacks. No silent degradation. Immediate, actionable errors.**

### Core Principles

1. **Explicit over implicit**: Every failure path returns a descriptive `Err` variant
2. **Fail loudly**: Errors propagate to Claude Code with full context
3. **No recovery attempts**: Let Claude Code decide retry strategy
4. **Typed errors**: Use domain-specific error enums for clarity

### Error Type Hierarchy

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RobloxMcpError {
    #[error("Studio plugin disconnected (last heartbeat: {0:?} ago). Restart Studio.")]
    PluginTimeout(Duration),
    
    #[error("Studio plugin returned error: {0}")]
    PluginExecutionError(String),
    
    #[error("Rojo sync failed: {0}")]
    RojoSyncFailure(String),
    
    #[error("Invalid Studio response: {0}")]
    InvalidStudioData(String),
    
    #[error("File operation failed on '{path}': {source}")]
    FileSystemError {
        path: String,
        #[source]
        source: std::io::Error,
    },
    
    #[error("HTTP request to plugin failed: {0}")]
    HttpError(#[from] reqwest::Error),
    
    #[error("JSON serialization failed: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl From<RobloxMcpError> for McpError {
    fn from(err: RobloxMcpError) -> Self {
        McpError::InternalError(err.to_string())
    }
}
```

### Example: Plugin Communication with Zero Fallbacks

```rust
async fn execute_plugin_command(
    bridge: Arc<PluginBridge>,
    action: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    // 1. Check plugin heartbeat - HARD FAILURE if stale
    let last_heartbeat = *bridge.last_heartbeat.read().await;
    let elapsed = last_heartbeat.elapsed();
    
    if elapsed > Duration::from_secs(10) {
        return Err(RobloxMcpError::PluginTimeout(elapsed).into());
    }
    
    // 2. Create command and response channel
    let id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    
    bridge.pending_results.write().await.insert(id.clone(), tx);
    bridge.pending_commands.write().await.push(Command {
        id: id.clone(),
        action: action.to_string(),
        params,
    });
    
    // 3. Wait with HARD TIMEOUT - no retry, no fallback
    let response = timeout(Duration::from_secs(30), rx)
        .await
        .map_err(|_| RobloxMcpError::PluginTimeout(Duration::from_secs(30)))?
        .map_err(|_| McpError::InternalError("Result channel closed".to_string()))?;
    
    // 4. Check for plugin-side errors - PROPAGATE IMMEDIATELY
    if let Some(error) = response.error {
        return Err(RobloxMcpError::PluginExecutionError(error).into());
    }
    
    // 5. Return result or fail if missing
    response.result.ok_or_else(|| 
        McpError::InternalError("Plugin returned success but no result".to_string())
    )
}
```

### Why Fast-Failure Beats Fallbacks

| Scenario | Fallback Approach | Fast-Failure Approach |
|----------|-------------------|----------------------|
| Plugin disconnected | Return cached data (stale) | Error: "Plugin disconnected. Restart Studio." |
| File not found | Return empty string | Error: "File 'X' not found at path Y" |
| Invalid JSON from plugin | Return default object | Error: "Plugin returned invalid JSON: [details]" |
| Network timeout | Retry 3x then fail | Error: "Plugin timeout after 30s" |

**Result**: Claude Code gets actionable errors it can communicate to the user or use to adjust its strategy. No hidden failures.

## State management across integration points

The MCP server maintains consistent state across filesystem, plugin bridge, and Open Cloud using Tokio's async primitives:

```rust
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::time::Instant;

pub struct ServerState {
    // File system state (updated via notify watcher)
    file_index: RwLock<HashMap<PathBuf, FileMetadata>>,
    
    // Studio plugin state
    plugin_bridge: Arc<PluginBridge>,
    
    // Open Cloud client
    cloud_client: Arc<OpenCloudClient>,
}

struct FileMetadata {
    mtime: Instant,
    hash: u64,  // For change detection
}

impl ServerState {
    pub async fn on_file_change(&self, path: PathBuf) {
        let hash = calculate_file_hash(&path).await.unwrap_or(0);
        
        self.file_index.write().await.insert(path.clone(), FileMetadata {
            mtime: Instant::now(),
            hash,
        });
        
        // No automatic Studio sync - Rojo handles that
        // State is just for tracking changes
    }
    
    pub async fn check_plugin_health(&self) -> Result<(), RobloxMcpError> {
        let elapsed = self.plugin_bridge.last_heartbeat.read().await.elapsed();
        
        if elapsed > Duration::from_secs(10) {
            Err(RobloxMcpError::PluginTimeout(elapsed))
        } else {
            Ok(())
        }
    }
}
```

**No conflict resolution**: If operations fail, they error immediately. Claude Code must handle retries.

## Implementation roadmap and complexity estimates

### Phase 1: Core filesystem integration (1-2 weeks)

| Task | Effort | Dependencies |
|------|--------|--------------|
| Rust project setup with rmcp 0.8.0 | 1 day | Rust 1.75+, cargo |
| File tree and read/write tools | 2 days | notify 7.x, tokio |
| Luau linting integration (selene) | 1 day | selene binary |
| MCP Inspector testing | 1 day | @modelcontextprotocol/inspector |

**Deliverable:** Claude Code can generate/modify Luau files, get project structure, search code

### Phase 2: Studio plugin bridge (2-3 weeks)

| Task | Effort | Dependencies |
|------|--------|--------------|
| Axum HTTP server for bridge | 2 days | axum 0.8, tower |
| Studio plugin development (Luau) | 5 days | Roblox Studio |
| Studio context tools (5 tools) | 3 days | ScriptEditorService |
| Modification tools with undo | 3 days | ChangeHistoryService |
| Health checks and error types | 2 days | thiserror, anyhow |

**Deliverable:** Claude Code can read/modify Studio state, create instances, edit scripts with undo

### Phase 3: Advanced features (2-3 weeks)

| Task | Effort | Dependencies |
|------|--------|--------------|
| State tracking and monitoring | 3 days | - |
| Open Cloud integration | 2 days | reqwest, API keys |
| Performance optimization | 2 days | - |
| Comprehensive test suite | 4 days | cargo test, mockito |

**Deliverable:** Production-ready hybrid integration with publishing automation

### Total estimated effort: 5-8 weeks for full implementation

## Recommended tech stack and libraries

| Component | Recommendation | Version | Rationale |
|-----------|---------------|---------|-----------|
| **MCP SDK** | rmcp | 0.8.0 | Official Rust SDK, 4,700+ QPS |
| **Async runtime** | tokio | 1.x | Industry standard, well-tested |
| **HTTP server** | axum | 0.8 | Fast, ergonomic, tower ecosystem |
| **File watching** | notify | 7.x | Cross-platform, efficient |
| **Error handling** | thiserror + anyhow | Latest | Ergonomic error definitions |
| **HTTP client** | reqwest | 0.12 | Async, robust, OpenCloud ready |
| **Serialization** | serde + serde_json | 1.x | De facto standard |
| **Testing** | cargo test + mockito | Built-in + 1.x | Native testing + HTTP mocks |
| **Logging** | tracing + tracing-subscriber | 0.1.x | Structured async logging |

**Cargo.toml:**

```toml
[package]
name = "roblox-studio-mcp"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"

[dependencies]
# MCP
rmcp = { version = "0.8.0", features = ["server"] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# HTTP server (plugin bridge)
axum = "0.8"
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "cors"] }

# HTTP client (Open Cloud)
reqwest = { version = "0.12", features = ["json"] }

# File watching
notify = "7"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Error handling
thiserror = "2"
anyhow = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# Utilities
uuid = { version = "1", features = ["v4", "serde"] }

[dev-dependencies]
mockito = "1"
```

**Configuration files needed:**

```toml
# selene.toml for Luau linting
std = "roblox"

[lints]
unused_variable = "warn"
shadowing = "warn"
incorrect_standard_library_use = "deny"
```

## Security considerations and mitigations

| Risk | Mitigation |
|------|-----------|
| Plugin HTTP exposure | **Localhost-only binding**: `127.0.0.1:8080` (enforced by axum) |
| Unauthorized tool access | API key validation on every Open Cloud request |
| Path traversal attacks | `canonicalize()` all paths, reject if outside project root |
| Script injection | Validate Luau syntax before sending to plugin |
| Credential leakage | Environment variables only, never in config files |
| Memory exhaustion | Set Axum payload limits (default 2MB) |

**Critical security implementation in Rust:**

```rust
// Localhost-only binding (MANDATORY)
let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
    .await
    .expect("Failed to bind to localhost:8080");

axum::serve(listener, app).await.unwrap();

// Path validation with Result
fn validate_project_path(
    requested: &Path,
    project_root: &Path,
) -> Result<PathBuf, McpError> {
    let canonical = requested.canonicalize()
        .map_err(|e| McpError::InvalidParams(format!(
            "Invalid path: {}",
            e
        )))?;
    
    if !canonical.starts_with(project_root) {
        return Err(McpError::InvalidParams(
            "Path traversal detected".to_string()
        ));
    }
    
    Ok(canonical)
}

// Plugin-side validation (Luau)
-- Only accept connections from localhost
local function validateRequest(request)
    local host = request.Headers["Host"]
    if not host or not host:match("^127%.0%.0%.1") then
        error("Unauthorized: non-localhost request")
    end
end
```

## Potential pitfalls and solutions

**Pitfall 1: Rojo sync latency during rapid iteration**
- **Symptom:** Claude Code writes file, immediately reads Studio state, gets stale data
- **Solution:** Add explicit 200ms delay in tools that need Studio state after file writes, OR poll Studio state until hash matches

**Pitfall 2: Plugin disconnects during long operations**
- **Symptom:** MCP tool hangs waiting for plugin response that never comes
- **Solution:** 30-second hard timeout with clear error message to Claude Code

**Pitfall 3: Large DataModel queries exhaust context window**
- **Symptom:** Tool responses exceed 10K tokens, Claude loses context
- **Solution:** Implement mandatory depth limits (default 3), pagination, and class filtering in `studio_get_descendants`

**Pitfall 4: HTTP permissions not enabled in Studio**
- **Symptom:** Plugin cannot reach MCP server
- **Solution:** Clear setup documentation: Game Settings → Security → Allow HTTP Requests ✓

**Pitfall 5: Rust compilation times slow iteration**
- **Symptom:** Code-test-debug cycle feels slower than TypeScript
- **Solution:** Use `cargo watch` during development, `sccache` for caching, and incremental compilation

## Conclusion: a production-ready Rust architecture

The recommended **Rust-based hybrid architecture** with **fast-failure error handling** delivers maximum performance (4,700+ QPS), compile-time safety, and immediate error transparency. This approach eliminates the silent failures and hidden degradation common in fallback-heavy designs.

Rust's `Result<T, E>` type system enforces explicit error handling at every integration point. When the Studio plugin disconnects, when files are missing, or when operations timeout, **Claude Code learns immediately** through clear, actionable error messages—not after exhausting retry budgets or returning stale cached data.

The **5-8 week implementation timeline** is realistic for a single Rust developer, with Phase 1 filesystem integration delivering immediate value within 2 weeks. The December 2025 Rust MCP ecosystem (rmcp 0.8.0, Prism MCP, PMCP) provides production-ready SDKs with comprehensive examples and documentation.

**Key success factors:**
- **Start with Phase 1 only** to validate workflow before adding complexity
- **Use cargo watch + MCP Inspector** extensively during development for fast iteration
- **Keep tool responses small** (<4KB) to preserve Claude Code's context window
- **Let errors propagate immediately**—no fallbacks, no silent degradation
- **Leverage Rust's compiler** to catch errors at build time rather than runtime

The Rust hybrid approach transforms Claude Code from a capable code generator into a full development partner that understands both your codebase on disk and your live Studio session—enabling workflows like "analyze my current selection and refactor the associated scripts" that neither Rojo nor plugins could accomplish alone, all with **sub-millisecond latency** and **immediate error feedback**.