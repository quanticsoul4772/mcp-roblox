# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Status

**SKELETON ONLY - NOT FUNCTIONAL.** This is a Rust MCP server project for Roblox Studio integration. Currently only has HTTP bridge skeleton - no MCP tools are implemented yet.

## Build & Run Commands

```powershell
# Build the project
cargo build

# Run the server (starts HTTP bridge on 127.0.0.1:8080)
cargo run

# Check for errors without building
cargo check

# Run tests (none exist yet)
cargo test

# Build optimized release binary
cargo build --release
```

Binary output: `target\debug\roblox-studio-mcp.exe` (or `target\release\` for release builds)

## Architecture

### Two-Part System
1. **Rust MCP Server** (`src/`) - Runs locally, will expose MCP tools to AI assistants
2. **Roblox Studio Plugin** (`plugin/init.lua`) - Runs inside Studio, executes commands via HTTP polling

### Communication Flow
```
AI Assistant <--MCP--> Rust Server <--HTTP--> Studio Plugin <--API--> Roblox Studio
                         (8080)       poll/result
```

### Key Components

**`src/main.rs`** - Entry point. Initializes logging with tracing and starts Axum HTTP server on localhost:8080.

**`src/bridge/http.rs`** - Plugin communication bridge:
- `PluginBridge` struct manages command queue and result handling
- `/poll` endpoint - Plugin polls for pending commands
- `/result` endpoint - Plugin returns command execution results
- Uses oneshot channels for request-response correlation with 30s hard timeout

**`src/error.rs`** - Custom error types (`RobloxMcpError`) with fast-failure philosophy. Converts to MCP protocol `ErrorData`.

**`src/tools/filesystem.rs`** - Utility functions (currently unused):
- Path validation with traversal protection
- Recursive file tree building
- Luau script read/write operations

**`plugin/init.lua`** - Roblox Studio plugin that:
- Polls HTTP server every 0.5s for commands
- Supports actions: `getSelection`, `getScriptSource`, `modifyScript`, `getDataModel`, `createInstance`
- Returns results/errors back to server

### Key Dependencies
- `rmcp` - Rust MCP SDK (server mode + macros)
- `schemars` - JSON Schema for tool parameters
- `axum` - HTTP server framework
- `tokio` - Async runtime
- `notify` - File watching (for future Rojo sync)

## rmcp Tool Pattern

Tools use `#[tool_router]` and `#[tool]` macros:
```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MyParams { pub field: String }

#[tool_router]
impl RobloxMcpServer {
    #[tool(description = "Tool description")]
    async fn my_tool(&self, Parameters(p): Parameters<MyParams>) -> Result<CallToolResult, ErrorData> { }
}
```

## Transport Architecture

HTTP bridge spawns as background task, STDIO MCP runs on main thread:
```rust
tokio::spawn(run_http_bridge(bridge.clone()));  // Background
server.serve(stdio()).await?;                    // Main thread (blocks)
```

## What Needs Implementation

See `IMPLEMENTATION_PLAN.md` for detailed phases:
- **Phase 1**: MCP server + 6 filesystem tools
- **Phase 2**: 8 Studio bridge tools + plugin updates
- **Phase 3**: File change tracking + Open Cloud
