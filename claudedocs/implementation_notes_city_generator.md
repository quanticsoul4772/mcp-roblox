# City Generator Implementation Notes

Implementation learnings from debugging the procedural city generator.

## Bugs Fixed

### 1. Layer Execution Order (Root Cause of Missing Buildings)

The building layer was running before road/lot layers due to Lua's undefined `pairs()` iteration order.

Location: `CityGenerator/Core/ChunkManager.lua`

Before:
```lua
for layerName, generator in pairs(self.generationLayers) do
    chunk.data[layerName] = generator(chunk, self)
end
```

After:
```lua
ChunkManager.LAYER_ORDER = {"terrain", "road", "lot", "building", "prop"}

for _, layerName in ipairs(ChunkManager.LAYER_ORDER) do
    local generator = self.generationLayers[layerName]
    if generator then
        chunk.data[layerName] = generator(chunk, self)
        coroutine.yield()
    end
end
```

### 2. Layer Name Mismatch

Init module referenced plural names ("Roads") but files used singular ("Road").

Location: `CityGenerator/init.lua`

Before:
```lua
local layerOrder = {"Terrain", "Roads", "Lots", "Buildings", "Props"}
```

After:
```lua
local layerOrder = {"Terrain", "Road", "Lot", "Building", "Prop"}
```

### 3. Data Key Mismatch

LotLayer read `chunk.data.roads` but RoadLayer wrote to `chunk.data.road`.

Location: `CityGenerator/Layers/LotLayer.lua`

Before:
```lua
local roadData = chunk.data.roads or {blocks = {}}
```

After:
```lua
local roadData = chunk.data.road or {blocks = {}}
```

Same fix in `BuildingLayer.lua`:
```lua
local lotData = chunk.data.lot or {lots = {}, zone = "suburban", groundY = 0}
```

### 4. PropLayer Road Data Structure Mismatch

PropLayer tried to use `road.startX/endX` but RoadLayer uses `type`/`worldX`/`worldZ`.

Location: `CityGenerator/Layers/PropLayer.lua`

Before:
```lua
local roadLength = road.endX - road.startX  -- nil arithmetic error
```

After:
```lua
if road.type == "vertical" then
    local roadX = road.worldX  -- correct field
```

### 5. Props Distance Check Using Wrong Origin

PropLayer checked distance from world origin (0,0,0) instead of player position.

Location: `CityGenerator/Layers/PropLayer.lua`

Fix: Removed distance check entirely - ChunkManager only loads chunks near players anyway.

### 6. LOD 1 Buildings Exceeding Part Budget

LOD 1 buildings were using ~150+ parts, quickly filling 300-part chunk budget.

Fixes applied:
- Windows: Reduced from 3 parts (glass + 2 frames) to 1 part
- Windows: Added MAX_WINDOWS_PER_WALL = 12 cap
- Windows: Only on front/back walls, not all 4 sides
- Sawtooth roofs: Limited to 3 teeth max

Result: LOD 1 buildings now ~45 parts (was 150+)

### 7. Street Lights in Intersection Center

Lights placed at intersection center instead of corner (sidewalk).

Fix: Added `INTERSECTION_OFFSET` and calculate corner position with road width offset.

## Debug Features

The Config module includes debug toggles that were disabled for production:

```lua
Config.Debug = {
    ENABLED = true,
    VERBOSE = false,             -- reduces log spam
    SHOW_CHUNK_BORDERS = false,  -- red neon borders around chunks
    SHOW_LOD_COLORS = false,
    LOG_GENERATION = false,
    LOG_PERFORMANCE = true,
}
```

The red grid lines visible during development were chunk border debug visualizations.

## Architecture Summary

```
CityGenerator/
  init.lua              -- entry point, coordinates layers
  Config.lua            -- all configuration constants
  Core/
    ChunkManager.lua    -- chunk lifecycle, load/unload
    GenScheduler.lua    -- frame-budget coroutine runner
  Layers/
    TerrainLayer.lua    -- perlin noise heightmap
    RoadLayer.lua       -- grid roads, intersections, sidewalks
    LotLayer.lua        -- subdivide blocks into building lots
    BuildingLayer.lua   -- procedural buildings with LOD 1-4
    PropLayer.lua       -- street furniture (lights, trees)
  Building/
    Components/
      Windows.lua       -- window generation (optimized 1-part)
      Doors.lua         -- door and entrance generation
      Trim.lua          -- cornices, ledges, base trim
  Utils/
    Noise.lua           -- perlin noise wrapper
```

## Key Patterns Used

1. Chunk-based generation with 128-stud chunks
2. Layer dependency chain: terrain -> road -> lot -> building -> prop
3. Frame budget of 2ms for mobile performance
4. Deterministic seeding: `seed * 31 + cx * 17 + cz * 13`
5. Zone-based building heights (downtown=tall, suburban=short)
6. Coroutine-based async generation with yields
7. Part budget optimization: LOD 1 ~45 parts, props 50 parts/chunk

## Known Issues

1. **Terrain height extreme at edges**: Perlin noise creates exaggerated hills far from center
   - Potential fix: Clamp terrain height or reduce noise amplitude at distance
