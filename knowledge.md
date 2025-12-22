# Project knowledge

Rust MCP server for Roblox Studio integration. Provides 27 MCP tools for filesystem operations, live Studio manipulation, and Open Cloud API access.

## Quickstart
- Setup: `cargo build`
- Dev: `cargo run` (uses STDIO transport)
- Test: `cargo test` (499 unit tests, 86.7% coverage)
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

## Luau/Roblox Gotchas (From City Generator Debugging)

### `pairs()` Iteration Order is UNDEFINED
```lua
-- WRONG: pairs() gives random order - will break layer dependencies
for layerName, generator in pairs(self.generationLayers) do
    generator(chunk)  -- terrain, road, lot, building may run in any order!
end

-- CORRECT: Use ordered iteration with ipairs()
local LAYER_ORDER = {"terrain", "road", "lot", "building"}
for _, layerName in ipairs(LAYER_ORDER) do
    self.generationLayers[layerName](chunk)  -- guaranteed order
end
```

### ModuleScript Require Paths
- `require(script.Parent.SiblingModule)` - sibling in same folder
- `require(script.Parent.Subfolder.Module)` - nested child
- `require(script.Parent.Parent.OtherFolder.Module)` - navigate up then down
- File names must match EXACTLY (case-sensitive): `RoadLayer.lua` ≠ `RoadsLayer`

### Layer Data Key Consistency
```lua
-- Layer writes: chunk.data.road = {...}
-- Consumer reads: chunk.data.road (NOT chunk.data.roads)
-- Key mismatch = silent nil, hard to debug
```

### Chunk-Based Procedural Generation Pattern
```lua
-- Deterministic seeding for reproducible worlds
local chunkSeed = masterSeed * 31 + cx * 17 + cz * 13

-- Frame-budget generation (2ms for mobile)
while budgetRemaining() do
    coroutine.resume(generatorCoroutine)
end

-- Layer dependencies: terrain → road → lot → building → prop
```

### Part Budget Optimization
```lua
-- LOD 1 buildings optimized to ~45 parts (was 150+)
-- Windows: 1 part each (simplified from 3-part glass+frame)
-- Windows only on front/back walls, max 12 per wall
-- Props budget: 50 parts per chunk
```

## City Generator Reference
- Design doc: `claudedocs/design_procedural_city_mobile.md`
- Research: `claudedocs/research_procedural_city_generation.md`
- Implementation notes: `claudedocs/implementation_notes_city_generator.md`
- Implementation: `ServerScriptService/CityGenerator/` (init, Config, Core/, Layers/, Building/, Utils/)

### Known Issues
- Terrain height gets extreme at far distances (perlin noise edge behavior)
