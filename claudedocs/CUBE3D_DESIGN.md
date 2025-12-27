# Cube 3D Integration Design (GenerationService Approach)

## Executive Summary

This document outlines the architecture for integrating Roblox's **GenerationService** into the MCP Roblox server, enabling text-to-3D mesh generation without requiring a GPU.

**Key Constraint**: Local inference requires 16-24GB VRAM. This design uses Roblox's cloud-based GenerationService instead.

---

## GenerationService Overview

### What It Does
- Generates 3D meshes from text prompts using Roblox's Cube 3D model
- Runs on **Roblox's servers** (no local GPU needed)
- Available in Studio and published experiences

### Rate Limits
- **5 generations per minute** per experience
- Prompt character limit applies
- Moderation filtering on prompts

### API Architecture (Client-Server Split)

| Method | Context | Purpose |
|--------|---------|---------|
| `GenerateMeshAsync()` | Server-side | Initiates generation, returns generationId |
| `LoadGeneratedMeshAsync()` | Client-side | Loads the mesh into the experience |

**Challenge**: The mesh only exists on the client that loaded it - NOT replicated.

---

## Integration Strategy

### The Problem

Our Studio bridge (plugin) runs in a special context. The GenerationService requires:
1. Server-side call to `GenerateMeshAsync()` → returns `generationId`
2. Client-side call to `LoadGeneratedMeshAsync(generationId)` → returns `MeshPart`

In Studio **Edit Mode** (not Play Mode), there's no client/server distinction, so our plugin may be able to call both.

### Proposed Solution: Plugin-Based Generation

```
┌─────────────────────────────────────────────────────────────────┐
│                     MCP Client (Claude)                         │
│  "Generate a medieval torch bracket and place it in Workspace"  │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                    MCP Roblox Server                            │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ studio_generate_mesh(prompt, suggested_size?, parent?)  │   │
│  └────────────────────────────┬────────────────────────────┘   │
└───────────────────────────────┼─────────────────────────────────┘
                                │ HTTP POST /execute
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Studio Plugin (MCPServer)                    │
│                                                                  │
│  1. Call GenerationService:GenerateMeshAsync()                  │
│     → Returns generationId                                       │
│                                                                  │
│  2. Call GenerationService:LoadGeneratedMeshAsync(generationId) │
│     → Returns MeshPart with EditableMesh                        │
│                                                                  │
│  3. Parent MeshPart to specified location                       │
│     → mesh.Parent = game.Workspace                              │
│                                                                  │
│  4. Return success with instance path                           │
└─────────────────────────────────────────────────────────────────┘
```

---

## Implementation Plan

### Phase 1: Plugin Command Handler (Week 1)

#### 1.1 Add GenerateMesh Command to Plugin

```lua
-- plugin/MCPServer.server.luau (addition)

local GenerationService = game:GetService("GenerationService")

local function handleGenerateMesh(params)
    local prompt = params.prompt
    local suggestedSize = params.suggested_size  -- Optional [x, y, z]
    local parent = params.parent or "game.Workspace"
    local name = params.name or "GeneratedMesh"

    -- Build options
    local options = {}
    if suggestedSize then
        options["SuggestedSize"] = Vector3.new(
            suggestedSize[1],
            suggestedSize[2],
            suggestedSize[3]
        )
    end

    -- Step 1: Generate
    local success, generationId, contextId = pcall(function()
        return GenerationService:GenerateMeshAsync(
            { ["Prompt"] = prompt },
            nil,  -- No player in plugin context
            options
        )
    end)

    if not success then
        return { success = false, error = generationId }
    end

    -- Step 2: Load
    local loadSuccess, mesh = pcall(function()
        return GenerationService:LoadGeneratedMeshAsync(generationId)
    end)

    if not loadSuccess then
        return {
            success = false,
            error = mesh,
            generationId = generationId  -- Return ID for manual retry
        }
    end

    -- Step 3: Place in scene
    mesh.Name = name
    mesh.Parent = resolvePath(parent)

    return {
        success = true,
        path = getFullPath(mesh),
        generationId = generationId,
        className = mesh.ClassName
    }
end

-- Register command
commands["generate_mesh"] = handleGenerateMesh
```

#### 1.2 Add MCP Tool in Rust

```rust
// src/mcp/params.rs (addition)

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudioGenerateMeshParams {
    /// Text description of the 3D object to generate
    #[schemars(description = "Text prompt describing the object (e.g., 'medieval torch bracket')")]
    pub prompt: String,

    /// Optional bounding box to guide proportions [width, height, depth]
    #[schemars(description = "Optional [x, y, z] size in studs to guide generation")]
    pub suggested_size: Option<[f32; 3]>,

    /// Parent instance path
    #[schemars(description = "Where to place the mesh (default: 'game.Workspace')")]
    pub parent: Option<String>,

    /// Name for the generated mesh
    #[schemars(description = "Name for the MeshPart (default: 'GeneratedMesh')")]
    pub name: Option<String>,
}
```

```rust
// src/mcp/tools/studio.rs (addition)

#[tool(description = "Generate a 3D mesh from text using Roblox's Cube 3D AI. \
    Requires Studio connection. Rate limited to 5/minute.")]
pub async fn studio_generate_mesh(
    &self,
    #[tool(params)] params: StudioGenerateMeshParams,
) -> Result<CallToolResult, McpError> {
    let result = self.bridge.execute(json!({
        "command": "generate_mesh",
        "params": {
            "prompt": params.prompt,
            "suggested_size": params.suggested_size,
            "parent": params.parent,
            "name": params.name,
        }
    })).await?;

    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&result)?
    )]))
}
```

### Phase 2: Error Handling & Validation (Week 1-2)

#### 2.1 Prompt Validation

```rust
fn validate_prompt(prompt: &str) -> Result<(), String> {
    if prompt.is_empty() {
        return Err("Prompt cannot be empty".into());
    }
    if prompt.len() > 500 {  // Assumed limit
        return Err("Prompt exceeds character limit".into());
    }
    // Basic content check (Roblox will also moderate)
    Ok(())
}
```

#### 2.2 Error Code Mapping

| Roblox Error | User Message |
|--------------|--------------|
| `Rate limit exceeded` | "Generation limit reached. Wait 60 seconds." |
| `Moderation failed` | "Prompt was flagged. Try a different description." |
| `Service overloaded` | "Roblox AI service busy. Retry in a moment." |
| `Character limit exceeded` | "Prompt too long. Use fewer words." |

### Phase 3: Enhanced Workflow (Week 2)

#### 3.1 Batch Generation Tool

```rust
#[tool(description = "Generate multiple meshes from a list of prompts")]
pub async fn studio_generate_meshes_batch(
    &self,
    prompts: Vec<BatchMeshRequest>,
) -> Result<CallToolResult, McpError> {
    // Generate with 12-second intervals (5/min limit)
    // Return results as they complete
}
```

#### 3.2 Size Estimation Helper

```rust
#[tool(description = "Suggest appropriate size for a mesh based on description")]
pub async fn cube3d_estimate_size(
    &self,
    prompt: String,
    context: Option<String>,  // e.g., "furniture", "prop", "building"
) -> Result<CallToolResult, McpError> {
    // Use heuristics or LLM to suggest size
    // "torch bracket" → [0.5, 1.0, 0.3]
    // "treasure chest" → [2.0, 1.5, 1.5]
}
```

---

## MCP Tool Specifications

### Tool: `studio_generate_mesh`

```json
{
  "name": "studio_generate_mesh",
  "description": "Generate a 3D mesh from text using Roblox's Cube 3D AI model. The mesh is created directly in Studio. Rate limited to 5 generations per minute.",
  "parameters": {
    "prompt": {
      "type": "string",
      "description": "Text description of the 3D object (e.g., 'medieval torch bracket', 'wooden treasure chest')"
    },
    "suggested_size": {
      "type": "array",
      "items": { "type": "number" },
      "minItems": 3,
      "maxItems": 3,
      "description": "Optional [width, height, depth] in studs to guide mesh proportions"
    },
    "parent": {
      "type": "string",
      "default": "game.Workspace",
      "description": "Instance path where the mesh will be placed"
    },
    "name": {
      "type": "string",
      "default": "GeneratedMesh",
      "description": "Name for the generated MeshPart"
    }
  },
  "required": ["prompt"]
}
```

**Example Usage**:
```
User: "Add a medieval torch bracket to the dungeon wall"

Claude calls: studio_generate_mesh(
  prompt: "medieval iron torch bracket with ornate scrollwork",
  suggested_size: [0.5, 1.0, 0.4],
  parent: "game.Workspace.Dungeon.Walls",
  name: "TorchBracket"
)
```

**Response**:
```json
{
  "success": true,
  "path": "game.Workspace.Dungeon.Walls.TorchBracket",
  "generationId": "gen_abc123",
  "className": "MeshPart"
}
```

---

## Implementation Checklist

### Week 1: Core Implementation
- [ ] Add `generate_mesh` command handler to plugin
- [ ] Test in Studio Edit Mode (plugin context)
- [ ] Add `StudioGenerateMeshParams` to params.rs
- [ ] Implement `studio_generate_mesh` tool
- [ ] Add error handling for rate limits
- [ ] Add error handling for moderation failures

### Week 2: Robustness
- [ ] Implement retry logic with backoff
- [ ] Add generation queue (respect 5/min limit)
- [ ] Add prompt validation

### Week 3: Polish
- [ ] Update API_REFERENCE.md
- [ ] Update CLAUDE.md with usage examples
- [ ] Add unit tests with mock bridge
- [ ] Add integration test (manual, requires Studio)

---

## Constraints & Limitations

| Constraint | Impact | Mitigation |
|------------|--------|------------|
| 5 generations/min | Can't mass-generate | Queue with delays |
| Prompt moderation | Some prompts rejected | Clear error message |
| Mesh not replicated | Only exists locally | Document behavior |
| Requires Studio | Can't use headless | Document requirement |
| EditableMesh format | May need conversion | Use AssetService |

---

## Testing Strategy

### Unit Tests (Rust)
- Parameter validation
- Error code mapping
- Mock bridge responses

### Integration Tests (Manual)
1. **Basic Generation**: "wooden chair" → MeshPart in Workspace
2. **With Size**: "table" with [2, 1, 1] → proportional mesh
3. **Rate Limit**: 6 rapid requests → proper error
4. **Moderation**: Inappropriate prompt → rejection message

### Plugin Tests (Luau)
```lua
-- Test in Studio command bar
local GenerationService = game:GetService("GenerationService")
local success, result = pcall(function()
    return GenerationService:GenerateMeshAsync(
        { ["Prompt"] = "test cube" },
        nil,
        {}
    )
end)
print("GenerateMeshAsync works in plugin:", success)
```

---

## Sources

- [GenerationService Documentation](https://create.roblox.com/docs/reference/engine/classes/GenerationService)
- [GenerateMeshAsync Reference](https://create.roblox.com/docs/reference/engine/classes/GenerationService/GenerateMeshAsync)
- [Cube 3D Beta DevForum Announcement](https://devforum.roblox.com/t/beta-cube-3d-generation-tools-and-apis-for-creators/3558947)
- [SuggestedSize Demo Experience](https://www.roblox.com/games/108838517877898/SuggestedSize-Generation-Template)
