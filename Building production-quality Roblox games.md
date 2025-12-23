# Building production-quality Roblox games: an engineering deep dive

Creating a solid, professional Roblox game requires mastering six interconnected domains: code architecture, performance optimization, modern development workflows, robust game systems, security practices, and continuous learning. **The gap between amateur and professional Roblox development comes down to server-authoritative design, modular architecture, and disciplined engineering practices**—not just scripting ability. This report synthesizes best practices from the Roblox DevForum, official documentation, and professional developers who have shipped games with millions of concurrent players.

## The Service/Controller architecture pattern dominates professional codebases

Professional Roblox developers organize code using a **Service/Controller pattern** that cleanly separates server and client responsibilities. Server logic lives in "Services" (ModuleScripts in ServerScriptService), while client logic lives in "Controllers" (ModuleScripts in StarterPlayerScripts). Both are initialized by single bootstrap scripts rather than dozens of scattered scripts.

The recommended folder structure follows this hierarchy:

| Location | Purpose | Access |
|----------|---------|--------|
| **ServerScriptService** | Server services and main.server.lua bootstrap | Server only |
| **ServerStorage** | Server-only assets, prefabs, inactive content | Server only |
| **ReplicatedStorage** | Shared modules, RemoteEvents, constants, utilities | Client + Server |
| **StarterPlayerScripts** | Client controllers and main.client.lua bootstrap | Client only |
| **ReplicatedFirst** | Loading screens, critical first-load assets | Client + Server |

Each service and controller follows a lifecycle pattern with `Init()` and `Start()` methods. Init runs first across all modules (for setting up dependencies), then Start runs (for connecting events and beginning logic). This two-phase initialization prevents circular dependency issues and race conditions.

**RemoteEvents should be preferred over RemoteFunctions** in nearly all cases. RemoteFunctions can hang the server if a client doesn't respond to a callback—a security vulnerability exploiters can abuse. For request-response patterns, use RemoteEvents with callback patterns: fire a request event, then listen for a response event with a correlation ID.

State management approaches range from simple (Attributes on instances, which auto-replicate) to sophisticated (reactive state libraries like Charm that provide atomic state management with automatic client-server synchronization). For complex character or game phase logic, **state machines** provide clean transitions and prevent invalid state combinations—essential for combat systems, NPC AI, and game flow.

## Performance optimization requires understanding specific budgets and bottlenecks

Professional Roblox games target specific performance budgets validated by Roblox staff engineers. Exceeding these thresholds causes noticeable degradation across the player base:

- **Triangle budget**: ~500,000 triangles in scene
- **Drawcall budget**: ~500 drawcalls
- **Client memory**: Under 1.3GB (to support 2GB mobile devices)
- **Network receive**: Under **50KB/s** (throttling begins at 40KB/s)
- **Moving physics assemblies**: 40-60 maximum (each consumes 0.4-0.9 KB/s bandwidth)

Memory leaks represent the most common performance issue in amateur games. **Event connections hold strong references to their callback variables**, preventing garbage collection even after the connection seems irrelevant. Every connection must be explicitly disconnected using `:Disconnect()` or by destroying the instance that owns the event. Professional developers use "Maid" or "Trove" cleanup patterns that track connections and destroy them together when a system shuts down.

Network optimization offers dramatic improvements through data structure choices. Sending a dictionary `{Name = "Sword", Damage = 50}` costs 14+ bytes just for the string keys—switching to arrays `{"Sword", 50}` eliminates this overhead entirely. Instance references cost only **4 bytes** versus the full string length for IDs. For position data, Vector3int16 uses 6 bytes versus Vector3's 12 bytes, halving bandwidth for integer coordinates.

For physics optimization, use collision groups to prevent unnecessary collision checks, set `CanCollide = false` and `CanTouch = false` on parts that don't need them, and anchor static parts. The MicroProfiler (Ctrl+F6) reveals exactly where frame time is spent—if CPU time vastly exceeds GPU time, scripts or physics are the bottleneck; if GPU exceeds CPU, reduce triangles and drawcalls.

## Modern workflows enable Git-based collaboration and automated deployment

The **Rojo ecosystem** has transformed Roblox development from a solo-in-Studio activity to a professional software engineering workflow. Rojo syncs code between external editors (typically VS Code) and Roblox Studio, enabling Git version control, code review through pull requests, and automated CI/CD pipelines.

The modern Roblox toolchain includes five essential tools, all managed through Rokit (the toolchain manager):

- **Rojo** (rojo-rbx/rojo): Syncs filesystem code to Studio
- **Wally** (UpliftGames/wally): Package manager for Roblox libraries
- **Selene** (Kampfkarren/selene): Luau linter catching bugs and anti-patterns
- **StyLua** (JohnnyMorganz/StyLua): Deterministic code formatter
- **Luau LSP**: Full IDE autocomplete and type checking in VS Code

Testing uses **TestEZ** (Roblox's official BDD testing framework) or **Jest-Roblox** (a more modern port). Tests live in `.spec.lua` files alongside the modules they test, using `describe`, `it`, and `expect` syntax familiar from JavaScript testing. CI pipelines run these tests automatically on pull requests using GitHub Actions, with deployment to Roblox using the Open Cloud API upon merge to production branches.

Top games like Adopt Me, Jailbreak, and Funky Friday all use Rojo-based workflows, demonstrating the pattern scales to millions of concurrent players.

## Data persistence requires session locking and defensive coding

DataStore failures cause the most player complaints in Roblox games. The professional approach uses **ProfileService** (or its successor **ProfileStore**) rather than raw DataStore calls. These libraries provide session locking—preventing the catastrophic data duplication that occurs when a player's data is edited by multiple servers simultaneously.

Critical DataStore practices include:

- **Use `UpdateAsync()` instead of `SetAsync()`**—it provides conditional updates and prevents race conditions
- **Always wrap DataStore calls in `pcall()`** with retry loops using `task.wait()` delays
- **Implement `game:BindToClose()`** to ensure data saves during server shutdowns
- **Store data as dictionaries**, not arrays—`data.Money` is self-documenting, `data[1]` is not
- **Never save NaN values** (check with `value == value` which returns false for NaN)

For UI, **React-lua** has replaced the deprecated Roact as the recommended framework. It provides the familiar React component model with hooks (`useState`, `useEffect`, `useMemo`) adapted for Luau and Roblox instances. This declarative approach dramatically reduces UI bugs compared to imperative ScreenGui manipulation.

Combat systems should follow a **hybrid client-server model**: the client performs hit detection for instant responsiveness and plays visual effects immediately, then sends the action to the server. The server validates the hit (checking distance, cooldowns, line of sight) before applying actual damage. This provides the snappy feel players expect while preventing exploits.

## Security requires server authority and aggressive validation

The fundamental principle of Roblox security is **never trust the client**. All LocalScript bytecode is sent to and stored in client memory—exploiters can decompile, modify, and intercept any client-side code. Every RemoteEvent payload is visible to exploiters and can be spoofed with arbitrary data.

Every server-side remote handler must validate:

- **Type checking**: `if typeof(arg1) ~= "number" then return end`
- **NaN checking**: `if arg1 ~= arg1 then return end` (NaN doesn't equal itself)
- **Sanity limits**: `if string.len(stringArg) > 100 then return end`
- **Cooldown enforcement**: Track last fire time per player, reject rapid requests
- **Logical validation**: Does the player own this item? Can they afford this? Are they in range?

Professional games prefer **passive anti-cheat** that corrects invalid states rather than aggressive punishment. If a player moves faster than their WalkSpeed allows, teleport them back to a valid position rather than kicking them. This handles latency gracefully and avoids false positives that damage legitimate player experience. Reserve bans for clear malicious behavior detected through server-side honeypots—fake RemoteEvents with tempting names that only exploiters would find and fire.

Free models represent a major security risk through server-side backdoors—hidden scripts that listen for commands from the model creator's client. Always inspect free models for `require()` calls with hardcoded asset IDs or `InsertService` usage before publishing.

## Learning resources span official documentation to battle-tested open source

The most valuable learning resources for serious Roblox developers combine official documentation with community expertise:

**Official Resources:**
- **Creator Hub** (create.roblox.com/docs): Comprehensive API reference and tutorials
- **Roblox GitHub** (github.com/Roblox): Luau language source, official frameworks

**DevForum Must-Reads:**
- "How you should secure your game" by Hexcede—comprehensive security with open-source anticheat
- "Best Practices Handbook" by CodedJack—guard clauses, modules, type checking
- "Real world building and scripting optimization" by MrChickenRocket—Roblox staff performance guide

**Frameworks to Study:**
- **Knit**: The most popular service/controller framework
- **ProfileService/ProfileStore**: Industry-standard data persistence
- **React-lua**: Official React port for UI
- **WCS**: Combat framework with skills and status effects

**Discord Communities:**
- HiddenDevs (251,000+ members): Skill development and networking
- RoDevs (131,000+): Hiring, marketplace, technical discussion
- Roblox Studio Community (117,000+): Skill sharing

## R15 NPC systems require understanding Motor6D joints and HumanoidDescription

Creating animated NPCs with clothing is deceptively complex. The two most common pitfalls—broken animations and missing clothing—stem from misunderstanding how R15 rigs work internally.

### Motor6D joints are sacred—never add WeldConstraints to body parts

R15 humanoid rigs consist of 16 MeshParts connected by **15 Motor6D joints**. These joints don't just hold the body together—they're the transformation targets for animations. When an animation plays, it modifies the `Transform` property of each Motor6D to pose the character.

**Critical mistake**: Adding WeldConstraints to "stabilize" NPCs that fall through the ground. WeldConstraints override Motor6D transforms, resulting in a T-posed character even though animations report `IsPlaying = true` and `TimePosition` advances normally.

| Constraint Type | Effect on Animation | Use Case |
|-----------------|---------------------|----------|
| Motor6D | ✅ Animated by Animator | Standard R15 rig joints |
| WeldConstraint | ❌ Locks relative position | Attaching accessories, tools |
| Weld | ❌ Locks relative position | Legacy attachment method |
| RigidConstraint | ❌ Locks position | Physics assemblies |

**Correct collision setup for humanoid NPCs:**
```lua
-- Only HumanoidRootPart needs collision - body parts stay connected via Motor6D
local function setupNPCCollision(model)
    for _, part in ipairs(model:GetDescendants()) do
        if part:IsA("BasePart") then
            if part.Name == "HumanoidRootPart" then
                part.CanCollide = true
            else
                part.CanCollide = false  -- Other parts don't need collision
            end
        end
    end
end
```

If NPCs fall through the ground with this setup, the issue is spawn position or physics groups—not missing welds.

### Clothing requires HumanoidDescription, not Shirt/Pants instances

R15 avatars created via `Players:CreateHumanoidModelFromDescription()` use MeshParts with baked appearance data. **Classic Shirt and Pants instances do not apply to these MeshParts**—they only work with legacy Part-based characters.

| Character Type | Clothing Method | Works? |
|----------------|-----------------|--------|
| Classic R6/R15 (Parts) | Shirt/Pants instances | ✅ Yes |
| HumanoidDescription R15 (MeshParts) | Shirt/Pants instances | ❌ No |
| HumanoidDescription R15 (MeshParts) | HumanoidDescription.Shirt/.Pants | ✅ Yes |

**Correct NPC spawning with clothing:**
```lua
local function createNPCWithClothing()
    local description = Instance.new("HumanoidDescription")
    description.Shirt = 6536082026      -- Asset ID (not rbxassetid:// format)
    description.Pants = 6536084832

    -- Skin tone
    local skinTone = Color3.fromRGB(180, 128, 92)
    description.HeadColor = skinTone
    description.LeftArmColor = skinTone
    description.RightArmColor = skinTone
    description.LeftLegColor = skinTone
    description.RightLegColor = skinTone
    description.TorsoColor = skinTone

    local model = Players:CreateHumanoidModelFromDescription(
        description,
        Enum.HumanoidRigType.R15
    )
    return model
end
```

### LocalScripts don't work for server-spawned NPCs

LocalScripts in characters rely on `game.Players.LocalPlayer` for context. Server-spawned NPCs have no associated player, so standard Animate scripts fail silently.

**Two approaches for NPC animation:**

1. **Server-side animation** (simpler but less smooth):
```lua
-- In server Script
local animator = humanoid:FindFirstChildOfClass("Animator")
local idleAnim = animator:LoadAnimation(idleAnimationInstance)
idleAnim:Play()
```

2. **Client-side animation handler** (recommended for smoothness):
```lua
-- LocalScript in StarterPlayerScripts
local function setupNPCAnimation(npcModel)
    local humanoid = npcModel:FindFirstChildOfClass("Humanoid")
    local animator = humanoid:FindFirstChildOfClass("Animator")

    local idleTrack = animator:LoadAnimation(idleAnimation)
    local walkTrack = animator:LoadAnimation(walkAnimation)

    idleTrack:Play()

    RunService.Heartbeat:Connect(function()
        local velocity = npcModel.PrimaryPart.AssemblyLinearVelocity
        local speed = Vector3.new(velocity.X, 0, velocity.Z).Magnitude

        if speed > 0.5 then
            if not walkTrack.IsPlaying then
                idleTrack:Stop()
                walkTrack:Play()
            end
        else
            if not idleTrack.IsPlaying then
                walkTrack:Stop()
                idleTrack:Play()
            end
        end
    end)
end

-- Scan workspace for NPCs and set them up
workspace.ChildAdded:Connect(function(child)
    if child:FindFirstChild("Humanoid") and not Players:GetPlayerFromCharacter(child) then
        setupNPCAnimation(child)
    end
end)
```

### Debugging animation issues systematically

When NPCs appear in T-pose despite animations "playing":

1. **Check Motor6D joints exist**: Should have 15 for R15 rig
   ```lua
   local jointCount = 0
   for _, desc in model:GetDescendants() do
       if desc:IsA("Motor6D") then jointCount += 1 end
   end
   print("Joint count:", jointCount)  -- Should be 15
   ```

2. **Check for interfering constraints**: WeldConstraints, Welds, RigidConstraints on body parts
   ```lua
   for _, desc in model:GetDescendants() do
       if desc:IsA("WeldConstraint") or desc:IsA("Weld") or desc:IsA("RigidConstraint") then
           warn("Found interfering constraint:", desc:GetFullName())
       end
   end
   ```

3. **Verify animation loads correctly**:
   ```lua
   local track = animator:LoadAnimation(animation)
   print("Animation length:", track.Length)  -- Should be > 0
   track:Play()
   task.wait(0.1)
   print("IsPlaying:", track.IsPlaying)      -- Should be true
   print("TimePosition:", track.TimePosition) -- Should advance
   ```

4. **Check Humanoid.RigType**: Must match animation type
   ```lua
   print("RigType:", humanoid.RigType)  -- Should be Enum.HumanoidRigType.R15
   ```

## Conclusion: engineering discipline separates professionals from amateurs

Building a professional Roblox game requires treating it as real software engineering. The technical differentiators—modular architecture, server-authoritative design, performance profiling, automated testing, and defensive data handling—are the same practices that distinguish professional software development in any domain.

The most impactful improvements for most developers are adopting the Rojo workflow (enabling Git, code review, and external editors), implementing ProfileService for data persistence, and internalizing the "never trust the client" security model. These three changes alone eliminate the most common failure modes in Roblox games: lost player data, exploiter-ruined economies, and unmaintainable spaghetti codebases.

The platform supports games with **21.6 million concurrent players** (Grow a Garden's Guinness World Record)—the infrastructure scales. What separates games that reach that scale from those that don't is the engineering foundation built before launch.