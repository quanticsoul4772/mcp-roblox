-- Roblox Studio Plugin for MCP Server Communication
-- Polls the MCP server for commands and executes them
-- Features automatic reconnection with exponential backoff

print("[MCP Plugin] VERSION 2025-12-27-A - UV fix loaded")

local HttpService = game:GetService("HttpService")
local Selection = game:GetService("Selection")
local ScriptEditorService = game:GetService("ScriptEditorService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")
local LogService = game:GetService("LogService")

local SERVER_URL = "http://127.0.0.1:8080"
local POLL_INTERVAL = 0.5
local MAX_BACKOFF = 30  -- Maximum backoff in seconds
local INITIAL_BACKOFF = 1  -- Initial backoff in seconds
local MAX_OUTPUT_BUFFER = 500  -- Maximum log entries to keep

local toolbar = plugin:CreateToolbar("MCP Server")
local button = toolbar:CreateButton("Connect", "Connect to MCP Server", "")

local connected = false
local currentBackoff = INITIAL_BACKOFF
local consecutiveFailures = 0

-- Output buffer for getOutput action
local outputBuffer = {}

-- Serialize Roblox types to JSON-compatible values
local function serializeValue(value)
    local t = typeof(value)

    if t == "nil" then
        return nil
    elseif t == "boolean" or t == "number" or t == "string" then
        return value
    elseif t == "Vector3" then
        return { type = "Vector3", value = {value.X, value.Y, value.Z} }
    elseif t == "Vector2" then
        return { type = "Vector2", value = {value.X, value.Y} }
    elseif t == "CFrame" then
        local components = {value:GetComponents()}
        return {
            type = "CFrame",
            position = {components[1], components[2], components[3]},
            value = components
        }
    elseif t == "Color3" then
        return { type = "Color3", value = {value.R, value.G, value.B} }
    elseif t == "BrickColor" then
        return { type = "BrickColor", value = value.Name, number = value.Number }
    elseif t == "UDim" then
        return { type = "UDim", value = {value.Scale, value.Offset} }
    elseif t == "UDim2" then
        return {
            type = "UDim2",
            value = {value.X.Scale, value.X.Offset, value.Y.Scale, value.Y.Offset}
        }
    elseif t == "Rect" then
        return {
            type = "Rect",
            min = {value.Min.X, value.Min.Y},
            max = {value.Max.X, value.Max.Y}
        }
    elseif t == "EnumItem" then
        return { type = "Enum", enum = tostring(value.EnumType), value = value.Name }
    elseif t == "Instance" then
        return { type = "Instance", path = value:GetFullName(), className = value.ClassName }
    elseif t == "NumberRange" then
        return { type = "NumberRange", min = value.Min, max = value.Max }
    elseif t == "NumberSequence" then
        local keypoints = {}
        for _, kp in ipairs(value.Keypoints) do
            table.insert(keypoints, {time = kp.Time, value = kp.Value, envelope = kp.Envelope})
        end
        return { type = "NumberSequence", keypoints = keypoints }
    elseif t == "ColorSequence" then
        local keypoints = {}
        for _, kp in ipairs(value.Keypoints) do
            table.insert(keypoints, {time = kp.Time, color = {kp.Value.R, kp.Value.G, kp.Value.B}})
        end
        return { type = "ColorSequence", keypoints = keypoints }
    else
        -- Fallback: convert to string
        return { type = t, value = tostring(value) }
    end
end

-- Common properties for different class types
local COMMON_PROPERTIES = {
    BasePart = {"Name", "Position", "Size", "CFrame", "Anchored", "CanCollide",
                "Transparency", "Color", "Material", "BrickColor"},
    Model = {"Name", "PrimaryPart"},
    Script = {"Name", "Enabled"},
    GuiObject = {"Name", "Position", "Size", "Visible", "BackgroundColor3",
                 "BackgroundTransparency", "BorderSizePixel"},
}

local function getDefaultPropertiesForClass(instance)
    if instance:IsA("BasePart") then
        return COMMON_PROPERTIES.BasePart
    elseif instance:IsA("Model") then
        return COMMON_PROPERTIES.Model
    elseif instance:IsA("LuaSourceContainer") then
        return COMMON_PROPERTIES.Script
    elseif instance:IsA("GuiObject") then
        return COMMON_PROPERTIES.GuiObject
    else
        return {"Name", "ClassName"}
    end
end

-- Capture log messages
LogService.MessageOut:Connect(function(message, messageType)
    local entry = {
        message = message,
        messageType = tostring(messageType),
        timestamp = os.time()
    }
    table.insert(outputBuffer, entry)
    -- Keep buffer size limited
    while #outputBuffer > MAX_OUTPUT_BUFFER do
        table.remove(outputBuffer, 1)
    end
end)

-- Helper function to resolve dot-notation paths like "Workspace.Part.SubPart"
local function resolveInstancePath(path)
    if not path or path == "" then
        return nil
    end

    -- Handle "game" prefix
    local workingPath = path
    if workingPath:sub(1, 5) == "game." then
        workingPath = workingPath:sub(6)
    elseif workingPath == "game" then
        return game
    end

    -- Split path by dots
    local parts = {}
    for part in workingPath:gmatch("[^.]+") do
        table.insert(parts, part)
    end

    if #parts == 0 then
        return nil
    end

    -- Start from game and traverse
    local current = game
    for _, part in ipairs(parts) do
        local child = current:FindFirstChild(part)
        if not child then
            -- Try recursive search as fallback for first part only
            if current == game then
                child = game:FindFirstChild(part, true)
            end
            if not child then
                return nil
            end
        end
        current = child
    end

    return current
end

-- Helper function to convert JSON values to Roblox types
local function convertValue(value, propertyName)
    if value == nil then
        return nil
    end

    -- If it's already a primitive type, return as-is
    if type(value) ~= "table" and type(value) ~= "string" then
        return value
    end

    -- Handle string-encoded JSON (MCP parameter workaround)
    if type(value) == "string" then
        -- Try to parse as JSON first
        if value:match("^%s*[%[{]") then
            local success, parsed = pcall(function()
                return HttpService:JSONDecode(value)
            end)
            if success and type(parsed) == "table" then
                -- Recursively convert the parsed value
                return convertValue(parsed, propertyName)
            end
        end
    end

    -- Handle string values that might need conversion
    if type(value) == "string" then
        -- Check if it looks like a BrickColor name
        local lowerProp = propertyName and propertyName:lower() or ""
        if lowerProp == "brickcolor" or lowerProp:find("color") then
            -- Try BrickColor first
            local success, brickColor = pcall(function()
                return BrickColor.new(value)
            end)
            if success then
                return brickColor
            end
        end

        -- Check for hex color format (#RRGGBB or #RGB)
        if value:match("^#%x+$") then
            local hex = value:sub(2)
            if #hex == 3 then
                hex = hex:sub(1,1):rep(2) .. hex:sub(2,2):rep(2) .. hex:sub(3,3):rep(2)
            end
            if #hex == 6 then
                local r = tonumber(hex:sub(1,2), 16) / 255
                local g = tonumber(hex:sub(3,4), 16) / 255
                local b = tonumber(hex:sub(5,6), 16) / 255
                return Color3.new(r, g, b)
            end
        end

        -- Return string as-is for other cases
        return value
    end

    -- Handle table/array values
    if type(value) == "table" then
        -- Check for explicit type specification: {type = "Vector3", value = {1, 2, 3}}
        if value.type then
            local t = value.type
            local v = value.value or value

            if t == "Vector3" and type(v) == "table" then
                return Vector3.new(v[1] or v.x or 0, v[2] or v.y or 0, v[3] or v.z or 0)
            elseif t == "Vector2" and type(v) == "table" then
                return Vector2.new(v[1] or v.x or 0, v[2] or v.y or 0)
            elseif t == "Color3" and type(v) == "table" then
                return Color3.new(v[1] or v.r or 0, v[2] or v.g or 0, v[3] or v.b or 0)
            elseif t == "Color3FromRGB" and type(v) == "table" then
                return Color3.fromRGB(v[1] or v.r or 0, v[2] or v.g or 0, v[3] or v.b or 0)
            elseif t == "BrickColor" then
                if type(v) == "string" then
                    return BrickColor.new(v)
                elseif type(v) == "number" then
                    return BrickColor.new(v)
                end
            elseif t == "UDim2" and type(v) == "table" then
                return UDim2.new(v[1] or 0, v[2] or 0, v[3] or 0, v[4] or 0)
            elseif t == "UDim" and type(v) == "table" then
                return UDim.new(v[1] or 0, v[2] or 0)
            elseif t == "CFrame" and type(v) == "table" then
                if #v == 3 then
                    return CFrame.new(v[1], v[2], v[3])
                elseif #v >= 12 then
                    return CFrame.new(v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11], v[12])
                end
            elseif t == "Enum" and value.enum and value.item then
                return Enum[value.enum][value.item]
            end
        end

        -- Auto-detect based on array length and property name
        local lowerProp = propertyName and propertyName:lower() or ""

        -- 3-element array: likely Vector3, Color3, or Size
        if #value == 3 then
            if lowerProp:find("color") then
                -- Assume RGB 0-1 range
                if value[1] <= 1 and value[2] <= 1 and value[3] <= 1 then
                    return Color3.new(value[1], value[2], value[3])
                else
                    -- Assume RGB 0-255 range
                    return Color3.fromRGB(value[1], value[2], value[3])
                end
            else
                -- Default to Vector3 for Position, Size, etc.
                return Vector3.new(value[1], value[2], value[3])
            end
        end

        -- 2-element array: likely Vector2
        if #value == 2 then
            return Vector2.new(value[1], value[2])
        end

        -- 4-element array: likely UDim2
        if #value == 4 then
            return UDim2.new(value[1], value[2], value[3], value[4])
        end
    end

    -- Return as-is if no conversion matched
    return value
end

local function executeCommand(action, params)
    if action == "getSelection" then
        local selected = Selection:Get()
        local instances = {}
        for i, inst in ipairs(selected) do
            table.insert(instances, {
                Name = inst.Name,
                ClassName = inst.ClassName,
                Path = inst:GetFullName()
            })
        end
        return { instances = instances }

    elseif action == "getScriptSource" then
        local script = resolveInstancePath(params.path) or game:FindFirstChild(params.path, true)
        if not script or not script:IsA("LuaSourceContainer") then
            error("Script not found: " .. params.path)
        end
        return { source = script.Source }

    elseif action == "modifyScript" then
        local script = resolveInstancePath(params.path) or game:FindFirstChild(params.path, true)
        if not script or not script:IsA("LuaSourceContainer") then
            error("Script not found: " .. params.path)
        end

        -- Use ScriptEditorService for undo support
        local document = ScriptEditorService:FindScriptDocument(script)
        if document and params.recordUndo ~= false then
            local success = document:EditTextAsync(params.newSource, 1, 1)
            if not success then
                error("EditTextAsync failed for script: " .. params.path)
            end
            ChangeHistoryService:SetWaypoint("MCP Script Edit")
        elseif params.recordUndo == false then
            -- Explicit opt-out of undo - direct assignment allowed
            script.Source = params.newSource
        else
            -- recordUndo requested but no document available
            error("Cannot record undo: script document not available for " .. params.path)
        end

        return { success = true }

    elseif action == "getDataModel" then
        local function serializeInstance(inst, depth, maxDepth)
            if depth >= maxDepth then
                return nil
            end

            local result = {
                Name = inst.Name,
                ClassName = inst.ClassName,
                Path = inst:GetFullName(),
                Children = {}
            }

            for _, child in ipairs(inst:GetChildren()) do
                local serialized = serializeInstance(child, depth + 1, maxDepth)
                if serialized then
                    table.insert(result.Children, serialized)
                end
            end

            return result
        end

        local maxDepth = params.maxDepth or 3
        return serializeInstance(game, 0, maxDepth)

    elseif action == "getDataModelPaginated" then
        -- Paginated DataModel traversal to avoid context overflow
        -- Uses breadth-first traversal with cursor-based pagination
        local startPath = params.startPath or "game"
        local maxDepth = params.maxDepth or 3
        local limit = params.limit or 500
        local cursor = params.cursor  -- Format: "path:childIndex" or nil for start

        -- Find the starting instance
        local startInstance
        if startPath == "game" then
            startInstance = game
        else
            startInstance = resolveInstancePath(startPath) or game:FindFirstChild(startPath, true)
            if not startInstance then
                error("Start path not found: " .. startPath)
            end
        end

        -- Parse cursor if provided (format: "fullPath:childIndex")
        local cursorPath, cursorIndex
        if cursor then
            local colonPos = cursor:find(":[^:]*$")  -- Find last colon
            if colonPos then
                cursorPath = cursor:sub(1, colonPos - 1)
                cursorIndex = tonumber(cursor:sub(colonPos + 1))
            end
        end

        local instances = {}
        local count = 0
        local nextCursor = nil
        local reachedCursor = (cursor == nil)  -- If no cursor, start immediately

        -- Breadth-first traversal with depth tracking
        local queue = {{instance = startInstance, depth = 0}}

        while #queue > 0 and count < limit do
            local current = table.remove(queue, 1)
            local inst = current.instance
            local depth = current.depth
            local shouldProcess = true

            -- Check if we've reached the cursor position
            if not reachedCursor then
                if inst:GetFullName() == cursorPath then
                    reachedCursor = true
                    -- Skip to the child index specified in cursor
                    local children = inst:GetChildren()
                    for i = (cursorIndex or 1), #children do
                        if depth + 1 < maxDepth then
                            table.insert(queue, {instance = children[i], depth = depth + 1})
                        end
                    end
                    shouldProcess = false
                else
                    shouldProcess = false
                end
            end

            if shouldProcess and reachedCursor then
                -- Add this instance to results
                table.insert(instances, {
                    Name = inst.Name,
                    ClassName = inst.ClassName,
                    Path = inst:GetFullName(),
                    Depth = depth
                })
                count = count + 1

                -- Check if we've hit the limit
                if count >= limit then
                    -- Find next item for cursor
                    if #queue > 0 then
                        local nextInst = queue[1].instance
                        nextCursor = nextInst:GetFullName() .. ":1"
                    end
                    break
                end

                -- Add children to queue (breadth-first)
                if depth + 1 < maxDepth then
                    local children = inst:GetChildren()
                    for i, child in ipairs(children) do
                        table.insert(queue, {instance = child, depth = depth + 1})
                    end
                end
            end
        end

        return {
            instances = instances,
            count = count,
            hasMore = (nextCursor ~= nil),
            nextCursor = nextCursor
        }

    elseif action == "createInstance" then
        local success, instance = pcall(function()
            return Instance.new(params.className)
        end)

        if not success then
            error("Invalid class name: " .. params.className)
        end

        if not params.name then
            error("Instance name is required")
        end
        instance.Name = params.name

        local parent = resolveInstancePath(params.parent) or game:FindFirstChild(params.parent, true)
        if not parent then
            error("Parent not found: " .. params.parent)
        end
        instance.Parent = parent

        if params.properties then
            for key, value in pairs(params.properties) do
                local propSuccess, err = pcall(function()
                    instance[key] = convertValue(value, key)
                end)
                if not propSuccess then
                    error("Failed to set property '" .. key .. "': " .. tostring(err))
                end
            end
        end

        if params.recordUndo ~= false then
            ChangeHistoryService:SetWaypoint("MCP Create Instance")
        end

        return {
            success = true,
            path = instance:GetFullName()
        }

    elseif action == "setProperty" then
        local instance = resolveInstancePath(params.path) or game:FindFirstChild(params.path, true)
        if not instance then
            error("Instance not found: " .. params.path)
        end

        if not params.property then
            error("Property name is required")
        end

        local convertedValue = convertValue(params.value, params.property)

        local success, err = pcall(function()
            instance[params.property] = convertedValue
        end)

        if not success then
            error("Failed to set property '" .. params.property .. "': " .. tostring(err))
        end

        if params.recordUndo ~= false then
            ChangeHistoryService:SetWaypoint("MCP Set Property")
        end

        return { success = true }

    elseif action == "deleteInstance" then
        local instance = resolveInstancePath(params.path) or game:FindFirstChild(params.path, true)
        if not instance then
            error("Instance not found: " .. params.path)
        end

        if params.recordUndo ~= false then
            ChangeHistoryService:SetWaypoint("MCP Delete Instance")
        end

        instance:Destroy()
        return { success = true }

    elseif action == "findInstances" then
        if not params.className then
            error("Class name is required")
        end

        local root = game
        if params.root then
            root = resolveInstancePath(params.root) or game:FindFirstChild(params.root, true)
            if not root then
                error("Root not found: " .. params.root)
            end
        end

        local results = {}
        for _, desc in ipairs(root:GetDescendants()) do
            if desc:IsA(params.className) then
                table.insert(results, {
                    Name = desc.Name,
                    ClassName = desc.ClassName,
                    Path = desc:GetFullName()
                })
            end
        end

        return { instances = results }

    elseif action == "getOutput" then
        local limit = params.limit or 100
        local entries = {}

        -- Get the most recent entries up to limit
        local startIndex = math.max(1, #outputBuffer - limit + 1)
        for i = startIndex, #outputBuffer do
            table.insert(entries, outputBuffer[i])
        end

        return { entries = entries, count = #entries }

    elseif action == "getProperties" then
        local instance = resolveInstancePath(params.path) or game:FindFirstChild(params.path, true)
        if not instance then
            error("Instance not found: " .. params.path)
        end

        -- Determine which properties to read
        local propertyNames = params.properties
        if not propertyNames or #propertyNames == 0 then
            propertyNames = getDefaultPropertiesForClass(instance)
        end

        local result = {
            path = instance:GetFullName(),
            className = instance.ClassName,
            properties = {}
        }

        for _, propName in ipairs(propertyNames) do
            local success, value = pcall(function()
                return instance[propName]
            end)

            if success then
                result.properties[propName] = serializeValue(value)
            else
                result.properties[propName] = { error = "Could not read property" }
            end
        end

        return result

    elseif action == "getBounds" then
        local instance = resolveInstancePath(params.path) or game:FindFirstChild(params.path, true)
        if not instance then
            error("Instance not found: " .. params.path)
        end

        local result = {
            path = instance:GetFullName(),
            className = instance.ClassName
        }

        if instance:IsA("BasePart") then
            -- For BaseParts, calculate bounds from Position and Size
            local pos = instance.Position
            local size = instance.Size
            local halfSize = size / 2

            -- Note: This assumes axis-aligned. For rotated parts, would need CFrame math
            result.center = {pos.X, pos.Y, pos.Z}
            result.size = {size.X, size.Y, size.Z}
            result.min = {pos.X - halfSize.X, pos.Y - halfSize.Y, pos.Z - halfSize.Z}
            result.max = {pos.X + halfSize.X, pos.Y + halfSize.Y, pos.Z + halfSize.Z}
            result.orientation = serializeValue(instance.CFrame)

        elseif instance:IsA("Model") then
            -- For Models, use GetBoundingBox()
            local cf, size = instance:GetBoundingBox()
            local pos = cf.Position
            local halfSize = size / 2

            result.center = {pos.X, pos.Y, pos.Z}
            result.size = {size.X, size.Y, size.Z}
            result.min = {pos.X - halfSize.X, pos.Y - halfSize.Y, pos.Z - halfSize.Z}
            result.max = {pos.X + halfSize.X, pos.Y + halfSize.Y, pos.Z + halfSize.Z}
            result.orientation = serializeValue(cf)

        else
            error("Instance must be a BasePart or Model, got: " .. instance.ClassName)
        end

        return result

    elseif action == "generateMesh" then
        -- Generate a 3D mesh from text using Roblox's Cube 3D AI (GenerationService)
        -- Rate limited to 5 generations per minute
        local GenerationService = game:GetService("GenerationService")

        if not params.prompt or params.prompt == "" then
            error("Prompt is required for mesh generation")
        end

        -- Build generation options
        local options = {}
        if params.suggestedSize then
            options["SuggestedSize"] = Vector3.new(
                params.suggestedSize[1] or 1,
                params.suggestedSize[2] or 1,
                params.suggestedSize[3] or 1
            )
        end

        -- Step 1: Generate mesh (returns generationId)
        -- API signature: GenerateMeshAsync(prompts, player, options)
        -- Requires Team Test mode where real Player objects exist in the server window
        local Players = game:GetService("Players")
        local RunService = game:GetService("RunService")

        -- Wait briefly for players to join (Team Test can take a moment)
        local player = nil
        local waitTime = 0
        local maxWait = 5 -- seconds

        repeat
            local children = Players:GetChildren()
            for _, child in ipairs(children) do
                if child:IsA("Player") then
                    player = child
                    break
                end
            end
            if not player then
                task.wait(0.5)
                waitTime = waitTime + 0.5
            end
        until player or waitTime >= maxWait

        -- Debug info
        local isRunning = RunService:IsRunning()
        local isServer = RunService:IsServer()
        local isClient = RunService:IsClient()
        local playerCount = #Players:GetChildren()

        if not player then
            error(string.format(
                "No Player found. IsRunning=%s, IsServer=%s, IsClient=%s, PlayerCount=%d. " ..
                "Use Team Test: Test tab > Start > set Players to 2 > Start",
                tostring(isRunning), tostring(isServer), tostring(isClient), playerCount
            ))
        end

        print(string.format("[MCP Plugin] Found player: %s (IsServer=%s, PlayerCount=%d)",
            player.Name, tostring(isServer), playerCount))

        local generateSuccess, generationId, contextId = pcall(function()
            return GenerationService:GenerateMeshAsync(
                { ["Prompt"] = params.prompt },
                player,
                options
            )
        end)

        if not generateSuccess then
            error("GenerateMeshAsync failed: " .. tostring(generationId))
        end

        print("[MCP Plugin] Generation started, ID: " .. tostring(generationId))

        -- Step 2: Load the generated mesh
        local loadSuccess, mesh = pcall(function()
            return GenerationService:LoadGeneratedMeshAsync(generationId)
        end)

        if not loadSuccess then
            error("LoadGeneratedMeshAsync failed: " .. tostring(mesh))
        end

        -- Step 3: Configure and place the mesh
        local meshName = params.name or "GeneratedMesh"
        mesh.Name = meshName

        local parentPath = params.parent or "game.Workspace"
        local parent = resolveInstancePath(parentPath) or game:FindFirstChild(parentPath, true)
        if not parent then
            error("Parent not found: " .. parentPath)
        end
        mesh.Parent = parent

        if params.recordUndo ~= false then
            ChangeHistoryService:SetWaypoint("MCP Generate Mesh")
        end

        return {
            success = true,
            path = mesh:GetFullName(),
            generationId = generationId,
            contextId = contextId,
            className = mesh.ClassName
        }

    elseif action == "createMeshFromData" then
        -- Create a MeshPart from vertex/face data using EditableMesh APIs
        -- This is used for AI-generated meshes from TRELLIS
        local AssetService = game:GetService("AssetService")

        if not params.vertices or #params.vertices == 0 then
            error("Vertices array is required and cannot be empty")
        end
        if not params.faces or #params.faces == 0 then
            error("Faces array is required and cannot be empty")
        end

        print(string.format("[MCP Plugin] Creating mesh from data: %d vertices, %d faces",
            #params.vertices, #params.faces))

        -- Create an empty EditableMesh
        local editableMesh = AssetService:CreateEditableMesh()
        if not editableMesh then
            error("Failed to create EditableMesh - check device memory budget")
        end

        -- Add vertices
        local vertexIds = {}
        for i, v in ipairs(params.vertices) do
            -- v is {x, y, z} from the OBJ parser
            local vertId = editableMesh:AddVertex(Vector3.new(v.x, v.y, v.z))
            vertexIds[i] = vertId
        end

        -- Add normals if provided
        local normalIds = {}
        if params.normals and #params.normals > 0 then
            for i, n in ipairs(params.normals) do
                local normId = editableMesh:AddNormal(Vector3.new(n.x, n.y, n.z))
                normalIds[i] = normId
            end
        end

        -- Add default UVs for each vertex (required by CreateMeshPartAsync)
        local uvIds = {}
        for i = 1, #params.vertices do
            local uvId = editableMesh:AddUV(Vector2.new(0, 0))
            uvIds[i] = uvId
        end

        -- Add faces with UVs
        for _, face in ipairs(params.faces) do
            local v1 = vertexIds[face.vertices[1] + 1]
            local v2 = vertexIds[face.vertices[2] + 1]
            local v3 = vertexIds[face.vertices[3] + 1]

            if v1 and v2 and v3 then
                local faceId = editableMesh:AddTriangle(v1, v2, v3)
                -- Assign UVs to face vertices
                local uv1 = uvIds[face.vertices[1] + 1]
                local uv2 = uvIds[face.vertices[2] + 1]
                local uv3 = uvIds[face.vertices[3] + 1]
                if uv1 and uv2 and uv3 then
                    pcall(function()
                        editableMesh:SetFaceUVs(faceId, {uv1, uv2, uv3})
                    end)
                end
            end
        end

        print("[MCP Plugin] EditableMesh populated, creating MeshPart...")

        -- Create MeshPart from EditableMesh
        local meshContent = Content.fromObject(editableMesh)
        local meshPart = AssetService:CreateMeshPartAsync(meshContent, {
            CollisionFidelity = Enum.CollisionFidelity.Default,
            RenderFidelity = Enum.RenderFidelity.Automatic
        })

        if not meshPart then
            error("Failed to create MeshPart from EditableMesh")
        end

        -- Configure the MeshPart
        local meshName = params.name or "GeneratedMesh"
        meshPart.Name = meshName

        local parentPath = params.parent or "game.Workspace"
        local parent = resolveInstancePath(parentPath) or game:FindFirstChild(parentPath, true)
        if not parent then
            error("Parent not found: " .. parentPath)
        end
        meshPart.Parent = parent

        -- Clean up EditableMesh (it's no longer needed after CreateMeshPartAsync)
        editableMesh:Destroy()

        if params.recordUndo ~= false then
            ChangeHistoryService:SetWaypoint("MCP Create Mesh From Data")
        end

        print(string.format("[MCP Plugin] MeshPart created: %s", meshPart:GetFullName()))

        return {
            success = true,
            path = meshPart:GetFullName(),
            vertexCount = #params.vertices,
            faceCount = #params.faces,
            className = meshPart.ClassName
        }

    elseif action == "insertR15Rig" then
        -- Create a complete R15 humanoid rig using Players service for proper mesh parts
        local Players = game:GetService("Players")

        local parent = resolveInstancePath(params.parent) or game:FindFirstChild(params.parent, true)
        if not parent then
            error("Parent not found: " .. params.parent)
        end

        if not params.name then
            error("Rig name is required")
        end

        -- Create a HumanoidDescription for a default avatar
        local description = Instance.new("HumanoidDescription")

        -- Use default Roblox body parts (these are the standard R15 mesh IDs)
        description.Head = 0  -- Default head
        description.Torso = 0
        description.LeftArm = 0
        description.RightArm = 0
        description.LeftLeg = 0
        description.RightLeg = 0

        -- Create the model using Players service
        local success, model = pcall(function()
            return Players:CreateHumanoidModelFromDescription(description, Enum.HumanoidRigType.R15)
        end)

        if not success or not model then
            -- Fallback: Try creating from user ID 1 (Roblox)
            success, model = pcall(function()
                return Players:CreateHumanoidModelFromUserId(1)
            end)
        end

        if not success or not model then
            error("Failed to create R15 rig: " .. tostring(model))
        end

        -- Rename and configure the model
        model.Name = params.name

        -- Remove accessories and clothing for a clean NPC
        for _, child in ipairs(model:GetDescendants()) do
            if child:IsA("Accessory") or child:IsA("Shirt") or child:IsA("Pants") or child:IsA("ShirtGraphic") then
                child:Destroy()
            end
        end

        -- Set body colors to a neutral tone
        local bodyColors = model:FindFirstChildOfClass("BodyColors")
        if not bodyColors then
            bodyColors = Instance.new("BodyColors")
            bodyColors.Parent = model
        end
        local skinColor = BrickColor.new("Light orange")
        bodyColors.HeadColor = skinColor
        bodyColors.TorsoColor = skinColor
        bodyColors.LeftArmColor = skinColor
        bodyColors.RightArmColor = skinColor
        bodyColors.LeftLegColor = skinColor
        bodyColors.RightLegColor = skinColor

        -- Make sure Humanoid has an Animator
        local humanoid = model:FindFirstChildOfClass("Humanoid")
        if humanoid then
            if not humanoid:FindFirstChildOfClass("Animator") then
                local animator = Instance.new("Animator")
                animator.Parent = humanoid
            end
        end

        -- Parent model to target
        model.Parent = parent

        -- Cleanup the description
        description:Destroy()

        if params.recordUndo ~= false then
            ChangeHistoryService:SetWaypoint("MCP Insert R15 Rig")
        end

        return {
            success = true,
            path = model:GetFullName(),
            method = "HumanoidDescription"
        }
    end

    error("Unknown action: " .. action)
end

local function checkHealth()
    local success, response = pcall(function()
        return HttpService:RequestAsync({
            Url = SERVER_URL .. "/health",
            Method = "GET"
        })
    end)

    return success and response.StatusCode == 200
end

local function pollLoop()
    while connected do
        local success, response = pcall(function()
            return HttpService:RequestAsync({
                Url = SERVER_URL .. "/poll",
                Method = "GET"
            })
        end)

        if success and response.StatusCode == 200 then
            -- Reset backoff on successful connection
            consecutiveFailures = 0
            currentBackoff = INITIAL_BACKOFF

            if response.Body ~= "null" then
                local command = HttpService:JSONDecode(response.Body)
                local result, error_msg

                -- Execute command with error capture
                local exec_success, exec_result = pcall(function()
                    return executeCommand(command.action, command.params)
                end)

                if exec_success then
                    result = exec_result
                else
                    error_msg = tostring(exec_result)
                    warn("[MCP Plugin] Command failed:", error_msg)
                end

                -- Send result back (errors included)
                pcall(function()
                    HttpService:RequestAsync({
                        Url = SERVER_URL .. "/result",
                        Method = "POST",
                        Headers = { ["Content-Type"] = "application/json" },
                        Body = HttpService:JSONEncode({
                            id = command.id,
                            result = result,
                            error = error_msg
                        })
                    })
                end)
            end

            task.wait(POLL_INTERVAL)
        else
            -- Connection failed - apply exponential backoff
            consecutiveFailures = consecutiveFailures + 1

            if consecutiveFailures == 1 then
                warn("[MCP Plugin] Connection lost. Attempting to reconnect...")
            end

            -- Wait with exponential backoff
            task.wait(currentBackoff)

            -- Increase backoff for next failure (capped at MAX_BACKOFF)
            currentBackoff = math.min(currentBackoff * 2, MAX_BACKOFF)

            -- Periodically log reconnection attempts
            if consecutiveFailures % 5 == 0 then
                warn(string.format("[MCP Plugin] Still trying to reconnect... (attempt %d, backoff: %ds)",
                    consecutiveFailures, currentBackoff))
            end
        end
    end
end

local pollTask = nil

button.Click:Connect(function()
    connected = not connected
    button:SetActive(connected)

    if connected then
        -- Reset backoff state on manual connect
        consecutiveFailures = 0
        currentBackoff = INITIAL_BACKOFF

        print("[MCP Plugin] Connecting to MCP server at", SERVER_URL)

        -- Check server health first
        if checkHealth() then
            print("[MCP Plugin] Server is healthy, starting poll loop")
        else
            warn("[MCP Plugin] Server not responding, will retry with backoff")
        end

        -- Start polling loop
        pollTask = task.spawn(pollLoop)
    else
        print("[MCP Plugin] Disconnected from MCP server")

        -- Stop polling loop
        if pollTask then
            task.cancel(pollTask)
            pollTask = nil
        end
    end
end)

print("[MCP Plugin] Loaded. Click the toolbar button to connect to MCP server.")
print("[MCP Plugin] Features: Auto-reconnection with exponential backoff (max " .. MAX_BACKOFF .. "s)")
