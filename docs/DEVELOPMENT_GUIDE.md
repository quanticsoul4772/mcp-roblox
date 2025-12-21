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

**API Key Setup:**
1. Go to https://create.roblox.com/dashboard/credentials
2. Click "Create API Key"
3. Add "Data Stores" under Access Permissions
4. Select your experience and enable Read/Write operations
5. Set IP restrictions (0.0.0.0/0 for testing)

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
Read/write DataStore entries. Uses v1 API endpoint format internally.

**Note:** Find your Universe ID at https://create.roblox.com/dashboard/creations - click on your experience and the ID is in the URL.

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
# Create a part with properties
studio_create_instance("Part", "Workspace", "Platform", {
    "Anchored": True,
    "Size": [10, 1, 10],        # Vector3 as array [x, y, z]
    "Position": [0, 5, 0],
    "BrickColor": "Bright blue", # Use BrickColor name strings
    "Material": "Neon",          # Material enum as string
    "Shape": "Ball",             # PartType: "Block", "Ball", "Cylinder"
    "Transparency": 0.5          # 0 = opaque, 1 = invisible
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
""", record_undo=False)  # Use record_undo=False for script modifications
```

**Supported Property Types:**
- `Vector3`: `[x, y, z]` array
- `Color3`: `[r, g, b]` array (0-1 range)
- `BrickColor`: String name like "Bright red", "Cyan", "Neon orange"
- `Material`: String like "Neon", "Concrete", "SmoothPlastic", "Grass"
- `Enum`: String values like "Ball" for Shape, "Bottom" for Face

**Known Limitations:**
- `UDim2` properties (GUI sizing) not supported via JSON
- Complex properties may need script-based modification

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

### 6. Creating Interactive Objects

```python
# Create a clickable button with ClickDetector
studio_create_instance("Part", "Workspace", "Button", {
    "Anchored": True,
    "Size": [4, 3, 4],
    "Position": [0, 2, 0],
    "BrickColor": "Bright green"
})
studio_create_instance("ClickDetector", "Workspace.Button", "ClickDetector", {
    "MaxActivationDistance": 20
})
studio_create_instance("Script", "Workspace.Button", "ClickHandler")
studio_modify_script("Workspace.Button.ClickHandler", """
local button = script.Parent
local clickDetector = button:WaitForChild("ClickDetector")

clickDetector.MouseClick:Connect(function(player)
    print(player.Name .. " clicked the button!")
    -- Flash effect
    button.BrickColor = BrickColor.new("White")
    task.wait(0.1)
    button.BrickColor = BrickColor.new("Bright green")
end)
""", record_undo=False)
```

### 7. Creating Lights

```python
# PointLight inside a neon part
studio_create_instance("Part", "Workspace", "GlowOrb", {
    "Anchored": True,
    "Size": [3, 3, 3],
    "Position": [0, 5, 0],
    "BrickColor": "Cyan",
    "Material": "Neon",
    "Shape": "Ball"
})
studio_create_instance("PointLight", "Workspace.GlowOrb", "Light", {
    "Range": 20,
    "Brightness": 2,
    "Color": [0, 1, 1]  # RGB 0-1 range
})

# SpotLight on ceiling
studio_create_instance("SpotLight", "Workspace.Ceiling", "Spotlight", {
    "Range": 30,
    "Brightness": 2,
    "Angle": 45,
    "Face": "Bottom"  # Which face light points from
})
```

### 8. Animation with TweenService

```lua
-- In a Luau script, animate parts smoothly
local TweenService = game:GetService("TweenService")
local part = script.Parent

local tweenInfo = TweenInfo.new(
    2,                          -- Duration in seconds
    Enum.EasingStyle.Linear,    -- Easing style
    Enum.EasingDirection.InOut, -- Easing direction
    -1,                         -- RepeatCount (-1 = infinite)
    true                        -- Reverses
)

local tween = TweenService:Create(part, tweenInfo, {
    Position = Vector3.new(10, 5, 0)  -- Target position
})
tween:Play()
```

### 9. Game State Management

```lua
-- Track per-player state
local Players = game:GetService("Players")
local playerData = {}

Players.PlayerAdded:Connect(function(player)
    playerData[player] = {
        score = 0,
        hasWon = false
    }

    -- Create leaderstats
    local leaderstats = Instance.new("Folder")
    leaderstats.Name = "leaderstats"
    leaderstats.Parent = player

    local score = Instance.new("IntValue")
    score.Name = "Score"
    score.Value = 0
    score.Parent = leaderstats
end)

Players.PlayerRemoving:Connect(function(player)
    playerData[player] = nil  -- Cleanup
end)
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

## Production-Quality Game Development

This section covers professional practices for building scalable, secure Roblox games.

### Service/Controller Architecture

Professional Roblox games use a **Service/Controller pattern** that separates server and client code:

| Location | Purpose | Access |
|----------|---------|--------|
| **ServerScriptService** | Server services + bootstrap script | Server only |
| **ServerStorage** | Server-only assets, prefabs | Server only |
| **ReplicatedStorage** | Shared modules, RemoteEvents, constants | Client + Server |
| **StarterPlayerScripts** | Client controllers + bootstrap script | Client only |
| **ReplicatedFirst** | Loading screens, critical first-load assets | Client + Server |

```lua
-- Example Service (ServerScriptService/Services/CoinService.lua)
local CoinService = {}

function CoinService:Init()
    -- Setup dependencies (runs first across all modules)
    self.coins = {}
end

function CoinService:Start()
    -- Connect events (runs after all Init calls complete)
    game.Workspace.ChildAdded:Connect(function(child)
        if child:IsA("Part") and child.Name == "Coin" then
            self:RegisterCoin(child)
        end
    end)
end

function CoinService:RegisterCoin(coin)
    self.coins[coin] = true
end

return CoinService
```

**Key principle:** Use `Init()` for setup, `Start()` for event connections. This two-phase initialization prevents race conditions.

### RemoteEvents vs RemoteFunctions

**Always prefer RemoteEvents over RemoteFunctions.** RemoteFunctions can hang the server if a client doesn't respond—a vulnerability exploiters abuse.

```lua
-- WRONG: RemoteFunction can hang
local result = remoteFunction:InvokeClient(player, data)  -- Dangerous!

-- CORRECT: RemoteEvent with callback pattern
local requestId = HttpService:GenerateGUID()
requestEvent:FireClient(player, requestId, data)
responseEvent.OnServerEvent:Connect(function(player, responseId, result)
    if responseId == requestId then
        -- Handle response
    end
end)
```

### Performance Budgets

Professional games target these thresholds (from Roblox staff engineers):

| Metric | Budget | Notes |
|--------|--------|-------|
| Triangle count | ~500,000 | Total in scene |
| Drawcalls | ~500 | Batch similar materials |
| Client memory | < 1.3GB | Support 2GB mobile devices |
| Network receive | < 50KB/s | Throttling starts at 40KB/s |
| Moving physics assemblies | 40-60 max | Each uses 0.4-0.9 KB/s |

Use **MicroProfiler** (Ctrl+F6) to identify bottlenecks:
- CPU >> GPU → Scripts or physics are the problem
- GPU >> CPU → Reduce triangles and drawcalls

### Memory Leak Prevention

Event connections hold strong references, preventing garbage collection:

```lua
-- WRONG: Connection leaks memory
part.Touched:Connect(function(hit)
    -- This connection lives forever, even if part is destroyed
end)

-- CORRECT: Track and disconnect
local connection
connection = part.Touched:Connect(function(hit)
    -- Handle touch
end)

-- Later, when done:
connection:Disconnect()

-- BEST: Use cleanup patterns (Maid/Trove)
local Trove = require(ReplicatedStorage.Packages.Trove)
local trove = Trove.new()

trove:Connect(part.Touched, function(hit)
    -- Handle touch
end)

-- Cleanup all connections at once
trove:Destroy()
```

### Network Optimization

Reduce bandwidth through data structure choices:

```lua
-- WRONG: String keys cost bytes
remoteEvent:FireClient(player, {
    Name = "Sword",      -- 4 bytes for "Name"
    Damage = 50,         -- 6 bytes for "Damage"
    Level = 5            -- 5 bytes for "Level"
})  -- ~15 extra bytes just for keys!

-- CORRECT: Arrays eliminate key overhead
remoteEvent:FireClient(player, {"Sword", 50, 5})

-- BEST: Instance references cost only 4 bytes
remoteEvent:FireClient(player, swordInstance)  -- 4 bytes total
```

For position data, use `Vector3int16` (6 bytes) instead of `Vector3` (12 bytes) when integer precision is acceptable.

### Security Validation

**Never trust the client.** Every RemoteEvent handler must validate:

```lua
-- Example: Secure purchase handler
local function onPurchaseRequest(player, itemId, quantity)
    -- Type checking
    if typeof(itemId) ~= "string" then return end
    if typeof(quantity) ~= "number" then return end

    -- NaN checking (NaN ~= NaN)
    if quantity ~= quantity then return end

    -- Sanity limits
    if quantity < 1 or quantity > 100 then return end
    if string.len(itemId) > 50 then return end

    -- Cooldown enforcement
    local lastPurchase = purchaseCooldowns[player]
    if lastPurchase and tick() - lastPurchase < 1 then return end
    purchaseCooldowns[player] = tick()

    -- Logical validation
    local item = Items[itemId]
    if not item then return end

    local cost = item.Price * quantity
    if getPlayerMoney(player) < cost then return end

    -- Only now process the purchase
    processPurchase(player, itemId, quantity)
end
```

**Prefer passive anti-cheat:** Teleport speed hackers back rather than kicking them—handles latency gracefully and avoids false positives.

### DataStore Best Practices

```lua
local DataStoreService = game:GetService("DataStoreService")
local playerStore = DataStoreService:GetDataStore("PlayerData")

-- Use UpdateAsync for atomic updates (prevents race conditions)
local success, result = pcall(function()
    return playerStore:UpdateAsync("player_" .. player.UserId, function(oldData)
        oldData = oldData or { coins = 0, level = 1 }
        oldData.coins = oldData.coins + coinsToAdd
        return oldData
    end)
end)

-- Always wrap in pcall with retry
local MAX_RETRIES = 3
for attempt = 1, MAX_RETRIES do
    local success, result = pcall(function()
        return playerStore:GetAsync(key)
    end)
    if success then
        return result
    end
    task.wait(attempt)  -- Exponential backoff
end

-- Save on server shutdown
game:BindToClose(function()
    for _, player in Players:GetPlayers() do
        savePlayerData(player)
    end
end)

-- Never save NaN (check with value == value)
if coins == coins then  -- false for NaN
    data.coins = coins
end
```

For production games, use **ProfileService** or **ProfileStore** for session locking—prevents data duplication when players switch servers quickly.

### Professional Toolchain

Beyond Rojo, the modern Roblox stack includes:

| Tool | Purpose | Install |
|------|---------|---------|
| **Wally** | Package manager | `rokit add wally` |
| **Selene** | Luau linter | `rokit add selene` |
| **StyLua** | Code formatter | `rokit add stylua` |
| **Luau LSP** | VS Code autocomplete | VS Code extension |
| **TestEZ** | Testing framework | `wally add testez` |

```bash
# Install toolchain manager
cargo install rokit

# Initialize project tools
rokit init

# Add tools
rokit add rojo selene stylua wally

# Lint code
selene src/

# Format code
stylua src/

# Run tests
rojo build -o test.rbxl && run-in-roblox --place test.rbxl --script tests/run.lua
```

### Recommended Libraries

| Library | Purpose | Wally |
|---------|---------|-------|
| **Knit** | Service/Controller framework | `sleitnick/knit` |
| **ProfileService** | Data persistence with session locking | `madstudioroblox/profileservice` |
| **React-lua** | Declarative UI framework | `jsdotlua/react` |
| **Trove** | Cleanup/connection management | `sleitnick/trove` |
| **Promise** | Async/await patterns | `evaera/promise` |

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
