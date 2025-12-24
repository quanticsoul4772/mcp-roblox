# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Project Overview

Rust MCP server for Roblox Studio integration. Provides **44 MCP tools** for filesystem operations, live Studio manipulation, Open Cloud API access, AI-powered code search, and toolchain integration.

## Build and Test Commands

```bash
cargo build          # Build debug binary
cargo build --release # Build optimized release binary
cargo test           # Run 920+ unit tests
cargo check          # Check for errors without building
cargo clippy         # Run linter
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

- `src/main.rs` - Entry point, STDIO transport, HTTP bridge spawning, AI initialization
- `src/config.rs` - Environment configuration parsing (testable)
- `src/limits.rs` - Resource limits (MAX_SEARCH_RESULTS, MAX_TREE_ENTRIES)
- `src/regex_safety.rs` - Regex DoS protection with pattern validation
- `src/mcp/server.rs` - MCP server with tool routing and AI integration
- `src/mcp/params.rs` - Tool parameter structs with JSON Schema
- `src/mcp/instrumentation.rs` - Metrics collection wrapper
- `src/mcp/tools/` - Domain-organized tool implementations:
  - `filesystem.rs` - fs_* tool implementations
  - `studio.rs` - studio_* tool implementations
  - `cloud.rs` - cloud_* tool implementations
  - `toolchain.rs` - stylua/rojo/wally/moonwave implementations
  - `ai.rs` - ai_* tool implementations
- `src/bridge/http.rs` - Plugin HTTP communication (poll/result endpoints)
- `src/bridge/auth.rs` - Bearer token authentication for plugin
- `src/bridge/mock.rs` - Mock bridge for testing Studio tools
- `src/cloud/client.rs` - Open Cloud API client (API key protected with secrecy)
- `src/cloud/traits.rs` - CloudClient trait for dependency injection
- `src/cloud/mock.rs` - Mock cloud client for testing cloud tools
- `src/cloud/ordered_datastores.rs` - OrderedDataStore operations (leaderboards)
- `src/cloud/universes.rs` - Universe info and server restart
- `src/http/` - HTTP client abstraction with mock for testing
- `src/tools/filesystem.rs` - File operations with path validation
- `src/tools/linting.rs` - Selene linter integration with mock
- `src/tools/formatting.rs` - StyLua formatter integration
- `src/tools/rojo.rs` - Rojo build and sourcemap operations
- `src/tools/wally.rs` - Wally package management
- `src/tools/moonwave.rs` - Moonwave documentation builds
- `src/tools/timeout.rs` - External tool timeout protection (30s default)
- `src/ai/` - AI-powered semantic code search (feature-gated):
  - `config.rs` - Voyage AI and Neo4j configuration
  - `embedder.rs` - Voyage AI embedding client
  - `knowledge_graph.rs` - Neo4j knowledge graph operations
  - `auto_indexer.rs` - Real-time file watching and indexing
  - `parser.rs` - Luau syntax parsing for relationships
  - `mock.rs` - Mock implementations for testing
- `src/watcher/mod.rs` - File change detection
- `src/metrics/mod.rs` - Tool execution metrics
- `plugin/MCPServer.server.luau` - Roblox Studio plugin

## Tool Categories (44 total)

| Category | Count | Tools |
|----------|-------|-------|
| Filesystem | 8 | fs_get_tree, fs_read_script, fs_write_script, fs_delete_script, fs_search_content, fs_get_changes, fs_lint_script, fs_watch_changes |
| Studio | 14 | studio_health_check, studio_get_selection, studio_get_datamodel, studio_get_datamodel_paginated, studio_get_script_source, studio_get_properties, studio_get_bounds, studio_modify_script, studio_create_instance, studio_insert_r15_rig, studio_set_property, studio_delete_instance, studio_find_instances, studio_get_output |
| Cloud | 11 | cloud_publish_place, cloud_upload_asset, cloud_datastore_get, cloud_datastore_set, cloud_ordered_datastore_list, cloud_ordered_datastore_set, cloud_ordered_datastore_increment, cloud_ordered_datastore_delete, cloud_get_universe, cloud_restart_servers, cloud_messaging_publish |
| AI | 4 | ai_index_project, ai_search_codebase, ai_find_related, ai_get_context |
| Toolchain | 6 | stylua_format, rojo_build, rojo_sourcemap, wally_install, wally_update, moonwave_build |
| Monitoring | 1 | server_get_metrics |

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

### Core
- `ROBLOX_OPEN_CLOUD_API_KEY` - Required for cloud tools (protected with secrecy crate)
- `ROBLOX_MCP_PORT` - HTTP bridge port (default: 8080)
- `RUST_LOG` - Log level (default: roblox_studio_mcp=info)

### AI (for semantic code search)
- `VOYAGE_API_KEY` - Voyage AI API key for embeddings (required for AI tools)
- `VOYAGE_MODEL` - Embedding model (default: voyage-code-3)
- `VOYAGE_DIMENSIONS` - Vector dimensions (default: 1024)
- `NEO4J_URI` - Neo4j connection URI (required for AI tools)
- `NEO4J_USERNAME` - Neo4j username (default: neo4j)
- `NEO4J_PASSWORD` - Neo4j password (required for AI tools)
- `NEO4J_DATABASE` - Neo4j database name (default: neo4j)

## Security Features

- **Regex DoS Protection**: `validate_regex_safety()` in `src/regex_safety.rs` rejects dangerous patterns
- **API Key Protection**: Uses `secrecy::Secret<String>` for automatic redaction in logs
- **Path Traversal Prevention**: `validate_path()` in `src/tools/filesystem.rs` blocks `..` sequences
- **Symlink Protection**: `reject_if_symlink()` prevents path escape attacks
- **Tool Timeouts**: `execute_with_timeout()` in `src/tools/timeout.rs` (30s default)
- **HTTP Authentication**: Bearer token auth in `src/bridge/auth.rs`

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

### OrderedDataStore API

For leaderboards and ranked data:
```
GET https://apis.roblox.com/ordered-data-stores/v1/universes/{id}/orderedDataStores/{name}/scopes/{scope}/entries
  ?max_page_size={limit}
  &order_by={desc|asc}
  &filter={filter_expression}
```

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

920+ unit tests cover:
- Configuration parsing and validation
- Filesystem operations and path validation
- HTTP bridge command handling
- Open Cloud API operations (with mocked HTTP)
- Cloud tool success paths (with MockCloudClient)
- AI tools (with MockKnowledgeGraph and MockEmbeddingProvider)
- Mock infrastructure (bridge, HTTP client, linter, cloud, AI)
- Error type conversions
- Tool parameter serialization
- Metrics collection
- File watcher
- Security features (regex safety, path traversal, symlinks)
- External tool timeouts

Integration tests require the compiled binary:

```bash
cargo test --test mcp_integration -- --ignored
```

## AI Tools Usage

### Indexing a Project
```rust
ai_index_project(path?, force?)  // Indexes .luau files, generates embeddings
```
- Creates embeddings via Voyage AI and stores in Neo4j
- Extracts code relationships (requires, remote calls, events)

### Semantic Code Search
```rust
ai_search_codebase(query, limit?, min_similarity?)
```
- Returns scripts matching by meaning, not just keywords
- Similarity scores 0.0-1.0 (default min: 0.5)

### Finding Related Scripts
```rust
ai_find_related(path, max_depth?)  // Finds scripts through code relationships
```
- Uses graph traversal to discover dependencies
- Tracks requires, RemoteEvents, BindableEvents

### Getting Context for Tasks
```rust
ai_get_context(task, token_budget?)  // Returns relevant snippets within budget
```
- Optimized for LLM context windows (default: 4000 tokens)

## Resource Limits

Defined in `src/limits.rs`:
- `MAX_SEARCH_RESULTS`: 1000 - Max lines returned by fs_search_content
- `MAX_FILE_ENTRIES`: 10000 - Max files tracked by fs_get_changes
- `MAX_TREE_ENTRIES`: 10000 - Max entries in build_tree output

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

## R15 NPC Patterns (Critical)

### Animation Requirements
- **Never add WeldConstraints to body parts** - They override Motor6D transforms, causing T-pose
- R15 rigs use 15 Motor6D joints that animations transform - welds lock these
- Only `HumanoidRootPart` needs `CanCollide = true`; other parts use Motor6D to stay connected

### Clothing for MeshPart Avatars
- **Shirt/Pants instances don't work** on `CreateHumanoidModelFromDescription()` avatars
- Must set clothing via `HumanoidDescription.Shirt` and `.Pants` properties (asset IDs as numbers)

### Server-Spawned NPC Animation
- LocalScripts fail (no `LocalPlayer` for NPCs)
- Use client-side animation handler in `StarterPlayerScripts` that scans for NPCs
- Check `Players:GetPlayerFromCharacter(model)` returns nil to identify NPCs
