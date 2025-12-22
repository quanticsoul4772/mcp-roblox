# Procedural City Generation Research
## Massive-Scale Building Generation for Roblox

**Research Date:** December 2025
**Objective:** Design a system capable of filling the entire Roblox map space with procedurally generated buildings

---

## Executive Summary

After extensive research into procedural generation algorithms, Roblox-specific constraints, and performance optimization techniques, I've synthesized an architecture for massive-scale city generation. The key insight is that **chunk-based generation with LOD (Level of Detail) and async processing** is essential for filling map space without crashing.

### Key Findings

| Aspect | Constraint/Recommendation |
|--------|--------------------------|
| **Part Count** | 20-35k parts for mobile, 50k max practical |
| **Per-Model Limit** | 500 parts max per model recommended |
| **Map Size** | Roblox supports up to ~100,000 studs |
| **StreamingEnabled** | Essential for large worlds |
| **Generation Speed** | ~5 chunk operations per frame for 40fps |

---

## Part 1: Core Algorithms

### 1.1 Road Network Generation

**Recommended: Tensor Field Approach** (Chen et al., 2008)

The tensor field method allows blending different road patterns smoothly:

```lua
-- Tensor fields for different city zones
local tensors = {
    grid = function(x, z) return Vector2.new(1, 0), Vector2.new(0, 1) end,
    radial = function(x, z, center)
        local dir = (Vector2.new(x, z) - center).Unit
        return dir, Vector2.new(-dir.Y, dir.X)
    end,
    organic = function(x, z, seed)
        local angle = math.noise(x/100, z/100, seed) * math.pi
        return Vector2.new(math.cos(angle), math.sin(angle))
    end
}
```

**Alternative: L-System Approach** (Parish & Müller, 2001)
- Uses priority queue for road segment placement
- `localConstraints()` adjusts geometry for conflicts
- `globalGoals()` determines branching based on population density

### 1.2 Building Placement

**Grid-Based Block Filling:**
1. Roads define city blocks (polygons)
2. Subdivide blocks into building lots
3. Place buildings respecting setbacks and spacing

**Separating Axis Theorem** for collision:
- Detect building-road and building-building overlaps
- Push buildings apart along minimum overlap axis
- Discard buildings that can't find valid placement

### 1.3 Building Generation

**Modular Component System:**
```lua
local BuildingComponents = {
    foundations = {"concrete_slab", "raised_platform", "stilts"},
    walls = {"brick", "glass", "concrete", "wood"},
    roofs = {"flat", "pitched", "dome", "complex"},
    floors = {min = 1, max = 20},
    features = {"balcony", "awning", "fire_escape", "antenna"}
}

function generateBuilding(lot, style, seed)
    local rng = Random.new(seed)
    local floors = rng:NextInteger(style.minFloors, style.maxFloors)
    local width = lot.width - style.setback * 2
    local depth = lot.depth - style.setback * 2

    -- Generate floor by floor with variations
    for floor = 1, floors do
        generateFloor(building, floor, width, depth, style, rng)
    end
end
```

**Wave Function Collapse** for interior/facade details:
- Define tiles with connector rules
- Collapse lowest-entropy cell first
- Propagate constraints to neighbors
- ~100 manually-created blocks needed for good variety

---

## Part 2: Chunk System Architecture

### 2.1 Core Chunk Design

```lua
local ChunkSystem = {
    CHUNK_SIZE = 256,        -- studs per chunk
    RENDER_DISTANCE = 4,     -- chunks in each direction
    UNLOAD_DISTANCE = 6,     -- when to destroy chunks
    GENERATION_QUEUE = {},   -- async processing queue
}

-- Chunk coordinate from world position
function ChunkSystem:getChunkCoord(position)
    return Vector2.new(
        math.floor(position.X / self.CHUNK_SIZE),
        math.floor(position.Z / self.CHUNK_SIZE)
    )
end

-- Chunk storage using dictionary (infinite world support)
local loadedChunks = {} -- key: "x,z" format

function ChunkSystem:getChunkKey(cx, cz)
    return cx .. "," .. cz
end
```

### 2.2 Layer-Based Generation Pipeline

Inspired by the InfiniteWorld architecture:

```lua
local GenerationLayers = {
    -- Layer 1: Terrain heightmap
    {
        name = "terrain",
        type = "data",
        generate = function(chunk, seed)
            local heights = {}
            for x = 0, CHUNK_SIZE do
                heights[x] = {}
                for z = 0, CHUNK_SIZE do
                    local worldX = chunk.origin.X + x
                    local worldZ = chunk.origin.Z + z
                    heights[x][z] = generateHeight(worldX, worldZ, seed)
                end
            end
            return heights
        end
    },

    -- Layer 2: Road network
    {
        name = "roads",
        type = "data",
        depends = {"terrain"},
        generate = function(chunk, seed, terrain)
            return generateRoadsInChunk(chunk, terrain, seed)
        end
    },

    -- Layer 3: Building lots
    {
        name = "lots",
        type = "data",
        depends = {"roads"},
        generate = function(chunk, seed, roads)
            return subdivideCityBlocks(roads.blocks, seed)
        end
    },

    -- Layer 4: Building geometry (LOD-aware)
    {
        name = "buildings",
        type = "geometry",
        depends = {"lots"},
        generate = function(chunk, seed, lots, lodLevel)
            return generateBuildings(lots, seed, lodLevel)
        end
    }
}
```

### 2.3 LOD (Level of Detail) System

```lua
local LOD_LEVELS = {
    [1] = {  -- Closest: full detail
        maxDistance = 128,
        partLimit = 500,
        features = {"windows", "doors", "trim", "roofDetails"}
    },
    [2] = {  -- Medium: simplified
        maxDistance = 384,
        partLimit = 50,
        features = {"windows", "doors"}
    },
    [3] = {  -- Far: basic shape
        maxDistance = 768,
        partLimit = 10,
        features = {}
    },
    [4] = {  -- Distant: single merged mesh
        maxDistance = math.huge,
        partLimit = 1,
        features = {}
    }
}

function getBuildingLOD(building, playerPosition)
    local distance = (building.position - playerPosition).Magnitude
    for level, config in ipairs(LOD_LEVELS) do
        if distance <= config.maxDistance then
            return level, config
        end
    end
end
```

---

## Part 3: Performance Optimization

### 3.1 Async Generation with Frame Budget

```lua
local FrameBudget = {
    maxTimePerFrame = 0.005, -- 5ms budget (target 60fps = 16.6ms total)
    startTime = 0,
}

function FrameBudget:start()
    self.startTime = os.clock()
end

function FrameBudget:hasTimeRemaining()
    return (os.clock() - self.startTime) < self.maxTimePerFrame
end

-- Main generation loop
RunService.Heartbeat:Connect(function()
    FrameBudget:start()

    while #generationQueue > 0 and FrameBudget:hasTimeRemaining() do
        local task = table.remove(generationQueue, 1)
        processGenerationTask(task)
    end
end)
```

### 3.2 Coroutine-Based Generation

```lua
function generateChunkAsync(chunk)
    return coroutine.create(function()
        -- Layer 1: Terrain (yield periodically)
        for x = 0, CHUNK_SIZE, 16 do
            generateTerrainStrip(chunk, x, x + 16)
            coroutine.yield() -- Give control back
        end

        -- Layer 2: Roads
        generateRoads(chunk)
        coroutine.yield()

        -- Layer 3: Buildings (one at a time)
        for _, lot in ipairs(chunk.lots) do
            generateBuilding(lot)
            coroutine.yield()
        end
    end)
end

-- Resume coroutines within frame budget
function processGenerationTask(task)
    if coroutine.status(task.coroutine) ~= "dead" then
        local success, err = coroutine.resume(task.coroutine)
        if not success then
            warn("Generation error:", err)
        end
        if coroutine.status(task.coroutine) ~= "dead" then
            table.insert(generationQueue, task) -- Re-queue
        end
    end
end
```

### 3.3 StreamingEnabled Configuration

```lua
-- Workspace settings for massive worlds
workspace.StreamingEnabled = true
workspace.StreamingMinRadius = 256
workspace.StreamingTargetRadius = 512
workspace.StreamingIntegrityMode = Enum.StreamingIntegrityMode.MinimumRadiusPause

-- For building models: use Default (non-atomic) for best performance
-- Atomic causes lag spikes with large part counts
building.ModelStreamingMode = Enum.ModelStreamingMode.Default
```

### 3.4 Part Optimization Strategies

| Strategy | When to Use | Performance Gain |
|----------|-------------|------------------|
| **MeshParts over Parts** | Complex shapes | 3-5x fewer draw calls |
| **Merge distant buildings** | LOD level 4 | 10-50x part reduction |
| **Texture atlasing** | Many materials | Fewer material swaps |
| **Disable CanCollide** | Decorative parts | Physics savings |
| **RenderFidelity.Performance** | Distant objects | GPU savings |

---

## Part 4: Recommended Architecture

### 4.1 System Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    CityGenerator (Main)                      │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ ChunkManager│  │ LODManager  │  │ GenerationScheduler │ │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘ │
│         │                │                     │            │
│  ┌──────┴──────┐  ┌──────┴──────┐  ┌──────────┴──────────┐ │
│  │ChunkLoader  │  │LODUpdater   │  │CoroutineRunner      │ │
│  │ChunkUnloader│  │MeshMerger   │  │FrameBudgetManager   │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                    Generation Pipeline                       │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────────────┐│
│  │ Terrain │→ │  Roads  │→ │  Lots   │→ │    Buildings    ││
│  │ Layer   │  │  Layer  │  │  Layer  │  │     Layer       ││
│  └─────────┘  └─────────┘  └─────────┘  └─────────────────┘│
├─────────────────────────────────────────────────────────────┤
│                    Building Components                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ WallGen     │  │ RoofGen     │  │ FurnitureGen        │ │
│  │ FloorGen    │  │ WindowGen   │  │ ExteriorDetailGen   │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 Seed-Based Determinism

Critical for infinite worlds - same seed = same city:

```lua
function CityGenerator:new(masterSeed)
    local rng = Random.new(masterSeed)

    return {
        masterSeed = masterSeed,
        terrainSeed = rng:NextInteger(1, 2^31),
        roadSeed = rng:NextInteger(1, 2^31),
        buildingSeed = rng:NextInteger(1, 2^31),

        getChunkSeed = function(self, chunkX, chunkZ)
            -- Deterministic seed for each chunk
            return self.masterSeed * 31 + chunkX * 17 + chunkZ * 13
        end
    }
end
```

---

## Part 5: Implementation Roadmap

### Phase 1: Foundation (Week 1-2)
1. ✅ Basic ChunkSystem with load/unload
2. ✅ Perlin noise terrain generation
3. ✅ Frame-budgeted async processing
4. ✅ StreamingEnabled configuration

### Phase 2: Road Network (Week 2-3)
1. Grid-based road generation
2. Intersection handling
3. Road-terrain integration
4. Block subdivision for lots

### Phase 3: Building Generation (Week 3-4)
1. Modular building components
2. Style presets (residential, commercial, industrial)
3. Height variation by zone
4. Basic LOD (full → box)

### Phase 4: Optimization (Week 4-5)
1. Mesh merging for distant buildings
2. Texture atlasing
3. Advanced LOD transitions
4. Memory profiling and limits

### Phase 5: Polish (Week 5-6)
1. Variety improvements (more building types)
2. Street furniture (lights, signs, trees)
3. Special landmarks
4. Edge handling at map boundaries

---

## Part 6: Starting Code Template

```lua
-- ServerScriptService/CityGenerator/init.lua
local CityGenerator = {}
CityGenerator.__index = CityGenerator

-- Configuration
local CONFIG = {
    CHUNK_SIZE = 256,
    RENDER_DISTANCE = 3,
    UNLOAD_DISTANCE = 5,
    FRAME_BUDGET_MS = 5,
    MAX_PARTS_PER_CHUNK = 2000,

    -- Building settings
    MIN_BUILDING_HEIGHT = 1,
    MAX_BUILDING_HEIGHT = 20,
    BUILDING_FLOOR_HEIGHT = 4,

    -- Road settings
    MAIN_ROAD_WIDTH = 16,
    SIDE_ROAD_WIDTH = 8,
    BLOCK_SIZE = 64,
}

function CityGenerator.new(seed)
    local self = setmetatable({}, CityGenerator)

    self.seed = seed or os.time()
    self.rng = Random.new(self.seed)
    self.chunks = {}
    self.generationQueue = {}
    self.activeCoroutines = {}

    return self
end

function CityGenerator:start()
    -- Connect to player movement for chunk loading
    game.Players.PlayerAdded:Connect(function(player)
        self:trackPlayer(player)
    end)

    -- Main generation loop
    game:GetService("RunService").Heartbeat:Connect(function(dt)
        self:processGenerationQueue(dt)
    end)

    print("[CityGenerator] Started with seed:", self.seed)
end

-- See full implementation in CityGenerator module

return CityGenerator
```

---

## Sources

### Roblox Developer Forum
- [Procedural City Generation 2.7](https://devforum.roblox.com/t/procedural-city-generation-27/3383522)
- [Procedural City Generator](https://devforum.roblox.com/t/procedural-city-generator/3008305)
- [Lagless Infinite Terrain Generator](https://devforum.roblox.com/t/lagless-infinite-terrain-generator/962044)
- [Ultimate Perlin Noise Guide](https://devforum.roblox.com/t/ultimate-perlin-noise-and-how-to-make-procedural-terrain-guide-24231-characters-detailed/3109400)
- [Dungeon Generation Guide](https://devforum.roblox.com/t/dungeon-generation-a-procedural-generation-guide/342413)
- [StreamingEnabled Optimization](https://devforum.roblox.com/t/is-streamingenabled-good-best-ways-for-optimization/625977)
- [Part Count Limits](https://devforum.roblox.com/t/whats-a-good-maximum-part-count-for-low-end-devices/1930430)
- [MeshPart Optimization](https://devforum.roblox.com/t/meshpart-usage-performance-optimizations/1319217)

### Academic/Technical
- [Infinite WFC City](https://marian42.de/article/wfc/) - Wave Function Collapse implementation
- [Procedural City Generation](https://www.tmwhere.com/city_generation.html) - Tensor field approach
- [Parish & Müller 2001](https://www.citygen.net/) - Original city generation paper
- [InfiniteWorld GitHub](https://github.com/ToberoCat/InfiniteWorld) - Layer-based chunk system

### Performance
- [Unity Coroutines & Async](https://blog.logrocket.com/performance-unity-async-await-tasks-coroutines-c-job-system-burst-compiler/)
- [LOD in Computer Graphics](https://en.wikipedia.org/wiki/Level_of_detail_(computer_graphics))
