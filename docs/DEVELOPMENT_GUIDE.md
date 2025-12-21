# Roblox Studio MCP Development Guide

This guide covers development workflows using the MCP server to interact with Roblox Studio, including Luau scripting, Rojo project management, and Open Cloud integration.

## Architecture Overview

```
┌─────────────────┐     STDIO      ┌──────────────────┐     HTTP      ┌─────────────────┐
│   MCP Client    │◄──────────────►│  Rust MCP Server │◄────────────►│  Studio Plugin  │
│ (Claude, IDE)   │                │  (port 8081)     │               │  (Luau)         │
└─────────────────┘                └──────────────────┘               └─────────────────┘
                                            │                                  │
                                            │ HTTPS                            │
                                            ▼                                  ▼
                                   ┌──────────────────┐               ┌─────────────────┐
                                   │  Roblox Open     │               │  Roblox Studio  │
                                   │  Cloud API       │               │  DataModel      │
                                   └──────────────────┘               └─────────────────┘
```

### Components

1. **MCP Server** (Rust) - Handles MCP protocol, routes commands to plugin or cloud
2. **Studio Plugin** (Luau) - Executes commands inside Roblox Studio
3. **HTTP Bridge** - Polling-based communication between server and plugin
4. **Open Cloud Client** - Direct API calls to Roblox cloud services

## Setup Requirements

### Prerequisites

- Roblox Studio installed
- Rust toolchain (`cargo`)
- Rojo CLI (`cargo install rojo`)
- Environment variable: `ROBLOX_OPEN_CLOUD_API_KEY` (for cloud features)

### Plugin Installation

```powershell
# Build and install plugin
cd plugin
rojo build default.project.json -o MCPServer.rbxm
Copy-Item MCPServer.rbxm "$env:LOCALAPPDATA\Roblox\Plugins\" -Force
```

### Starting the Server

```powershell
# Build and run MCP server
cargo build --release
./target/release/roblox-studio-mcp.exe
```

The server listens on `127.0.0.1:8081` for plugin connections.

### Connecting in Studio

1. Open Roblox Studio
2. Go to **Plugins** tab
3. Click **Connect** button in MCP Server toolbar
4. Check Output window for: `[MCP Plugin] Server is healthy, starting poll loop`

---

## Luau Language Reference

Luau is Roblox's Lua dialect with performance optimizations and type annotations.

### Key Differences from Standard Lua

```lua
-- Type annotations (optional but recommended)
local function greet(name: string): string
    return "Hello, " .. name
end

-- Typed tables
type PlayerData = {
    name: string,
    score: number,
    inventory: {string}
}

-- Continue in loops (Luau has 'continue', but NOT 'goto')
for i = 1, 10 do
    if i % 2 == 0 then
        continue  -- Skip even numbers
    end
    print(i)
end

-- String interpolation
local name = "World"
print(`Hello, {name}!`)  -- Backtick strings with {}

-- Compound assignments
local x = 5
x += 10  -- x = x + 10
x *= 2   -- x = x * 2
```

### Luau Restrictions (vs Lua 5.1)

```lua
-- NO goto statements (use continue, break, or restructure logic)
-- WRONG:
goto skip
::skip::

-- CORRECT: Use flags or restructure
local shouldProcess = true
if condition then
    shouldProcess = false
end
if shouldProcess then
    -- process
end

-- NO setfenv/getfenv (use ModuleScripts instead)
-- NO loadstring in published games (security)
```

### Roblox Services Pattern

```lua
-- Always get services at script top
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")

-- Service availability depends on script context:
-- Server scripts: ServerScriptService, ServerStorage
-- Client scripts: StarterPlayerScripts, StarterGui
-- Shared: ReplicatedStorage, ReplicatedFirst
```

### Instance Paths

Instances are referenced by dot-separated paths from `game`:

```lua
-- These are equivalent:
local part = game.Workspace.MyPart
local part = game:GetService("Workspace"):FindFirstChild("MyPart")

-- Full paths from GetFullName()
print(part:GetFullName())  -- "Workspace.MyPart"

-- Path resolution (what our plugin does):
local function resolvePath(path)
    local parts = path:split(".")
    local current = game
    local start = 1
    if parts[1] == "game" then start = 2 end
    
    for i = start, #parts do
        current = current:FindFirstChild(parts[i])
        if not current then return nil end
    end
    return current
end
```

---

## Rojo Project Management

Rojo syncs filesystem scripts with Roblox Studio.

### Project Structure

```
my-game/
├── default.project.json    # Rojo project config
├── src/
│   ├── server/            # ServerScriptService
│   │   └── Main.server.lua
│   ├── client/            # StarterPlayerScripts  
│   │   └── Client.client.lua
│   └── shared/            # ReplicatedStorage
│       └── Utils.lua      # ModuleScript
├── assets/
│   └── models/
└── README.md
```

### File Naming Conventions

| Suffix | Instance Type | Location |
|--------|--------------|----------|
| `.server.lua` | Script | ServerScriptService |
| `.client.lua` | LocalScript | StarterPlayerScripts |
| `.lua` (no suffix) | ModuleScript | Anywhere |
| `init.lua` | ModuleScript (folder becomes module) | Anywhere |
| `init.server.lua` | Script (folder becomes script) | Server |
| `init.client.lua` | LocalScript (folder becomes script) | Client |

### Project JSON

```json
{
  "name": "MyGame",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": {
      "$className": "ServerScriptService",
      "$path": "src/server"
    },
    "ReplicatedStorage": {
      "$className": "ReplicatedStorage", 
      "$path": "src/shared"
    },
    "StarterPlayer": {
      "$className": "StarterPlayer",
      "StarterPlayerScripts": {
        "$className": "StarterPlayerScripts",
        "$path": "src/client"
      }
    }
  }
}
```

### Rojo Commands

```bash
# Build to .rbxl file
rojo build default.project.json -o game.rbxl

# Build plugin to .rbxm
rojo build default.project.json -o plugin.rbxm

# Start live sync server (Studio connects to this)
rojo serve

# Upload to Roblox (requires auth)
rojo upload default.project.json --asset_id 123456789
```

### Important: $className + $path Conflict

When using `$path` pointing to a `.server.lua` or `.client.lua` file, do NOT also specify `$className` - the file suffix already defines it:

```json
// WRONG - causes Rojo error
{
  "tree": {
    "$className": "Script",
    "$path": "MyScript.server.lua"
  }
}

// CORRECT - let suffix define class
{
  "tree": {
    "$path": "MyScript.server.lua"
  }
}
```

---

## MCP Tools Reference

### Studio Tools (require plugin connection)

#### Health & Status

```
studio_health_check
```
Check plugin connection status. Use before batch operations.

```
studio_get_output(limit?)
```
Retrieve Output window logs. Essential for debugging.

```json
// Response
{
  "logs": [
    {"message": "...", "messageType": "Enum.MessageType.MessageOutput", "timestamp": 1234567890}
  ]
}
```

#### DataModel Navigation

```
studio_get_selection
```
Get currently selected instances in Studio.

```
studio_get_datamodel(max_depth?)
```
Get full DataModel tree (recursive). Default depth: 3.

```
studio_get_datamodel_paginated(start_path?, max_depth?, limit?, cursor?)
```
Paginated traversal for large hierarchies. Returns cursor for continuation.

```
studio_find_instances(class_name, root?)
```
Find all instances of a class. Example: find all Scripts in ServerScriptService.

#### Instance Manipulation

```
studio_create_instance(class_name, parent, name, properties?, record_undo?)
```
Create new instance. Parent is a path like `"Workspace"` or `"ServerScriptService"`.

```lua
-- Creates: Workspace.MyPart
studio_create_instance("Part", "Workspace", "MyPart", {"Anchored": true})
```

```
studio_set_property(path, property, value, record_undo?)
```
Set instance property. Path uses dots: `"Workspace.MyPart"`.

```
studio_delete_instance(path, record_undo?)
```
Delete instance. Creates undo waypoint by default.

#### Script Operations

```
studio_get_script_source(path)
```
Read script source code.

```
studio_modify_script(path, new_source, record_undo?)
```
Update script content with undo support.

### Filesystem Tools

```
fs_get_tree(path, max_depth?)
```
List project file structure.

```
fs_read_script(file_path)
fs_write_script(file_path, content, create_directories?)
fs_delete_script(file_path)
```
Read/write/delete .luau files.

```
fs_search_content(path, pattern, extension)
```
Regex search in script files.

```
fs_lint_script(file_path, config_path?)
```
Run Selene linter on Luau file.

```
fs_get_changes(path)
fs_watch_changes(limit?)
```
File modification detection.

### Cloud Tools (require API key)

Set `ROBLOX_OPEN_CLOUD_API_KEY` environment variable.

```
cloud_publish_place(universe_id, place_id, rbxl_path)
```
Publish .rbxl file to Roblox.

```
cloud_upload_asset(asset_type, file_path, name, description, creator_id)
```
Upload image/model/audio asset.

```
cloud_datastore_get(universe_id, datastore_name, key, scope?)
cloud_datastore_set(universe_id, datastore_name, key, value, scope?)
```
Read/write DataStore entries.

```
cloud_messaging_publish(universe_id, topic, message)
```
Send message to live game servers via MessagingService.

---

## Common Workflows

### 1. Exploring a Place

```python
# Check connection
studio_health_check()

# Get overview
studio_get_datamodel(max_depth=2)

# Find specific types
studio_find_instances("Script", root="ServerScriptService")
studio_find_instances("ModuleScript", root="ReplicatedStorage")

# Read a script
studio_get_script_source("ServerScriptService.Main")
```

### 2. Creating Game Objects

```python
# Create a part
studio_create_instance("Part", "Workspace", "Platform", {
    "Anchored": True,
    "Size": [10, 1, 10],
    "Position": [0, 5, 0],
    "BrickColor": "Bright blue"
})

# Add a script to it
studio_create_instance("Script", "Workspace.Platform", "TouchScript")
studio_modify_script("Workspace.Platform.TouchScript", """
local part = script.Parent

part.Touched:Connect(function(hit)
    local player = game.Players:GetPlayerFromCharacter(hit.Parent)
    if player then
        print(player.Name .. " touched the platform!")
    end
end)
""")
```

### 3. Debugging Issues

```python
# Always check output first
studio_get_output(limit=50)

# Look for errors (messageType contains "Error" or "Warning")
logs = studio_get_output(100)
errors = [l for l in logs if "Error" in l["messageType"]]
```

### 4. CI/CD Pipeline

```python
# Build with Rojo
# rojo build default.project.json -o game.rbxl

# Publish to Roblox
cloud_publish_place(
    universe_id=123456789,
    place_id=987654321,
    rbxl_path="./game.rbxl"
)

# Notify live servers
cloud_messaging_publish(
    universe_id=123456789,
    topic="deployment",
    message={"version": "1.2.3", "timestamp": "2024-01-01T00:00:00Z"}
)
```

### 5. DataStore Operations

```python
# Save player data
cloud_datastore_set(
    universe_id=123456789,
    datastore_name="PlayerData",
    key="player_12345",
    value={"coins": 100, "level": 5}
)

# Load player data
data = cloud_datastore_get(
    universe_id=123456789,
    datastore_name="PlayerData", 
    key="player_12345"
)
```

---

## Best Practices

### Path Handling

- Always use simple paths: `"Workspace.MyPart"` not `"game.Workspace.MyPart"`
- The plugin's `resolvePath()` handles both, but be consistent
- Use `GetFullName()` paths from `studio_find_instances` results

### Error Handling

- Always check `studio_health_check()` before batch operations
- Monitor `studio_get_output()` for runtime errors
- Plugin errors appear in Output with `[MCP Plugin]` prefix

### Undo Support

- `record_undo=true` (default) creates undo waypoints
- Set `record_undo=false` for batch operations, then create one waypoint at end
- Users can Ctrl+Z to undo MCP changes

### Performance

- Use `studio_get_datamodel_paginated` for large hierarchies
- Limit `studio_get_output` to recent entries
- Batch related operations when possible

### Security

- Never commit `ROBLOX_OPEN_CLOUD_API_KEY`
- Cloud API keys should have minimal required permissions
- DataStore operations affect live player data - use caution

---

## Troubleshooting

### Plugin Won't Connect

1. Check server is running on port 8081
2. Verify plugin file in `%LOCALAPPDATA%\Roblox\Plugins\`
3. Restart Studio completely (close all windows)
4. Check Output for `[MCP Plugin]` messages

### Commands Fail Silently

```python
# Check output for errors
studio_get_output(20)
```

### Path Not Found

- Verify path exists: `studio_find_instances("Part", "Workspace")`
- Check spelling and case sensitivity
- Use `studio_get_datamodel()` to see actual hierarchy

### Cloud API Errors

- Verify `ROBLOX_OPEN_CLOUD_API_KEY` is set
- Check API key has required permissions
- Universe/Place IDs must match your game

---

## Plugin Development

### Adding New Commands

1. Add action handler in `plugin/MCPServer.server.lua`:

```lua
elseif action == "myNewAction" then
    local param1 = params.param1
    -- Do something
    return { success = true, result = "..." }
```

2. Add params struct in `src/mcp/params.rs`:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MyNewActionParams {
    #[schemars(description = "Description")]
    pub param1: String,
}
```

3. Add tool in `src/mcp/server.rs`:

```rust
#[tool(description = "Description of my new action")]
async fn my_new_action(
    &self,
    Parameters(params): Parameters<MyNewActionParams>,
) -> Result<CallToolResult, ErrorData> {
    let result = self.bridge
        .execute_command("myNewAction", json!({ "param1": params.param1 }))
        .await?;
    Ok(CallToolResult::success(vec![Content::text(result)]))
}
```

4. Rebuild both plugin and server

### Testing Changes

```bash
# Rebuild plugin
cd plugin && rojo build default.project.json -o MCPServer.rbxm
cp MCPServer.rbxm "$LOCALAPPDATA/Roblox/Plugins/"

# Rebuild server
cargo build --release

# Restart both Studio and MCP server
```
