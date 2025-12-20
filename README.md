# Roblox Studio MCP Server

A Rust MCP (Model Context Protocol) server for Roblox Studio integration. Provides filesystem operations, live Studio manipulation, and Open Cloud API access through a standardized tool interface.

## Features

### Filesystem Tools (7 tools)
- `fs_get_tree` - List project file structure with configurable depth limits
- `fs_read_script` - Read Luau script files
- `fs_write_script` - Write or create Luau script files with optional directory creation
- `fs_delete_script` - Delete Luau script files
- `fs_search_content` - Search for patterns in script files using regex
- `fs_get_changes` - Get file modification times for change detection
- `fs_lint_script` - Run Selene linter on Luau scripts (requires Selene installed)

### Studio Tools (8 tools)
Requires the companion Roblox Studio plugin to be running.

- `studio_get_selection` - Get currently selected instances in Studio
- `studio_get_datamodel` - Explore the live DataModel hierarchy
- `studio_get_datamodel_paginated` - Paginated DataModel traversal for large hierarchies
- `studio_get_script_source` - Read script source from Studio instances
- `studio_modify_script` - Modify script source with undo support
- `studio_create_instance` - Create new instances with initial properties
- `studio_set_property` - Set properties on instances
- `studio_delete_instance` - Delete instances with undo support
- `studio_find_instances` - Find all instances of a specific class

### Open Cloud Tools (3 tools)
Requires `ROBLOX_OPEN_CLOUD_API_KEY` environment variable.

- `cloud_publish_place` - Publish .rbxl files to Roblox
- `cloud_upload_asset` - Upload images, models, or audio
- `cloud_datastore_get` - Read from DataStores

### Monitoring Tools (2 tools)
- `fs_watch_changes` - Poll for real-time file changes
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

The `plugin/init.lua` file contains a Roblox Studio plugin that communicates with the MCP server via HTTP. To use Studio tools:

1. Copy `plugin/init.lua` to your Roblox Studio plugins folder
2. Start the MCP server
3. Click the "Connect" button in the Studio toolbar

The plugin features automatic reconnection with exponential backoff if the connection is lost.

## Architecture

```
mcp-roblox/
├── src/
│   ├── main.rs           # Entry point, STDIO transport, HTTP bridge
│   ├── mcp/
│   │   ├── server.rs     # MCP tool implementations
│   │   ├── params.rs     # Tool parameter definitions
│   │   └── instrumentation.rs  # Metrics collection
│   ├── bridge/
│   │   └── http.rs       # Plugin HTTP communication
│   ├── cloud/
│   │   ├── client.rs     # Open Cloud API client
│   │   ├── assets.rs     # Asset upload
│   │   └── datastores.rs # DataStore operations
│   ├── tools/
│   │   ├── filesystem.rs # File operations
│   │   └── linting.rs    # Selene integration
│   ├── watcher/          # File change detection
│   ├── metrics/          # Server metrics
│   └── error.rs          # Error types
└── plugin/
    └── init.lua          # Roblox Studio plugin
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

The project includes 98 unit tests covering:
- Filesystem operations and path validation
- HTTP bridge command handling
- Error type conversions
- Tool parameter serialization
- Metrics collection

```bash
cargo test
```

Integration tests require the compiled binary:

```bash
cargo test --test mcp_integration -- --ignored
```

## License

MIT OR Apache-2.0
