# Roblox Studio MCP Server

**Status: SKELETON ONLY - NOT FUNCTIONAL**

Rust MCP server project for Roblox Studio integration. Currently only has basic HTTP bridge skeleton - no MCP tools implemented yet.

## What Actually Exists

- ✅ Compiles to binary (with warnings)
- ✅ Basic HTTP server skeleton on `127.0.0.1:8080`
- ✅ Error type definitions
- ✅ Unused filesystem utility functions
- ✅ Studio plugin skeleton (untested)
- ❌ NO MCP tool registration
- ❌ NO functional tools
- ❌ NO actual MCP server implementation

## Prerequisites

- Rust 1.75+ (already installed)
- Roblox Studio
- Rojo CLI (for file sync)

## Build

```powershell
cd C:\Development\Projects\MCP\project-root\mcp-servers\mcp-roblox
cargo build
```

Binary: `target\debug\roblox-studio-mcp.exe`

## Run

```powershell
cargo run
```

This will start the HTTP bridge on `127.0.0.1:8080` but it won't do anything useful yet.

## Project Structure

```
mcp-roblox/
├── Cargo.toml              # Dependencies
├── src/
│   ├── main.rs             # Entry point (HTTP server only)
│   ├── error.rs            # Error types
│   ├── tools/
│   │   └── filesystem.rs   # Unused utility functions
│   └── bridge/
│       └── http.rs         # Plugin HTTP bridge skeleton
└── plugin/
    └── init.lua            # Untested Studio plugin
```

## What Needs To Be Implemented

Everything. This is just a skeleton.

## Development

```powershell
# Build
cargo build

# Run
cargo run

# Check for errors
cargo check

# Run tests (none exist)
cargo test
```
