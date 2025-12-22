# Mobile-Friendly Procedural City Generation
## Implementation Design Document

**Target:** Fill entire Roblox map with procedurally generated buildings
**Constraint:** Mobile-friendly (30+ fps on phones/tablets)

---

## Design Constraints (Mobile)

| Metric | Mobile Limit | Our Target |
|--------|--------------|------------|
| **Total parts loaded** | 20-35k max | 15k active |
| **Parts per chunk** | ~500 | 300 |
| **Render distance** | 512 studs | 384 studs |
| **Memory** | ~1GB limit | 800MB max |
| **Frame time** | 33ms (30fps) | 25ms target |
| **Generation budget** | 3ms/frame | 2ms/frame |

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                      CityGenerator (Entry Point)                     │
│  - Initializes all subsystems                                        │
│  - Manages master seed for determinism                               │
│  - Coordinates player tracking                                       │
└─────────────────────────────────────────────────────────────────────┘
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        ▼                           ▼                           ▼
┌───────────────┐          ┌───────────────┐          ┌───────────────┐
│  ChunkManager │          │  LODManager   │          │ GenScheduler  │
│               │          │               │          │               │
│ - Load chunks │◄────────►│ - Track dist  │◄────────►│ - Frame budget│
│ - Unload far  │          │ - Swap LODs   │          │ - Coroutines  │
│ - Priority Q  │          │ - Merge meshes│          │ - Task queue  │
└───────────────┘          └───────────────┘          └───────────────┘
        │                           │                           │
        └───────────────────────────┼───────────────────────────┘
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        Generation Pipeline                           │
├─────────────┬─────────────┬─────────────┬─────────────┬─────────────┤
│   Terrain   │    Roads    │    Lots     │  Buildings  │   Details   │
│   Layer     │    Layer    │    Layer    │    Layer    │    Layer    │
│             │             │             │             │             │
│ Perlin grid │ Grid-based  │ Block subdiv│ Modular gen │ Street props│
│ Height map  │ Intersects  │ Lot sizing  │ LOD-aware   │ LOD 1 only  │
└─────────────┴─────────────┴─────────────┴─────────────┴─────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                       Building Factory                               │
├─────────────────┬─────────────────┬─────────────────────────────────┤
│  Component Pool │  Style Presets  │  LOD Generators                 │
│                 │                 │                                 │
│ - Reusable parts│ - Residential   │ - LOD1: Full (100 parts max)   │
│ - Pre-made walls│ - Commercial    │ - LOD2: Simple (20 parts)      │
│ - Roof pieces   │ - Industrial    │ - LOD3: Box (4 parts)          │
│ - Window sets   │ - Landmark      │ - LOD4: Billboard (1 part)     │
└─────────────────┴─────────────────┴─────────────────────────────────┘
```

---

## Module Specifications

### Module 1: ChunkManager

**Purpose:** Manage spatial partitioning and chunk lifecycle

```lua
-- Configuration
ChunkManager.CONFIG = {
    CHUNK_SIZE = 128,           -- studs (smaller for mobile)
    LOAD_RADIUS = 3,            -- chunks (384 studs)
    UNLOAD_RADIUS = 5,          -- chunks (640 studs)
    MAX_CHUNKS_LOADED = 50,     -- hard limit
    LOAD_PRIORITY_INTERVAL = 0.5, -- seconds between priority updates
}

-- Public API
ChunkManager:initialize(seed: number)
ChunkManager:update(playerPositions: {Vector3})
ChunkManager:getChunk(cx: number, cz: number) -> Chunk?
ChunkManager:requestLoad(cx: number, cz: number, priority: number)
ChunkManager:forceUnload(cx: number, cz: number)

-- Chunk structure
Chunk = {
    cx: number,                 -- chunk X coordinate
    cz: number,                 -- chunk Z coordinate
    state: "loading" | "ready" | "unloading",
    model: Model?,              -- workspace model when ready
    partCount: number,          -- for budget tracking
    lastAccess: number,         -- for LRU unloading
    lod: number,                -- current LOD level
    data: {                     -- generated data (kept even when unloaded)
        terrain: HeightMap,
        roads: RoadNetwork,
        lots: {Lot},
        buildings: {BuildingData},
    }
}
```

### Module 2: LODManager

**Purpose:** Manage level-of-detail transitions for performance

```lua
-- Configuration (Mobile-optimized)
LODManager.LEVELS = {
    [1] = { -- FULL DETAIL
        maxDistance = 64,
        maxPartsPerBuilding = 100,
        features = {"walls", "roof", "windows", "doors", "trim"},
        updateRate = 0, -- always current
    },
    [2] = { -- REDUCED
        maxDistance = 128,
        maxPartsPerBuilding = 20,
        features = {"walls", "roof", "windows"},
        updateRate = 0.5, -- check every 0.5s
    },
    [3] = { -- MINIMAL
        maxDistance = 256,
        maxPartsPerBuilding = 4,
        features = {"walls", "roof"},
        updateRate = 1.0,
    },
    [4] = { -- BILLBOARD
        maxDistance = math.huge,
        maxPartsPerBuilding = 1,
        features = {},  -- single merged mesh or billboard
        updateRate = 2.0,
    },
}

-- Public API
LODManager:initialize()
LODManager:update(playerPosition: Vector3)
LODManager:getLODLevel(position: Vector3) -> number
LODManager:transitionBuilding(building: Model, fromLOD: number, toLOD: number)
LODManager:getPartBudget(lod: number) -> number
```

### Module 3: GenerationScheduler

**Purpose:** Async generation with strict frame budget

```lua
-- Configuration
GenScheduler.CONFIG = {
    FRAME_BUDGET_MS = 2,        -- 2ms for mobile (leaves room for rendering)
    MAX_TASKS_PER_FRAME = 3,    -- limit task switches
    PRIORITY_LEVELS = 4,        -- 0=urgent, 3=background
}

-- Task structure
Task = {
    id: string,
    priority: number,
    coroutine: thread,
    chunkKey: string,
    layer: string,              -- "terrain", "roads", "lots", "buildings"
    estimatedCost: number,      -- ms estimate
}

-- Public API
GenScheduler:initialize()
GenScheduler:schedule(task: Task)
GenScheduler:cancel(taskId: string)
GenScheduler:cancelChunk(chunkKey: string)
GenScheduler:update() -- called every frame
GenScheduler:getQueueSize() -> number
GenScheduler:isPaused() -> boolean
GenScheduler:pause() / :resume()
```

### Module 4: BuildingFactory

**Purpose:** Generate buildings with LOD variants

```lua
-- Building style definitions
BuildingFactory.STYLES = {
    residential_small = {
        minFloors = 1, maxFloors = 3,
        minWidth = 8, maxWidth = 16,
        minDepth = 8, maxDepth = 16,
        roofTypes = {"flat", "pitched", "gabled"},
        colors = {primary = "warm", accent = "neutral"},
        windowDensity = 0.6,
    },
    residential_medium = {
        minFloors = 3, maxFloors = 8,
        minWidth = 12, maxWidth = 24,
        -- ...
    },
    commercial = {
        minFloors = 1, maxFloors = 15,
        minWidth = 16, maxWidth = 40,
        roofTypes = {"flat"},
        colors = {primary = "neutral", accent = "brand"},
        windowDensity = 0.8,
    },
    industrial = {
        minFloors = 1, maxFloors = 2,
        minWidth = 20, maxWidth = 60,
        roofTypes = {"flat", "sawtooth"},
        colors = {primary = "gray", accent = "warning"},
        windowDensity = 0.2,
    },
}

-- Public API
BuildingFactory:initialize(componentPool: Folder)
BuildingFactory:generate(lot: Lot, style: string, seed: number, lod: number) -> Model
BuildingFactory:generateLODVariant(buildingData: BuildingData, targetLOD: number) -> Model
BuildingFactory:estimatePartCount(lot: Lot, style: string, lod: number) -> number
```

### Module 5: RoadGenerator

**Purpose:** Create road network with intersections

```lua
-- Configuration
RoadGenerator.CONFIG = {
    MAIN_ROAD_WIDTH = 12,
    SIDE_ROAD_WIDTH = 8,
    BLOCK_SIZE_MIN = 32,
    BLOCK_SIZE_MAX = 64,
    INTERSECTION_SIZE = 12,
}

-- Road structure
Road = {
    start: Vector2,
    finish: Vector2,
    width: number,
    type: "main" | "side" | "alley",
}

Intersection = {
    position: Vector2,
    roads: {Road},
    type: "4way" | "3way" | "corner",
}

Block = {
    vertices: {Vector2},        -- polygon vertices
    area: number,
    lots: {Lot}?,               -- filled by LotGenerator
}

-- Public API
RoadGenerator:generate(chunk: Chunk, seed: number) -> RoadNetwork
RoadGenerator:getBlocksInChunk(roadNetwork: RoadNetwork) -> {Block}
```

---

## Implementation Phases

### Phase 1: Core Infrastructure (Priority: Critical)

**Goal:** Chunk system + async scheduler working

```
Duration: 3-4 hours
Files to create:
├── ServerScriptService/
│   └── CityGenerator/
│       ├── init.lua              -- Main entry point
│       ├── ChunkManager.lua      -- Chunk lifecycle
│       ├── GenScheduler.lua      -- Async task runner
│       └── Config.lua            -- All configuration
```

**Deliverables:**
- [ ] ChunkManager loads/unloads based on player position
- [ ] GenScheduler runs tasks within frame budget
- [ ] Empty chunks appear/disappear correctly
- [ ] Works with multiple players

**Test:** Walk around, verify chunks load ahead and unload behind

---

### Phase 2: Terrain & Roads (Priority: High)

**Goal:** Grid-based roads with blocks

```
Duration: 2-3 hours
Files to create:
├── CityGenerator/
│   ├── Layers/
│   │   ├── TerrainLayer.lua      -- Height map generation
│   │   └── RoadLayer.lua         -- Road network
│   └── Utils/
│       └── Noise.lua             -- Perlin noise wrapper
```

**Deliverables:**
- [ ] Perlin noise terrain (flat for city, hills at edges)
- [ ] Grid roads spawn per chunk
- [ ] Roads connect across chunk boundaries
- [ ] Intersections form correctly

**Test:** Roads form continuous network, no gaps at chunk edges

---

### Phase 3: Lots & Basic Buildings (Priority: High)

**Goal:** Buildings spawn on lots

```
Duration: 3-4 hours
Files to create:
├── CityGenerator/
│   ├── Layers/
│   │   ├── LotLayer.lua          -- Block subdivision
│   │   └── BuildingLayer.lua     -- Building placement
│   └── Building/
│       ├── Factory.lua           -- Building generator
│       └── Styles.lua            -- Style definitions
```

**Deliverables:**
- [ ] Blocks subdivide into lots
- [ ] Simple box buildings spawn (LOD3)
- [ ] Building heights vary by noise
- [ ] Part count stays under budget

**Test:** City fills with basic buildings, mobile fps > 30

---

### Phase 4: LOD System (Priority: High)

**Goal:** Buildings swap detail based on distance

```
Duration: 2-3 hours
Files to create:
├── CityGenerator/
│   ├── LODManager.lua            -- Distance tracking, LOD swaps
│   └── Building/
│       └── LODVariants.lua       -- Multi-LOD generation
```

**Deliverables:**
- [ ] Buildings start at LOD3/4
- [ ] Approaching triggers LOD upgrade
- [ ] Leaving triggers LOD downgrade
- [ ] Smooth transitions (no pop-in)

**Test:** Walk toward building, watch detail increase

---

### Phase 5: Building Details (Priority: Medium)

**Goal:** Full-detail buildings with variety

```
Duration: 4-5 hours
Files to create:
├── CityGenerator/
│   └── Building/
│       ├── Components/
│       │   ├── Walls.lua
│       │   ├── Roofs.lua
│       │   ├── Windows.lua
│       │   └── Doors.lua
│       └── Interiors.lua         -- Optional interior shells
```

**Deliverables:**
- [ ] LOD1 buildings have windows, doors, trim
- [ ] Multiple wall/roof styles
- [ ] Color variation per building
- [ ] Residential vs commercial look different

**Test:** Close-up buildings look detailed and varied

---

### Phase 6: Street Details (Priority: Low)

**Goal:** Props and atmosphere

```
Duration: 2-3 hours
Files to create:
├── CityGenerator/
│   └── Layers/
│       └── PropLayer.lua         -- Street furniture
```

**Deliverables:**
- [ ] Street lights at intersections
- [ ] Trees along roads (LOD1 only)
- [ ] Sidewalks
- [ ] Optional: cars, signs

**Test:** Streets feel alive at close range

---

## Mobile Optimization Checklist

### Part Reduction
- [x] LOD system with 4 levels
- [ ] Merge distant buildings into single mesh
- [ ] Use MeshParts over Unions
- [ ] Component reuse (clone, don't create)

### Memory Management
- [ ] Unload chunks aggressively (5 chunk radius)
- [ ] Clear chunk data after unload (optional, breaks re-visit)
- [ ] Limit concurrent generation tasks
- [ ] Pool building components

### Rendering
- [ ] RenderFidelity.Performance for LOD3+
- [ ] CastShadow = false for LOD2+
- [ ] CanCollide = false for decorative parts
- [ ] Disable CanQuery for non-interactive

### Streaming
- [ ] Enable workspace.StreamingEnabled
- [ ] Set StreamingMinRadius = 256
- [ ] Set StreamingTargetRadius = 384
- [ ] Use ModelStreamingMode.Default (not Atomic)

---

## File Structure (Final)

```
ServerScriptService/
└── CityGenerator/
    ├── init.lua                  -- Entry point, public API
    ├── Config.lua                -- All configuration constants
    │
    ├── Core/
    │   ├── ChunkManager.lua      -- Chunk lifecycle
    │   ├── LODManager.lua        -- Level of detail
    │   └── GenScheduler.lua      -- Async task runner
    │
    ├── Layers/
    │   ├── TerrainLayer.lua      -- Height map
    │   ├── RoadLayer.lua         -- Road network
    │   ├── LotLayer.lua          -- Block subdivision
    │   ├── BuildingLayer.lua     -- Building placement
    │   └── PropLayer.lua         -- Street details
    │
    ├── Building/
    │   ├── Factory.lua           -- Main generator
    │   ├── Styles.lua            -- Style definitions
    │   ├── LODVariants.lua       -- Multi-LOD support
    │   └── Components/
    │       ├── Walls.lua
    │       ├── Roofs.lua
    │       ├── Windows.lua
    │       └── Doors.lua
    │
    └── Utils/
        ├── Noise.lua             -- Perlin noise
        ├── Random.lua            -- Seeded random
        └── Geometry.lua          -- Vector math helpers

ReplicatedStorage/
└── CityAssets/
    ├── Components/               -- Pre-made building parts
    │   ├── Walls/
    │   ├── Roofs/
    │   └── Props/
    └── Materials/                -- Shared materials
```

---

## Quick Start After Design Approval

```lua
-- To begin implementation, I will create:

-- 1. CityGenerator/init.lua (entry point)
-- 2. CityGenerator/Config.lua (all constants)
-- 3. CityGenerator/Core/ChunkManager.lua
-- 4. CityGenerator/Core/GenScheduler.lua

-- Then test with:
local CityGenerator = require(game.ServerScriptService.CityGenerator)
CityGenerator:initialize({
    seed = 12345,
    mobileMode = true,  -- Enables aggressive optimization
})
CityGenerator:start()
```

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| **Part count explosion** | Hard cap per chunk (300), LOD enforcement |
| **Chunk boundary gaps** | Overlap generation by 1 unit, shared road network |
| **Slow generation** | Priority queue, urgent = nearby, background = far |
| **Memory leaks** | Explicit Destroy() on unload, connection cleanup |
| **Mobile lag spikes** | 2ms budget, yield frequently, batch operations |
