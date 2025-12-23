# Roblox Studio MCP Server - Project Knowledge Base

High-performance Rust MCP server for Roblox Studio integration. Provides **39 MCP tools** across 5 categories for filesystem operations, live Studio manipulation, Open Cloud API access, toolchain integration, and monitoring.

---

## Quick Start

```bash
cargo build              # Build debug binary
cargo build --release    # Build optimized release binary
cargo test               # Run 770 unit tests
cargo run                # Run MCP server (STDIO transport)
cargo clippy             # Run linter
```

**Binary output:** `target/release/roblox-studio-mcp` (or `.exe` on Windows)

---

## Architecture Overview

```
┌─────────────────┐     STDIO      ┌──────────────────┐     HTTP:8080    ┌─────────────────┐
│   MCP Client    │◄──────────────►│  Rust MCP Server │◄────────────────►│  Studio Plugin  │
│ (Claude, IDE)   │                │                  │                  │  (Luau)         │
└─────────────────┘                └──────────────────┘                  └─────────────────┘
                                           │                                    │
                                           │ HTTPS                              ▼
                                           ▼                            ┌─────────────────┐
                                  ┌──────────────────┐                  │  Roblox Studio  │
                                  │  Roblox Open     │                  │  DataModel      │
                                  │  Cloud API       │                  └─────────────────┘
                                  └──────────────────┘
```

---

## Source Directory Structure

```
src/
├── main.rs                    # Entry point, STDIO transport, HTTP bridge spawning
├── config.rs                  # Environment configuration parsing
├── error.rs                   # Error types and MCP error code mapping
├── limits.rs                  # Resource limits (MAX_SEARCH_RESULTS, etc.)
├── regex_safety.rs            # Regex DoS protection
├── startup.rs                 # Initialization helpers
│
├── mcp/                       # MCP Protocol Implementation
│   ├── server.rs              # All 39 tool implementations
│   ├── params.rs              # Tool parameter structs (JSON Schema)
│   └── instrumentation.rs     # Metrics collection wrapper
│
├── bridge/                    # Studio Plugin Communication
│   ├── mod.rs                 # StudioBridge trait definition
│   ├── http.rs                # PluginBridge - HTTP polling implementation
│   ├── auth.rs                # Bearer token authentication
│   └── mock.rs                # MockBridge for testing
│
├── cloud/                     # Roblox Open Cloud API
│   ├── mod.rs                 # Module exports
│   ├── client.rs              # OpenCloudClient implementation
│   ├── traits.rs              # CloudClient trait for DI
│   ├── mock.rs                # MockCloudClient for testing
│   ├── assets.rs              # Asset upload operations
│   ├── datastores.rs          # DataStore get/set operations
│   ├── ordered_datastores.rs  # OrderedDataStore (leaderboards)
│   ├── universes.rs           # Universe info and server restart
│   └── messaging.rs           # MessagingService publish
│
├── http/                      # HTTP Client Abstraction
│   ├── mod.rs                 # HttpClient trait definition
│   ├── reqwest_client.rs      # Production reqwest implementation
│   └── mock.rs                # MockHttpClient for testing
│
├── tools/                     # External Tool Integrations
│   ├── mod.rs                 # Module exports
│   ├── filesystem.rs          # File operations with path validation
│   ├── linting.rs             # Selene linter (with MockLinter)
│   ├── formatting.rs          # StyLua formatter (with MockFormatter)
│   ├── rojo.rs                # Rojo build/sourcemap (with MockRojoRunner)
│   ├── wally.rs               # Wally package management (with MockWallyRunner)
│   ├── moonwave.rs            # Moonwave docs (with MockMoonwaveRunner)
│   └── timeout.rs             # External tool timeout protection (30s)
│
├── watcher/                   # File Change Detection
│   └── mod.rs                 # FileWatcher implementation
│
├── metrics/                   # Server Monitoring
│   └── mod.rs                 # ServerMetrics for tool execution tracking
│
└── tasks/                     # Background Task Management
    └── mod.rs                 # Task scheduling utilities

plugin/
├── MCPServer.server.luau      # Roblox Studio plugin source
└── default.project.json       # Rojo build configuration

tests/
├── mcp_integration.rs         # Integration tests (spawn server process)
└── toolchain_integration.rs   # External tool integration tests
```

---

## MCP Tools Reference (39 Total)

### Filesystem Tools (8)
| Tool | Description |
|------|-------------|
| `fs_get_tree` | List project file structure with depth limits |
| `fs_read_script` | Read .luau script files |
| `fs_write_script` | Write/create .luau scripts with directory creation |
| `fs_delete_script` | Delete .luau script files |
| `fs_search_content` | Regex search in scripts (DoS-protected) |
| `fs_get_changes` | Get file modification times for change detection |
| `fs_lint_script` | Run Selene linter (requires `selene` installed) |
| `fs_watch_changes` | Poll for real-time file changes |

### Studio Tools (13) - Requires Plugin Connection
| Tool | Description |
|------|-------------|
| `studio_health_check` | Check plugin connection status |
| `studio_get_selection` | Get currently selected instances |
| `studio_get_datamodel` | Explore DataModel hierarchy |
| `studio_get_datamodel_paginated` | Paginated DataModel for large hierarchies |
| `studio_get_script_source` | Read script source from Studio |
| `studio_get_properties` | Read instance properties |
| `studio_get_bounds` | Get bounding box of Parts/Models |
| `studio_modify_script` | Update script source (with undo support) |
| `studio_create_instance` | Create new instances with properties |
| `studio_set_property` | Set instance properties |
| `studio_delete_instance` | Delete instances (with undo support) |
| `studio_find_instances` | Find all instances by class name |
| `studio_get_output` | Get recent Output window logs |

### Cloud Tools (11) - Requires `ROBLOX_OPEN_CLOUD_API_KEY`
| Tool | Description |
|------|-------------|
| `cloud_publish_place` | Publish .rbxl files to Roblox |
| `cloud_upload_asset` | Upload images, models, or audio |
| `cloud_datastore_get` | Read from DataStores |
| `cloud_datastore_set` | Write to DataStores |
| `cloud_ordered_datastore_list` | List OrderedDataStore entries (leaderboards) |
| `cloud_ordered_datastore_set` | Set OrderedDataStore entry |
| `cloud_ordered_datastore_increment` | Atomically increment entry value |
| `cloud_ordered_datastore_delete` | Delete OrderedDataStore entry |
| `cloud_get_universe` | Get universe (game) metadata |
| `cloud_restart_servers` | Restart all game servers |
| `cloud_messaging_publish` | Publish to MessagingService topics |

### Toolchain Tools (6)
| Tool | Description | Requires |
|------|-------------|----------|
| `stylua_format` | Format Luau scripts | `stylua` |
| `rojo_build` | Build Roblox projects | `rojo` |
| `rojo_sourcemap` | Generate sourcemaps | `rojo` |
| `wally_install` | Install Wally packages | `wally` |
| `wally_update` | Update packages to latest | `wally` |
| `moonwave_build` | Build documentation | `moonwave` |

### Monitoring Tools (1)
| Tool | Description |
|------|-------------|
| `server_get_metrics` | Tool execution counts, durations, error rates |

---

## Key Traits (Dependency Injection)

| Trait | Location | Purpose | Implementations |
|-------|----------|---------|-----------------|
| `StudioBridge` | `src/bridge/mod.rs` | Plugin communication | `PluginBridge`, `MockBridge` |
| `CloudClient` | `src/cloud/traits.rs` | Open Cloud API | `OpenCloudClient`, `MockCloudClient` |
| `HttpClient` | `src/http/mod.rs` | HTTP operations | `ReqwestHttpClient`, `MockHttpClient` |
| `Linter` | `src/tools/linting.rs` | Script linting | `SeleneLinter`, `MockLinter` |
| `Formatter` | `src/tools/formatting.rs` | Code formatting | `StyLuaFormatter`, `MockFormatter` |
| `RojoRunner` | `src/tools/rojo.rs` | Rojo builds | `DefaultRojoRunner`, `MockRojoRunner` |
| `WallyRunner` | `src/tools/wally.rs` | Package management | `DefaultWallyRunner`, `MockWallyRunner` |
| `MoonwaveRunner` | `src/tools/moonwave.rs` | Doc generation | `DefaultMoonwaveRunner`, `MockMoonwaveRunner` |

---

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ROBLOX_OPEN_CLOUD_API_KEY` | No | - | API key for cloud tools (protected with `secrecy`) |
| `ROBLOX_MCP_PORT` | No | 8080 | HTTP bridge port for plugin communication |
| `RUST_LOG` | No | `roblox_studio_mcp=info` | Log level configuration |

---

## Resource Limits

| Constant | Value | Location | Description |
|----------|-------|----------|-------------|
| `MAX_SEARCH_RESULTS` | 1000 | `src/limits.rs` | Max lines from `fs_search_content` |
| `MAX_FILE_ENTRIES` | 10000 | `src/limits.rs` | Max files in `fs_get_changes` |
| `MAX_TREE_ENTRIES` | 10000 | `src/limits.rs` | Max entries in `build_tree` output |

---

## Security Features

| Feature | Location | Description |
|---------|----------|-------------|
| Regex DoS Protection | `src/regex_safety.rs` | Validates patterns before compilation |
| API Key Protection | `src/cloud/client.rs` | Uses `secrecy::Secret<String>` for auto-redaction |
| Path Traversal Prevention | `src/tools/filesystem.rs` | Blocks `..` sequences in paths |
| Symlink Protection | `src/tools/filesystem.rs` | Rejects symlinks to prevent escapes |
| Tool Timeouts | `src/tools/timeout.rs` | 30s timeout for external tools |
| HTTP Authentication | `src/bridge/auth.rs` | Bearer token for plugin communication |

---

## Testing Patterns

### Test Coverage
- **770 unit tests** covering all major components
- Mock infrastructure for all external dependencies
- Integration tests in `tests/` directory

### Mock Queue Pattern (CloudClient)
```rust
let mock = Arc::new(MockCloudClient::new());
mock.queue_datastore_get(Ok(DataStoreEntry { ... }));
let server = create_test_server(root).with_cloud_client(mock);
```

### Mock Response Map Pattern (Bridge)
```rust
let mock = MockBridge::new();
mock.set_response("getSelection", json!({ "selected": [...] }));
let server = RobloxMcpServer::with_mock_bridge(Arc::new(mock), root);
```

---

## Property Type Mappings (Studio Tools)

| Roblox Type | JSON Format | Example |
|-------------|-------------|---------|
| `Vector3` | `[x, y, z]` array | `[0, 5, 0]` |
| `Color3` | `[r, g, b]` array (0-1) | `[1, 0, 0]` |
| `BrickColor` | String name | `"Bright red"` |
| `Material` | String name | `"Neon"` |
| `Enum` | String value | `"Ball"` |

**Known Limitation:** `UDim2` not supported via JSON.

---

## Tool Implementation Pattern

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

---

## Documentation Index

### Project Documentation
| Document | Path | Description |
|----------|------|-------------|
| README | `README.md` | Project overview and setup |
| CLAUDE.md | `CLAUDE.md` | AI assistant instructions |
| API Reference | `docs/API_REFERENCE.md` | Traits, types, and tools |
| Development Guide | `docs/DEVELOPMENT_GUIDE.md` | Workflows, Luau reference |
| Testing Patterns | `docs/TESTING_PATTERNS.md` | Mock infrastructure |
| Production Patterns | `Building production-quality Roblox games.md` | Game development best practices |

### Obsidian Knowledge Base (Roblox Development)
| Note | Description |
|------|-------------|
| `Roblox/City Template Analysis.md` | Comprehensive technical analysis of Roblox's official City Template |
| `Roblox/City Template - Script Best Practices.md` | Code patterns: networking, input handling, audio, lifecycle management |
| `Roblox/City Template - Deep Technical Reference.md` | Precise values: lighting, vehicle physics, GUI specs, building structure |
| `Roblox/City Template - Advanced Systems.md` | Deep dive into immersion layers and advanced systems |
| `Roblox/City Recreation Guide.md` | Step-by-step procedural generation guide for recreating city environments |
| `Roblox/Modular Building Quick Reference.md` | Piece dimensions, naming conventions, assembly order checklist |

#### City Template Quick Reference
- **Lighting**: ClockTime 15.5 (golden hour), Atmosphere density 0.325
- **Vehicle Physics**: 16 constraints, 2000 Nm motors, underdamped suspension (ζ=0.205)
- **Buildings**: 10 prefabs (A-J), 8-folder hierarchy per building
- **Street Props**: 30-40 stud lamp spacing, A/B variant alternation

---

## Luau/Roblox Gotchas

### `pairs()` Iteration Order is UNDEFINED
```lua
-- WRONG: pairs() gives random order
for layerName, generator in pairs(self.generationLayers) do
    generator(chunk)  -- order unpredictable!
end

-- CORRECT: Use ipairs() with explicit order
local LAYER_ORDER = {"terrain", "road", "lot", "building"}
for _, layerName in ipairs(LAYER_ORDER) do
    self.generationLayers[layerName](chunk)
end
```

### ModuleScript Require Paths
```lua
require(script.Parent.SiblingModule)           -- sibling
require(script.Parent.Subfolder.Module)        -- nested child
require(script.Parent.Parent.OtherFolder.Mod)  -- up then down
-- File names are CASE-SENSITIVE
```

### Script Context Locations
| Script Type | Container | Access |
|-------------|-----------|--------|
| Server Script | `ServerScriptService` | `ServerStorage`, datastores |
| LocalScript | `StarterPlayerScripts` | Client-only services |
| ModuleScript | Anywhere | Shared code via `require()` |

---

## Quick Reference Commands

```bash
# Development
cargo build                    # Build debug
cargo run                      # Run with STDIO transport
RUST_LOG=debug cargo run       # Debug logging

# Testing
cargo test                     # All unit tests
cargo test --test mcp_integration -- --ignored  # Integration tests
cargo test tool_name           # Specific test

# Quality
cargo fmt                      # Format code
cargo clippy                   # Lint
cargo doc --open               # Generate docs

# Plugin
cd plugin && rojo build -o MCPServer.rbxm   # Build plugin
# Copy to: %LOCALAPPDATA%\Roblox\Plugins\ (Windows)
```

---

*Last updated: 2025-12-22*
