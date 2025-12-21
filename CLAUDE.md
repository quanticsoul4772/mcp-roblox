# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

Rust MCP server for Roblox Studio integration. Provides 25 MCP tools for filesystem operations, live Studio manipulation, and Open Cloud API access.

## Build and Test Commands

```bash
cargo build          # Build debug binary
cargo build --release # Build optimized release binary
cargo test           # Run 365 unit tests
cargo check          # Check for errors without building
cargo run            # Run the server
```

Binary output: `target/debug/roblox-studio-mcp` or `target/release/roblox-studio-mcp`

## Architecture

Two-part system:
1. Rust MCP Server (src/) - Exposes MCP tools via STDIO transport
2. Roblox Studio Plugin (plugin/MCPServer.server.luau) - Executes commands in Studio via HTTP polling

Communication flow:
```
MCP Client <--STDIO--> Rust Server <--HTTP:8080--> Studio Plugin <--> Roblox Studio
```

## Key Source Files

- `src/main.rs` - Entry point, STDIO transport, HTTP bridge spawning
- `src/config.rs` - Environment configuration parsing (testable)
- `src/mcp/server.rs` - All 25 MCP tool implementations
- `src/mcp/params.rs` - Tool parameter structs with JSON Schema
- `src/bridge/http.rs` - Plugin HTTP communication (poll/result endpoints)
- `src/bridge/mock.rs` - Mock bridge for testing Studio tools
- `src/cloud/` - Open Cloud API client for publishing, assets, datastores, messaging
- `src/http/` - HTTP client abstraction with mock for testing
- `src/tools/filesystem.rs` - File operations with path validation
- `src/tools/linting.rs` - Selene linter integration with mock
- `src/watcher/mod.rs` - File change detection
- `src/metrics/mod.rs` - Tool execution metrics
- `plugin/MCPServer.server.luau` - Roblox Studio plugin

## Tool Implementation Pattern

Tools use rmcp macros:
```rust
#[tool(description = "Tool description here")]
async fn tool_name(
    &self,
    Parameters(params): Parameters<ToolParams>,
) -> Result<CallToolResult, ErrorData> {
    let call = self.start_instrumentation("tool_name");
    let result = self.tool_name_impl(params).await;
    call.finish_with(result).await
}
```

## Environment Variables

- `ROBLOX_OPEN_CLOUD_API_KEY` - Required for cloud tools
- `ROBLOX_MCP_PORT` - HTTP bridge port (default: 8080)
- `RUST_LOG` - Log level (default: roblox_studio_mcp=info)

## Testing

365 unit tests cover:
- Configuration parsing and validation
- Filesystem operations and path validation
- HTTP bridge command handling
- Open Cloud API operations (with mocked HTTP)
- Mock infrastructure (bridge, HTTP client, linter)
- Error type conversions
- Tool parameter serialization
- Metrics collection
- File watcher

Integration tests require the compiled binary:

```bash
cargo test --test mcp_integration -- --ignored
```
