# Roblox Studio MCP Server

A Rust MCP (Model Context Protocol) server for Roblox Studio integration. Provides filesystem operations, live Studio manipulation, Open Cloud API access, and toolchain integration through a standardized tool interface.

## Features

### Filesystem Tools (8 tools)
- `fs_get_tree` - List project file structure with configurable depth limits
- `fs_read_script` - Read Luau script files
- `fs_write_script` - Write or create Luau script files with optional directory creation
- `fs_delete_script` - Delete Luau script files
- `fs_search_content` - Search for patterns in script files using regex (with DoS protection)
- `fs_get_changes` - Get file modification times for change detection
- `fs_lint_script` - Run Selene linter on Luau scripts (requires Selene installed)
- `fs_watch_changes` - Poll for real-time file changes

### Studio Tools (14 tools)
Requires the companion Roblox Studio plugin to be running.

- `studio_health_check` - Check plugin connection status
- `studio_get_selection` - Get currently selected instances in Studio
- `studio_get_datamodel` - Explore the live DataModel hierarchy
- `studio_get_datamodel_paginated` - Paginated DataModel traversal for large hierarchies
- `studio_get_script_source` - Read script source from Studio instances
- `studio_get_properties` - Read properties from any instance
- `studio_get_bounds` - Get bounding box of Parts/Models
- `studio_modify_script` - Modify script source with undo support
- `studio_create_instance` - Create new instances with initial properties
- `studio_insert_r15_rig` - Insert a complete R15 humanoid rig with Motor6D joints
- `studio_set_property` - Set properties on instances (supports BrickColor, Vector3, Color3)
- `studio_delete_instance` - Delete instances with undo support
- `studio_find_instances` - Find all instances of a specific class
- `studio_get_output` - Get recent Output window logs from Studio

### Open Cloud Tools (11 tools)
Requires `ROBLOX_OPEN_CLOUD_API_KEY` environment variable.

- `cloud_publish_place` - Publish .rbxl files to Roblox
- `cloud_upload_asset` - Upload images, models, or audio
- `cloud_datastore_get` - Read from DataStores
- `cloud_datastore_set` - Write to DataStores
- `cloud_ordered_datastore_list` - List entries from OrderedDataStores (leaderboards)
- `cloud_ordered_datastore_set` - Set entries in OrderedDataStores
- `cloud_ordered_datastore_increment` - Atomically increment OrderedDataStore values
- `cloud_ordered_datastore_delete` - Delete entries from OrderedDataStores
- `cloud_get_universe` - Get universe (game) metadata
- `cloud_restart_servers` - Restart all game servers for a universe
- `cloud_messaging_publish` - Publish messages to MessagingService topics

### Toolchain Tools (6 tools)
Integration with Roblox development toolchain.

- `stylua_format` - Format Luau scripts with StyLua (requires StyLua installed)
- `rojo_build` - Build Roblox projects with Rojo (requires Rojo installed)
- `rojo_sourcemap` - Generate Rojo sourcemaps for debugging
- `wally_install` - Install Wally packages (requires Wally installed)
- `wally_update` - Update Wally packages to latest versions
- `moonwave_build` - Build documentation with Moonwave (requires Moonwave installed)

### Monitoring Tools (1 tool)
- `server_get_metrics` - Get tool execution counts, durations, and error rates

## Requirements

- Rust 1.75 or later
- Roblox Studio (for Studio tools)
- Optional toolchain:
  - Selene (`cargo install selene`) - for linting
  - StyLua (`cargo install stylua`) - for formatting
  - Rojo (`cargo install rojo`) - for project builds
  - Wally (`aftman install wally`) - for package management
  - Moonwave (`npm install -g moonwave`) - for documentation

## Installation

```bash
git clone https://github.com/quanticsoul4772/mcp-roblox.git
cd mcp-roblox
cargo build --release
```

The binary will be at `target/release/roblox-studio-mcp.exe` (Windows) or `target/release/roblox-studio-mcp` (Linux/macOS).

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ROBLOX_OPEN_CLOUD_API_KEY` | No | - | API key for Open Cloud tools |
| `ROBLOX_MCP_PORT` | No | 8080 | HTTP bridge port for plugin communication |
| `RUST_LOG` | No | `roblox_studio_mcp=info` | Log level configuration |

### MCP Client Configuration

Add to your MCP client configuration (e.g., Claude Desktop):

```json
{
  "mcpServers": {
    "roblox-studio": {
      "command": "path/to/roblox-studio-mcp",
      "args": [],
      "env": {
        "ROBLOX_OPEN_CLOUD_API_KEY": "your-api-key-here"
      }
    }
  }
}
```

## Studio Plugin

The `plugin/MCPServer.server.luau` file contains a Roblox Studio plugin that communicates with the MCP server via HTTP. To use Studio tools:

### Option 1: Build with Rojo (Recommended)
```bash
cd plugin
rojo build -o MCPServer.rbxm
```
Then copy `MCPServer.rbxm` to your Roblox Studio plugins folder:
- Windows: `%LOCALAPPDATA%\Roblox\Plugins`
- macOS: `~/Documents/Roblox/Plugins`

### Option 2: Install Plugin Directly
Copy `plugin/MCPServer.server.luau` directly to your plugins folder (renamed as needed).

### Usage
1. Start the MCP server
2. Open Roblox Studio
3. Click the "Connect" button in the Studio toolbar

The plugin features:
- Automatic reconnection with exponential backoff
- Bearer token authentication for security
- Dot-notation path resolution (e.g., "Workspace.Part.SubPart")
- Automatic type conversion for BrickColor, Vector3, Color3
- Output log capture for debugging

## Architecture

```
mcp-roblox/
├── src/
│   ├── main.rs              # Entry point, STDIO transport, HTTP bridge
│   ├── config.rs            # Environment configuration parsing
│   ├── error.rs             # Error types and MCP error conversion
│   ├── limits.rs            # Resource limits (search results, tree entries)
│   ├── regex_safety.rs      # Regex DoS protection
│   ├── mcp/
│   │   ├── server.rs        # All 40 MCP tool implementations
│   │   ├── params.rs        # Tool parameter definitions with JSON Schema
│   │   └── instrumentation.rs  # Metrics collection wrapper
│   ├── bridge/
│   │   ├── http.rs          # Plugin HTTP communication (poll/result endpoints)
│   │   ├── auth.rs          # Bearer token authentication
│   │   └── mock.rs          # Mock bridge for testing
│   ├── cloud/
│   │   ├── client.rs        # Open Cloud API client (with API key protection)
│   │   ├── traits.rs        # CloudClient trait for DI
│   │   ├── mock.rs          # Mock cloud client for testing
│   │   ├── assets.rs        # Asset upload operations
│   │   ├── datastores.rs    # DataStore get/set operations
│   │   ├── ordered_datastores.rs  # OrderedDataStore operations
│   │   ├── universes.rs     # Universe info and server restart
│   │   └── messaging.rs     # MessagingService publish
│   ├── http/
│   │   ├── mod.rs           # HTTP client trait abstraction
│   │   ├── reqwest_client.rs  # Production HTTP client
│   │   └── mock.rs          # Mock HTTP client for testing
│   ├── tools/
│   │   ├── filesystem.rs    # File operations with path validation
│   │   ├── linting.rs       # Selene linter integration
│   │   ├── formatting.rs    # StyLua formatter integration
│   │   ├── rojo.rs          # Rojo build/sourcemap operations
│   │   ├── wally.rs         # Wally package management
│   │   ├── moonwave.rs      # Moonwave documentation builds
│   │   └── timeout.rs       # External tool timeout protection
│   ├── watcher/             # File change detection
│   └── metrics/             # Tool execution metrics
└── plugin/
    ├── MCPServer.server.luau  # Roblox Studio plugin source
    └── default.project.json   # Rojo build configuration
```

## Security Features

- **Regex DoS Protection**: User-provided regex patterns are validated to prevent catastrophic backtracking
- **API Key Protection**: Cloud API keys use the `secrecy` crate for automatic memory redaction
- **Path Traversal Prevention**: All file operations validate paths to prevent directory traversal attacks
- **Tool Timeouts**: External tools (StyLua, Selene, Rojo, Wally, Moonwave) have 30-second timeout protection
- **HTTP Authentication**: Plugin communication uses bearer token authentication
- **Symlink Protection**: Filesystem operations reject symlinks to prevent path escapes

## Development

```bash
# Build
cargo build

# Run tests (770 tests)
cargo test

# Run with debug logging
RUST_LOG=debug cargo run

# Build release binary
cargo build --release

# Run clippy
cargo clippy
```

## Testing

The project includes 780+ unit tests covering:
- Configuration parsing and environment variable handling
- Filesystem operations and path validation
- HTTP bridge command handling and edge cases
- Open Cloud API operations (DataStores, OrderedDataStores, Messaging, Assets, Universes)
- Cloud tool success paths (with MockCloudClient)
- Mock infrastructure for dependency injection (bridge, HTTP client, linter, cloud)
- Error type conversions and MCP error mapping
- Tool parameter serialization with JSON Schema
- Metrics collection and instrumentation
- File watcher change detection
- Security features (regex safety, path traversal, symlink rejection)
- External tool timeout handling

```bash
cargo test
```

Integration tests require the compiled binary (they spawn the actual server process):

```bash
cargo build && cargo test --test mcp_integration -- --ignored
```

## Documentation

- [Development Guide](docs/DEVELOPMENT_GUIDE.md) - Workflows, Luau reference, tool usage, production patterns
- [API Reference](docs/API_REFERENCE.md) - Public traits, types, and MCP tools
- [Testing Patterns](docs/TESTING_PATTERNS.md) - Mock infrastructure and testing best practices

## License

MIT OR Apache-2.0
