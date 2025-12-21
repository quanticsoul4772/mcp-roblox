# Roblox Studio MCP Server

A Rust MCP (Model Context Protocol) server for Roblox Studio integration. Provides filesystem operations, live Studio manipulation, and Open Cloud API access through a standardized tool interface.

## Features

### Filesystem Tools (8 tools)
- `fs_get_tree` - List project file structure with configurable depth limits
- `fs_read_script` - Read Luau script files
- `fs_write_script` - Write or create Luau script files with optional directory creation
- `fs_delete_script` - Delete Luau script files
- `fs_search_content` - Search for patterns in script files using regex
- `fs_get_changes` - Get file modification times for change detection
- `fs_lint_script` - Run Selene linter on Luau scripts (requires Selene installed)
- `fs_watch_changes` - Poll for real-time file changes

### Studio Tools (11 tools)
Requires the companion Roblox Studio plugin to be running.

- `studio_health_check` - Check plugin connection status
- `studio_get_selection` - Get currently selected instances in Studio
- `studio_get_datamodel` - Explore the live DataModel hierarchy
- `studio_get_datamodel_paginated` - Paginated DataModel traversal for large hierarchies
- `studio_get_script_source` - Read script source from Studio instances
- `studio_modify_script` - Modify script source with undo support
- `studio_create_instance` - Create new instances with initial properties
- `studio_set_property` - Set properties on instances (supports BrickColor, Vector3, Color3, UDim2)
- `studio_delete_instance` - Delete instances with undo support
- `studio_find_instances` - Find all instances of a specific class
- `studio_get_output` - Get recent Output window logs from Studio

### Open Cloud Tools (5 tools)
Requires `ROBLOX_OPEN_CLOUD_API_KEY` environment variable.

- `cloud_publish_place` - Publish .rbxl files to Roblox
- `cloud_upload_asset` - Upload images, models, or audio
- `cloud_datastore_get` - Read from DataStores
- `cloud_datastore_set` - Write to DataStores
- `cloud_messaging_publish` - Publish messages to MessagingService topics

### Monitoring Tools (1 tool)
- `server_get_metrics` - Get tool execution counts, durations, and error rates

## Requirements

- Rust 1.75 or later
- Roblox Studio (for Studio tools)
- Selene (optional, for linting - install with `cargo install selene`)

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
- Dot-notation path resolution (e.g., "Workspace.Part.SubPart")
- Automatic type conversion for BrickColor, Vector3, Color3, UDim2
- Output log capture for debugging

## Architecture

```
mcp-roblox/
├── src/
│   ├── main.rs           # Entry point, STDIO transport, HTTP bridge
│   ├── config.rs         # Environment configuration parsing
│   ├── error.rs          # Error types and MCP error conversion
│   ├── mcp/
│   │   ├── server.rs     # MCP tool implementations (25 tools)
│   │   ├── params.rs     # Tool parameter definitions with JSON Schema
│   │   └── instrumentation.rs  # Metrics collection wrapper
│   ├── bridge/
│   │   ├── http.rs       # Plugin HTTP communication (poll/result endpoints)
│   │   └── mock.rs       # Mock bridge for testing
│   ├── cloud/
│   │   ├── client.rs     # Open Cloud API client
│   │   ├── traits.rs     # CloudClient trait for DI
│   │   ├── mock.rs       # Mock cloud client for testing
│   │   ├── assets.rs     # Asset upload operations
│   │   ├── datastores.rs # DataStore get/set operations
│   │   └── messaging.rs  # MessagingService publish
│   ├── http/
│   │   ├── mod.rs        # HTTP client trait abstraction
│   │   ├── reqwest_client.rs  # Production HTTP client
│   │   └── mock.rs       # Mock HTTP client for testing
│   ├── tools/
│   │   ├── filesystem.rs # File operations with path validation
│   │   └── linting.rs    # Selene linter integration
│   ├── watcher/          # File change detection
│   └── metrics/          # Tool execution metrics
└── plugin/
    ├── MCPServer.server.luau  # Roblox Studio plugin source
    └── default.project.json   # Rojo build configuration
```

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run

# Build release binary
cargo build --release
```

## Testing

The project includes 499 unit tests (86.7% coverage) covering:
- Configuration parsing and environment variable handling
- Filesystem operations and path validation
- HTTP bridge command handling and edge cases
- Open Cloud API operations (DataStores, Messaging, Assets)
- Cloud tool success paths (with MockCloudClient)
- Mock infrastructure for dependency injection (bridge, HTTP client, linter, cloud)
- Error type conversions and MCP error mapping
- Tool parameter serialization with JSON Schema
- Metrics collection and instrumentation
- File watcher change detection
- Selene output parsing

```bash
cargo test
```

Integration tests require the compiled binary (they spawn the actual server process):

```bash
cargo build && cargo test --test mcp_integration -- --ignored
```

## Documentation

- [Development Guide](docs/DEVELOPMENT_GUIDE.md) - Workflows, Luau reference, and tool usage
- [API Reference](docs/API_REFERENCE.md) - Public traits, types, and MCP tools
- [Testing Patterns](docs/TESTING_PATTERNS.md) - Mock infrastructure and testing best practices

## License

MIT OR Apache-2.0
