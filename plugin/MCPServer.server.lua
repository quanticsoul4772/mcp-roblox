-- Roblox Studio Plugin for MCP Server Communication
-- Polls the MCP server for commands and executes them
-- Features automatic reconnection with exponential backoff

local HttpService = game:GetService("HttpService")
local Selection = game:GetService("Selection")
local ScriptEditorService = game:GetService("ScriptEditorService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")
local LogService = game:GetService("LogService")

local SERVER_URL = "http://127.0.0.1:8081"
local POLL_INTERVAL = 0.5
local MAX_BACKOFF = 30
local INITIAL_BACKOFF = 1

local toolbar = plugin:CreateToolbar("MCP Server")
local button = toolbar:CreateButton("Connect", "Connect to MCP Server", "")

local connected = false
local currentBackoff = INITIAL_BACKOFF
local consecutiveFailures = 0

-- Resolve a dot-separated path to an instance
-- Examples: "Workspace.Part1", "game.Workspace.Part1", "ServerScriptService.Main"
local function resolvePath(path)
    if not path or path == "" then
        return nil
    end
    
    -- Split path by dots
    local parts = {}
    for part in string.gmatch(path, "[^%.]+") do
        table.insert(parts, part)
    end
    
    if #parts == 0 then
        return nil
    end
    
    -- Start from game, skip "game" if it's the first part
    local current = game
    local startIndex = 1
    
    if parts[1] == "game" or parts[1] == "Game" then
        startIndex = 2
    end
    
    -- Navigate through the path
    for i = startIndex, #parts do
        local childName = parts[i]
        local child = current:FindFirstChild(childName)
        if not child then
            return nil
        end
        current = child
    end
    
    return current
end

local function executeCommand(action, params)
    if action == "getSelection" then
        local selected = Selection:Get()
        local instances = {}
        for _, inst in ipairs(selected) do
            table.insert(instances, {
                Name = inst.Name,
                ClassName = inst.ClassName,
                Path = inst:GetFullName()
            })
        end
        return { instances = instances }

    elseif action == "getScriptSource" then
        local inst = resolvePath(params.path)
        if not inst or not inst:IsA("LuaSourceContainer") then
            error("Script not found: " .. params.path)
        end
        return { source = inst.Source }

    elseif action == "modifyScript" then
        local inst = resolvePath(params.path)
        if not inst or not inst:IsA("LuaSourceContainer") then
            error("Script not found: " .. params.path)
        end

        if params.recordUndo == false then
            inst.Source = params.newSource
        else
            local document = ScriptEditorService:FindScriptDocument(inst)
            if document then
                local success = document:EditTextAsync(params.newSource, 1, 1)
                if not success then
                    error("EditTextAsync failed for script: " .. params.path)
                end
                ChangeHistoryService:SetWaypoint("MCP Script Edit")
            else
                error("Cannot record undo: script document not available for " .. params.path)
            end
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
        local startPath = params.startPath or "game"
        local maxDepth = params.maxDepth or 3
        local limit = params.limit or 500
        local cursor = params.cursor

        local startInstance
        if startPath == "game" then
            startInstance = game
        else
            startInstance = resolvePath(startPath)
            if not startInstance then
                error("Start path not found: " .. startPath)
            end
        end

        local cursorPath, cursorIndex
        if cursor then
            local colonPos = cursor:find(":[^:]*$")
            if colonPos then
                cursorPath = cursor:sub(1, colonPos - 1)
                cursorIndex = tonumber(cursor:sub(colonPos + 1))
            end
        end

        local instances = {}
        local count = 0
        local nextCursor = nil
        local reachedCursor = (cursor == nil)

        local queue = {{instance = startInstance, depth = 0}}

        while #queue > 0 and count < limit do
            local current = table.remove(queue, 1)
            local inst = current.instance
            local depth = current.depth
            local shouldProcess = true

            if not reachedCursor then
                if inst:GetFullName() == cursorPath then
                    reachedCursor = true
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
                table.insert(instances, {
                    Name = inst.Name,
                    ClassName = inst.ClassName,
                    Path = inst:GetFullName(),
                    Depth = depth
                })
                count = count + 1

                if count >= limit then
                    if #queue > 0 then
                        local nextInst = queue[1].instance
                        nextCursor = nextInst:GetFullName() .. ":1"
                    end
                    break
                end

                if depth + 1 < maxDepth then
                    local children = inst:GetChildren()
                    for _, child in ipairs(children) do
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

        local parent = resolvePath(params.parent)
        if not parent then
            error("Parent not found: " .. params.parent)
        end
        instance.Parent = parent

        if params.properties then
            for key, value in pairs(params.properties) do
                local propSuccess, err = pcall(function()
                    instance[key] = value
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
        local instance = resolvePath(params.path)
        if not instance then
            error("Instance not found: " .. params.path)
        end

        if not params.property then
            error("Property name is required")
        end

        local success, err = pcall(function()
            instance[params.property] = params.value
        end)

        if not success then
            error("Failed to set property '" .. params.property .. "': " .. tostring(err))
        end

        if params.recordUndo ~= false then
            ChangeHistoryService:SetWaypoint("MCP Set Property")
        end

        return { success = true }

    elseif action == "deleteInstance" then
        local instance = resolvePath(params.path)
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
            root = resolvePath(params.root)
            if not root then
                error("Root not found: " .. params.root)
            end
        end

        local results = {}
        for _, desc in ipairs(root:GetDescendants()) do
            if desc.ClassName == params.className then
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
        local history = LogService:GetLogHistory()
        local logs = {}
        local startIndex = math.max(1, #history - limit + 1)
        
        for i = startIndex, #history do
            local entry = history[i]
            table.insert(logs, {
                message = entry.message,
                messageType = tostring(entry.messageType),
                timestamp = entry.timestamp
            })
        end
        
        return { logs = logs, count = #logs }
    end

    error("Unknown action: " .. action)
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
            consecutiveFailures = 0
            currentBackoff = INITIAL_BACKOFF

            if response.Body and response.Body ~= "null" and response.Body ~= "" then
                local decodeSuccess, command = pcall(function()
                    return HttpService:JSONDecode(response.Body)
                end)
                
                if decodeSuccess and command then
                    local result, error_msg

                    local exec_success, exec_result = pcall(function()
                        return executeCommand(command.action, command.params)
                    end)

                    if exec_success then
                        result = exec_result
                    else
                        error_msg = tostring(exec_result)
                        warn("[MCP Plugin] Command failed:", error_msg)
                    end

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
            end

            task.wait(POLL_INTERVAL)
        else
            consecutiveFailures = consecutiveFailures + 1

            if consecutiveFailures == 1 then
                warn("[MCP Plugin] Connection lost. Attempting to reconnect...")
            end

            task.wait(currentBackoff)
            currentBackoff = math.min(currentBackoff * 2, MAX_BACKOFF)

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
        consecutiveFailures = 0
        currentBackoff = INITIAL_BACKOFF

        print("[MCP Plugin] Connecting to MCP server at", SERVER_URL)

        local success, response = pcall(function()
            return HttpService:RequestAsync({
                Url = SERVER_URL .. "/health",
                Method = "GET"
            })
        end)

        if success and response.StatusCode == 200 then
            print("[MCP Plugin] Server is healthy, starting poll loop")
        else
            warn("[MCP Plugin] Server not responding, will retry with backoff")
        end

        pollTask = task.spawn(pollLoop)
    else
        print("[MCP Plugin] Disconnected from MCP server")

        if pollTask then
            task.cancel(pollTask)
            pollTask = nil
        end
    end
end)

print("[MCP Plugin] Loaded. Click the toolbar button to connect to MCP server.")
print("[MCP Plugin] Features: Auto-reconnection with exponential backoff (max " .. MAX_BACKOFF .. "s)")
