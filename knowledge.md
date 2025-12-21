# Project knowledge

Rust MCP server for Roblox Studio integration. Provides 25 MCP tools for filesystem operations, live Studio manipulation, and Open Cloud API access.

## Quickstart
- Setup: `cargo build`
- Dev: `cargo run` (uses STDIO transport)
- Test: `cargo test` (365 unit tests)
- Release: `cargo build --release`

## Architecture
- Key directories:
  - `src/mcp/` - MCP tool implementations and params
  - `src/bridge/` - HTTP communication with Studio plugin
  - `src/cloud/` - Open Cloud API client (datastores, messaging, assets)
  - `src/tools/` - Filesystem ops and Selene linting
  - `plugin/` - Roblox Studio plugin (MCPServer.server.luau)
- Data flow: `MCP Client <--STDIO--> Rust Server <--HTTP:8080--> Studio Plugin <--> Roblox Studio`

## Conventions
- Formatting/linting: `cargo fmt`, `cargo clippy`
- Patterns to follow:
  - Tools use rmcp macros with `#[tool(description = "...")]`
  - All tools wrap execution in `start_instrumentation()` / `finish_with()` for metrics
  - Use mock traits for testability (MockBridge, MockHttpClient, MockLinter)
- Things to avoid:
  - Direct HTTP client usage - use the `HttpClient` trait abstraction
  - Hardcoded paths - use `PathBuf` and validate against project root

## Environment Variables
- `ROBLOX_OPEN_CLOUD_API_KEY` - Required for cloud tools
- `ROBLOX_MCP_PORT` - HTTP bridge port (default: 8080)
- `RUST_LOG` - Log level (default: roblox_studio_mcp=info)

## Testing
- Unit tests: `cargo test`
- Integration tests: `cargo build && cargo test --test mcp_integration -- --ignored`
- Mock infrastructure in `src/bridge/mock.rs`, `src/http/mock.rs`
