# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

Rust MCP server for Roblox Studio integration. Provides 27 MCP tools for filesystem operations, live Studio manipulation, and Open Cloud API access.

## Build and Test Commands

```bash
cargo build          # Build debug binary
cargo build --release # Build optimized release binary
cargo test           # Run 607 unit tests
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
- `src/mcp/server.rs` - All 27 MCP tool implementations
- `src/mcp/params.rs` - Tool parameter structs with JSON Schema
- `src/bridge/http.rs` - Plugin HTTP communication (poll/result endpoints)
- `src/bridge/mock.rs` - Mock bridge for testing Studio tools
- `src/cloud/` - Open Cloud API client for publishing, assets, datastores, messaging
- `src/cloud/traits.rs` - CloudClient trait for dependency injection
- `src/cloud/mock.rs` - Mock cloud client for testing cloud tools
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

## Open Cloud API Details

### DataStore API

Uses v1 API endpoint format with query parameters:
```
GET/POST https://apis.roblox.com/datastores/v1/universes/{universe_id}/standard-datastores/datastore/entries/entry
  ?datastoreName={datastore_name}
  &entryKey={key}
  &scope={scope}
```

**Required headers for datastore_set:**
- `x-api-key`: API key from environment
- `content-type`: `application/json`
- `content-md5`: Base64-encoded MD5 hash of request body

### Property Types in Studio Tools

When using `studio_create_instance` or `studio_set_property`:
- `Vector3`: `[x, y, z]` array
- `Color3`: `[r, g, b]` array (0-1 range)
- `BrickColor`: String name like "Bright red", "Cyan"
- `Material`: String like "Neon", "Concrete", "SmoothPlastic"
- `Enum`: String values like "Ball" for Shape

**Known Limitation:** UDim2 properties not supported via JSON

### Reading Properties and Bounds

Use `studio_get_properties(path, properties?)` to read instance properties:
- Returns common properties for the class if `properties` array omitted
- Response includes typed values: `{"Position": {"type": "Vector3", "value": [x, y, z]}}`

Use `studio_get_bounds(path)` to get bounding box of Parts/Models:
- Returns `center`, `size`, `min`, `max` coordinates and `orientation`
- Useful for verifying placement and calculating furniture positions

### Script Modification

Use `record_undo: false` when modifying scripts to avoid "script document not available" errors:
```rust
studio_modify_script(path, source, record_undo: false)
```

## Testing

607 unit tests (78% coverage) cover:
- Configuration parsing and validation
- Filesystem operations and path validation
- HTTP bridge command handling
- Open Cloud API operations (with mocked HTTP)
- Cloud tool success paths (with MockCloudClient)
- Mock infrastructure (bridge, HTTP client, linter, cloud)
- Error type conversions
- Tool parameter serialization
- Metrics collection
- File watcher

Integration tests require the compiled binary:

```bash
cargo test --test mcp_integration -- --ignored
```

## Documentation

- `docs/DEVELOPMENT_GUIDE.md` - Workflows, Luau reference, tool usage, **production-quality patterns**
- `docs/API_REFERENCE.md` - Public traits, types, and MCP tools
- `docs/TESTING_PATTERNS.md` - Mock infrastructure and testing patterns
- `Building production-quality Roblox games.md` - Reference guide for professional Roblox development

## Production Game Patterns (Quick Reference)

Key patterns from the production guide:

- **Service/Controller Architecture**: Server logic in Services (ServerScriptService), client in Controllers (StarterPlayerScripts)
- **RemoteEvents > RemoteFunctions**: RemoteFunctions can hang; use RemoteEvents with callback patterns
- **Performance Budgets**: 500K triangles, 500 drawcalls, <1.3GB memory, <50KB/s network
- **Memory Leaks**: Always `:Disconnect()` event connections; use Trove/Maid patterns
- **Security**: Never trust client; validate types, NaN, sanity limits, cooldowns on every RemoteEvent
- **DataStore**: Use `UpdateAsync()` not `SetAsync()`; wrap in `pcall()`; implement `BindToClose()`
- **Toolchain**: Rojo + Wally + Selene + StyLua + Luau LSP
