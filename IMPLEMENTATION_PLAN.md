# Implementation Plan: Roblox Studio MCP Server

## Current State

The skeleton implementation includes:
- ✅ Cargo.toml with correct dependencies (rmcp 0.8.0, axum 0.8, tokio, notify)
- ✅ Error types with fast-failure philosophy (`src/error.rs`)
- ✅ HTTP bridge for plugin communication (`src/bridge/http.rs`)
- ✅ Filesystem utility functions (`src/tools/filesystem.rs`)
- ✅ Studio plugin with 5 actions (`plugin/init.lua`)
- ❌ NO MCP server registration
- ❌ NO MCP tools exposed
- ❌ Plugin missing 4 actions for full spec

## rmcp 0.8.x API Pattern (VERIFIED)

The correct pattern uses `#[tool_router]` and `#[tool]` macros:

```rust
use rmcp::{
    ServerHandler, tool, tool_router, tool_handler,
    handler::server::router::tool::ToolRouter,
    model::{ServerInfo, ServerCapabilities, CallToolResult, Content},
    schemars,
};
use rmcp::handler::server::tool::Parameters;
use serde::Deserialize;

// 1. Parameter structs with JsonSchema
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FsReadScriptParams {
    #[schemars(description = "Path to .luau file")]
    pub file_path: String,
}

// 2. Service struct with ToolRouter
#[derive(Debug, Clone)]
pub struct RobloxMcpServer {
    tool_router: ToolRouter<Self>,
    bridge: Arc<PluginBridge>,
    project_root: PathBuf,
}

// 3. Tool router impl with #[tool] methods
#[tool_router]
impl RobloxMcpServer {
    pub fn new(bridge: Arc<PluginBridge>, project_root: PathBuf) -> Self {
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
        }
    }

    #[tool(description = "Read a Luau script file")]
    async fn fs_read_script(
        &self,
        Parameters(params): Parameters<FsReadScriptParams>,
    ) -> Result<CallToolResult, McpError> {
        // Implementation
        Ok(CallToolResult::success(vec![Content::text(content)]))
    }
}

// 4. ServerHandler with capabilities
#[tool_handler]
impl ServerHandler for RobloxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Roblox Studio MCP Server".into()),
        }
    }
}
```

## Transport Architecture (FIXED)

HTTP bridge runs as spawned task, STDIO MCP on main thread:

```rust
use rmcp::transport::stdio;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // CRITICAL: Initialize logging to STDERR (stdout is for JSON-RPC protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "roblox_studio_mcp=info".into())
        )
        .init();

    // Create shared plugin bridge
    let bridge = Arc::new(PluginBridge::new());
    let project_root = std::env::current_dir()?;

    // Spawn HTTP bridge as background task
    let http_bridge = bridge.clone();
    tokio::spawn(async move {
        let app = create_router(http_bridge);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
            .await
            .expect("Failed to bind HTTP bridge");
        tracing::info!("HTTP bridge listening on 127.0.0.1:8080");
        axum::serve(listener, app).await.expect("HTTP server error");
    });

    // Create MCP server
    let server = RobloxMcpServer::new(bridge, project_root);

    // Run MCP server on STDIO (blocks)
    tracing::info!("Starting MCP server on STDIO");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
```

---

## Implementation Phases

### Phase 1: MCP Core + Filesystem Tools (3-4 days)

**Objective:** Enable Claude Code to connect and use filesystem tools.

#### Step 1.1: Update Cargo.toml

Add required features (already applied):
```toml
rmcp = { version = "0.8.0", features = ["server", "transport-io", "macros"] }
schemars = "0.8"  # Required for parameter schemas
```

**Critical**: The `transport-io` feature is required for STDIO transport.

#### Step 1.2: Create MCP Module

Create `src/mcp/mod.rs`:
```rust
pub mod server;
pub mod params;

pub use server::RobloxMcpServer;
```

Create `src/mcp/params.rs` (parameter structs):
```rust
use serde::Deserialize;
use schemars::JsonSchema;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsGetTreeParams {
    #[schemars(description = "Root path to explore")]
    pub path: String,
    #[schemars(description = "Maximum depth (default: 5)")]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsReadScriptParams {
    #[schemars(description = "Path to .luau file")]
    pub file_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsWriteScriptParams {
    #[schemars(description = "Path to .luau file")]
    pub file_path: String,
    #[schemars(description = "Script content")]
    pub content: String,
    #[schemars(description = "Create parent directories if missing")]
    pub create_directories: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsDeleteScriptParams {
    #[schemars(description = "Path to .luau file to delete")]
    pub file_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsSearchContentParams {
    #[schemars(description = "Directory to search")]
    pub path: String,
    #[schemars(description = "Regex pattern to match")]
    pub pattern: String,
    #[schemars(description = "File extension filter (e.g., 'luau')")]
    pub extension: Option<String>,
}
```

Create `src/mcp/server.rs`:
```rust
use std::sync::Arc;
use std::path::PathBuf;
use rmcp::{
    ServerHandler, tool, tool_router, tool_handler,
    handler::server::router::tool::ToolRouter,
    handler::server::tool::Parameters,
    model::*,
    schemars,
};
use crate::bridge::http::PluginBridge;
use crate::error::RobloxMcpError;
use crate::mcp::params::*;

#[derive(Clone)]
pub struct RobloxMcpServer {
    tool_router: ToolRouter<Self>,
    bridge: Arc<PluginBridge>,
    project_root: PathBuf,
}

#[tool_router]
impl RobloxMcpServer {
    pub fn new(bridge: Arc<PluginBridge>, project_root: PathBuf) -> Self {
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
        }
    }

    // === FILESYSTEM TOOLS (6) ===

    #[tool(description = "List project file structure with depth limits")]
    async fn fs_get_tree(
        &self,
        Parameters(params): Parameters<FsGetTreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Implementation using existing build_tree function
    }

    #[tool(description = "Read a Luau script file")]
    async fn fs_read_script(
        &self,
        Parameters(params): Parameters<FsReadScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Implementation using existing read_script function
    }

    #[tool(description = "Write or create a Luau script file")]
    async fn fs_write_script(
        &self,
        Parameters(params): Parameters<FsWriteScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Implementation using existing write_script function
    }

    #[tool(description = "Delete a Luau script file")]
    async fn fs_delete_script(
        &self,
        Parameters(params): Parameters<FsDeleteScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // New implementation
    }

    #[tool(description = "Search for patterns in script files")]
    async fn fs_search_content(
        &self,
        Parameters(params): Parameters<FsSearchContentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // New implementation using regex
    }

    #[tool(description = "Watch for file changes (returns current state, use polling)")]
    async fn fs_get_changes(
        &self,
        Parameters(params): Parameters<FsGetTreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Simplified: return file mtimes for change detection
    }
}

#[tool_handler]
impl ServerHandler for RobloxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Roblox Studio MCP Server. Provides filesystem and Studio integration tools."
                    .into(),
            ),
        }
    }
}
```

#### Step 1.3: Update main.rs

Replace current main.rs with transport architecture above.

#### Verification

```powershell
cargo build --release
```

Add to Claude Code MCP config (`%APPDATA%\Claude\claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "roblox-studio": {
      "command": "C:\\Development\\Projects\\MCP\\project-root\\mcp-servers\\mcp-roblox\\target\\release\\roblox-studio-mcp.exe"
    }
  }
}
```

Restart Claude Code, verify 6 filesystem tools appear.

---

### Phase 2: Studio Bridge Tools (4-5 days)

**Objective:** Enable Claude Code to interact with live Roblox Studio.

#### Step 2.1: Add Studio Parameter Structs

Add to `src/mcp/params.rs`:
```rust
// === STUDIO PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetDataModelParams {
    #[schemars(description = "Maximum depth to traverse (default: 3)")]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetScriptSourceParams {
    #[schemars(description = "Full path to script (e.g., 'game.ServerScriptService.Main')")]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioModifyScriptParams {
    #[schemars(description = "Full path to script")]
    pub path: String,
    #[schemars(description = "New script content")]
    pub new_source: String,
    #[schemars(description = "Record undo waypoint (default: true)")]
    pub record_undo: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioCreateInstanceParams {
    #[schemars(description = "Roblox class name (e.g., 'Part', 'Script')")]
    pub class_name: String,
    #[schemars(description = "Parent path (e.g., 'game.Workspace')")]
    pub parent: String,
    #[schemars(description = "Instance name")]
    pub name: String,
    #[schemars(description = "Properties to set")]
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioSetPropertyParams {
    #[schemars(description = "Instance path")]
    pub path: String,
    #[schemars(description = "Property name")]
    pub property: String,
    #[schemars(description = "Property value")]
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioDeleteInstanceParams {
    #[schemars(description = "Instance path to delete")]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioFindInstancesParams {
    #[schemars(description = "Class name to find")]
    pub class_name: String,
    #[schemars(description = "Root to search from (default: 'game')")]
    pub root: Option<String>,
}
```

#### Step 2.2: Add Studio Tools to Server

Add to `src/mcp/server.rs` inside the `#[tool_router] impl`:
```rust
    // === STUDIO TOOLS (6) ===

    #[tool(description = "Get currently selected instances in Roblox Studio")]
    async fn studio_get_selection(&self) -> Result<CallToolResult, ErrorData> {
        let result = self.bridge.execute_command("getSelection", json!({})).await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(result.to_string())]))
    }

    #[tool(description = "Explore the live Studio DataModel hierarchy")]
    async fn studio_get_datamodel(
        &self,
        Parameters(params): Parameters<StudioGetDataModelParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self.bridge.execute_command(
            "getDataModel",
            json!({ "maxDepth": params.max_depth.unwrap_or(3) }),
        ).await.map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(result.to_string())]))
    }

    #[tool(description = "Read script source from Studio editor")]
    async fn studio_get_script_source(
        &self,
        Parameters(params): Parameters<StudioGetScriptSourceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Uses bridge.execute_command("getScriptSource", ...)
    }

    #[tool(description = "Modify script in Studio with undo support")]
    async fn studio_modify_script(
        &self,
        Parameters(params): Parameters<StudioModifyScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Uses bridge.execute_command("modifyScript", ...)
    }

    #[tool(description = "Create a new instance in Studio")]
    async fn studio_create_instance(
        &self,
        Parameters(params): Parameters<StudioCreateInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Uses bridge.execute_command("createInstance", ...)
    }

    #[tool(description = "Set a property on an instance")]
    async fn studio_set_property(
        &self,
        Parameters(params): Parameters<StudioSetPropertyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Uses bridge.execute_command("setProperty", ...)
    }

    #[tool(description = "Delete an instance from Studio")]
    async fn studio_delete_instance(
        &self,
        Parameters(params): Parameters<StudioDeleteInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Uses bridge.execute_command("deleteInstance", ...)
    }

    #[tool(description = "Find instances by class name")]
    async fn studio_find_instances(
        &self,
        Parameters(params): Parameters<StudioFindInstancesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Uses bridge.execute_command("findInstances", ...)
    }
```

#### Step 2.3: Update Plugin with Missing Actions

Modify `plugin/init.lua` to add missing actions:

```lua
-- Add to executeCommand function:

elseif action == "setProperty" then
    local instance = game:FindFirstChild(params.path, true)
    if not instance then
        error("Instance not found: " .. params.path)
    end

    local success, err = pcall(function()
        instance[params.property] = params.value
    end)

    if not success then
        error("Failed to set property: " .. tostring(err))
    end

    if params.recordUndo ~= false then
        ChangeHistoryService:SetWaypoint("MCP Set Property")
    end

    return { success = true }

elseif action == "deleteInstance" then
    local instance = game:FindFirstChild(params.path, true)
    if not instance then
        error("Instance not found: " .. params.path)
    end

    if params.recordUndo ~= false then
        ChangeHistoryService:SetWaypoint("MCP Delete Instance")
    end

    instance:Destroy()
    return { success = true }

elseif action == "findInstances" then
    local root = game
    if params.root then
        root = game:FindFirstChild(params.root, true)
        if not root then
            error("Root not found: " .. params.root)
        end
    end

    local results = {}
    for _, desc in ipairs(root:GetDescendants()) do
        if desc.ClassName == params.className then
            table.insert(results, {
                Name = desc.Name,
                ClassName = desc.ClassName,
                Path = desc:GetFullName()
            })
        end
    end

    return { instances = results }
```

#### Verification

1. Build: `cargo build --release`
2. Restart Claude Code
3. Open Roblox Studio, enable HTTP requests (Game Settings → Security)
4. Install plugin, click "Connect"
5. Ask Claude Code to use studio tools

---

### Phase 3: Advanced Features (2-3 days)

**Objective:** Add file watching notifications and Open Cloud integration.

#### Step 3.1: File Change Tracking

Add simple file mtime tracking (simpler than full notify watcher for MCP):

```rust
#[tool(description = "Get file modification times for change detection")]
async fn fs_get_mtimes(
    &self,
    Parameters(params): Parameters<FsGetTreeParams>,
) -> Result<CallToolResult, ErrorData> {
    // Return HashMap<path, mtime> for all .luau files
}
```

#### Step 3.2: Open Cloud Client (Optional)

Create `src/cloud/mod.rs` for publishing automation.

---

## Complete Tool List (15 Tools)

### Filesystem Tools (6)
| Tool | Description | Status |
|------|-------------|--------|
| `fs_get_tree` | List project structure | Phase 1 |
| `fs_read_script` | Read .luau file | Phase 1 |
| `fs_write_script` | Write .luau file | Phase 1 |
| `fs_delete_script` | Delete .luau file | Phase 1 |
| `fs_search_content` | Search with regex | Phase 1 |
| `fs_get_changes` | Get file mtimes | Phase 1 |

### Studio Tools (9)
| Tool | Plugin Action | Status |
|------|---------------|--------|
| `studio_get_selection` | `getSelection` | Phase 2 (plugin ready) |
| `studio_get_datamodel` | `getDataModel` | Phase 2 (plugin ready) |
| `studio_get_script_source` | `getScriptSource` | Phase 2 (plugin ready) |
| `studio_modify_script` | `modifyScript` | Phase 2 (plugin ready) |
| `studio_create_instance` | `createInstance` | Phase 2 (plugin ready) |
| `studio_set_property` | `setProperty` | Phase 2 (plugin needs update) |
| `studio_delete_instance` | `deleteInstance` | Phase 2 (plugin needs update) |
| `studio_find_instances` | `findInstances` | Phase 2 (plugin needs update) |
| `studio_get_services` | `getServices` | Phase 3 (optional) |

---

## File Changes Summary

### New Files
```
src/mcp/mod.rs           # MCP module exports
src/mcp/server.rs        # RobloxMcpServer with all tools
src/mcp/params.rs        # Parameter structs with JsonSchema
src/cloud/mod.rs         # Open Cloud client (Phase 3)
```

### Modified Files
```
Cargo.toml               # Add schemars dependency
src/main.rs              # New transport architecture
src/tools/mod.rs         # Export tool modules
plugin/init.lua          # Add 3 missing actions
```

### Unchanged Files
```
src/error.rs             # Error types complete
src/bridge/mod.rs        # Bridge exports complete
src/bridge/http.rs       # HTTP bridge complete
src/tools/filesystem.rs  # Utility functions (used by MCP tools)
```

---

## Testing Strategy

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_fs_read_script_valid() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.luau");
        std::fs::write(&path, "print('hello')").unwrap();

        // Test read_script function
    }

    #[tokio::test]
    async fn test_fs_read_script_wrong_extension() {
        // Should return error for .txt files
    }
}
```

### Integration Tests

1. Build: `cargo build --release`
2. Configure in Claude Code MCP settings
3. Restart Claude Code
4. Open Roblox Studio, connect plugin
5. Ask Claude Code to use the tools in conversation

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| rmcp API changes | Pin to exact version 0.8.0, add schemars 0.8 |
| Plugin HTTP blocked | Document: Game Settings → Security → Allow HTTP |
| Large DataModel responses | Default max_depth=3, document limits |
| STDIO blocks main thread | HTTP bridge spawned as background task |
| Plugin missing actions | Phase 2 includes plugin updates |

---

## Success Criteria

### Phase 1 Complete When:
- [ ] `cargo build` succeeds with no errors
- [ ] Claude Code shows 6 filesystem tools available
- [ ] `fs_read_script` returns .luau file contents
- [ ] `fs_write_script` creates file, Rojo syncs to Studio

### Phase 2 Complete When:
- [ ] Claude Code shows 14 total tools (6 fs + 8 studio)
- [ ] `studio_get_selection` returns selected instances
- [ ] `studio_modify_script` edits script with undo waypoint
- [ ] Plugin disconnect shows clear timeout error
- [ ] New plugin actions (`setProperty`, `deleteInstance`, `findInstances`) work

### Phase 3 Complete When:
- [ ] `fs_get_changes` returns file mtimes for change detection
- [ ] All 15 tools functional
- [ ] Open Cloud publishing works (if implemented)

---

## References

- [rmcp docs](https://docs.rs/rmcp/latest/rmcp/)
- [Rust SDK GitHub](https://github.com/modelcontextprotocol/rust-sdk)
- [Shuttle STDIO Tutorial](https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust)
