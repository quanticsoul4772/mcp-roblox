-- Roblox Studio Plugin for MCP Server Communication
-- Polls the MCP server for commands and executes them

local HttpService = game:GetService("HttpService")
local Selection = game:GetService("Selection")
local ScriptEditorService = game:GetService("ScriptEditorService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local SERVER_URL = "http://127.0.0.1:8080"
local POLL_INTERVAL = 0.5

local toolbar = plugin:CreateToolbar("MCP Server")
local button = toolbar:CreateButton("Connect", "Connect to MCP Server", "rbxasset://textures/ui/LuaApp/icons/ic-studio-settings.png")

local connected = false

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
        local script = game:FindFirstChild(params.path, true)
        if not script or not script:IsA("LuaSourceContainer") then
            error("Script not found: " .. params.path)
        end
        return { source = script.Source }
        
    elseif action == "modifyScript" then
        local script = game:FindFirstChild(params.path, true)
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

        local parent = game:FindFirstChild(params.parent, true)
        if not parent then
            error("Parent not found: " .. params.parent)
        end
        instance.Parent = parent

        if params.properties then
            for key, value in pairs(params.properties) do
                local success, err = pcall(function()
                    instance[key] = value
                end)
                if not success then
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
        local instance = game:FindFirstChild(params.path, true)
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
        local instance = game:FindFirstChild(params.path, true)
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
            root = game:FindFirstChild(params.root, true)
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
        
        if success and response.StatusCode == 200 and response.Body ~= "null" then
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
        elseif not success then
            warn("[MCP Plugin] Poll failed:", response)
            task.wait(5) -- Back off on errors
        end
        
        task.wait(POLL_INTERVAL)
    end
end

local pollTask = nil

button.Click:Connect(function()
    connected = not connected
    button:SetActive(connected)
    
    if connected then
        print("[MCP Plugin] Connecting to MCP server at", SERVER_URL)
        button.Text = "Disconnect"
        
        -- Start polling loop
        pollTask = task.spawn(pollLoop)
    else
        print("[MCP Plugin] Disconnected from MCP server")
        button.Text = "Connect"
        
        -- Stop polling loop
        if pollTask then
            task.cancel(pollTask)
            pollTask = nil
        end
    end
end)

print("[MCP Plugin] Loaded. Click the toolbar button to connect to MCP server.")
