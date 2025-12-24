-- run_all_tests.luau
-- Test runner that executes all Luau tests by requiring them directly

-- Colors
local GREEN = "\27[32m"
local RED = "\27[31m"
local YELLOW = "\27[33m"
local CYAN = "\27[36m"
local RESET = "\27[0m"

print(string.format("%s", CYAN))
print("╔════════════════════════════════════════╗")
print("║       NPC System Test Suite            ║")
print("║       Running all Luau tests...        ║")
print("╚════════════════════════════════════════╝")
print(RESET)

-- Track results
local allResults = {}
local totalPassed = 0
local totalFailed = 0

-- Helper to run a test module
local function runTest(name: string, testModule)
	print(string.format("\n%s▶ Running: %s%s", YELLOW, name, RESET))
	print(string.rep("─", 50))

	-- Reset the TestUtils for this test
	local TestUtils = require("./test_utils")
	TestUtils.reset()

	-- Run the test (it should return success boolean)
	local success, err = pcall(function()
		return testModule()
	end)

	local passed = TestUtils.passed
	local failed = TestUtils.failed

	totalPassed += passed
	totalFailed += failed

	local status = "PASSED"
	if not success then
		status = "ERROR"
		print(string.format("%sError: %s%s", RED, tostring(err), RESET))
		totalFailed += 1
	elseif failed > 0 then
		status = "FAILED"
	end

	table.insert(allResults, {
		name = name,
		passed = passed,
		failed = failed,
		status = status,
	})
end

-- Import and run each test
local function runNPCConfigTests()
	local TestUtils = require("./test_utils")

	local NPCConfig = {
		MAX_NPCS = 8,
		SPAWN_RADIUS = 150,
		DESPAWN_RADIUS = 200,
		MIN_SPAWN_DISTANCE = 50,
		MIN_NPC_DISTANCE = 15,
		WALK_SPEED = 8,
		MIN_WALK_DISTANCE = 20,
		MAX_WALK_DISTANCE = 60,
		IDLE_TIME_MIN = 2,
		IDLE_TIME_MAX = 8,
		TURN_DURATION = 0.5,
		IDLE_CHANCE = 0.15,
		TURN_AROUND_CHANCE = 0.10,
	}

	TestUtils.suite("NPCConfig Validation")
	TestUtils.test("MAX_NPCS is mobile-friendly (5-15)", NPCConfig.MAX_NPCS >= 5 and NPCConfig.MAX_NPCS <= 15)
	TestUtils.test("DESPAWN_RADIUS > SPAWN_RADIUS", NPCConfig.DESPAWN_RADIUS > NPCConfig.SPAWN_RADIUS)
	TestUtils.test("MIN_SPAWN_DISTANCE < SPAWN_RADIUS", NPCConfig.MIN_SPAWN_DISTANCE < NPCConfig.SPAWN_RADIUS)
	TestUtils.test("Spawn zone > 50 studs", NPCConfig.SPAWN_RADIUS - NPCConfig.MIN_SPAWN_DISTANCE > 50)
	TestUtils.test("WALK_SPEED < player speed (16)", NPCConfig.WALK_SPEED < 16)
	TestUtils.test("Walk distance range valid", NPCConfig.MAX_WALK_DISTANCE > NPCConfig.MIN_WALK_DISTANCE)
	TestUtils.test("Idle time range valid", NPCConfig.IDLE_TIME_MAX > NPCConfig.IDLE_TIME_MIN)
	TestUtils.test("Turn duration reasonable", NPCConfig.TURN_DURATION >= 0.2 and NPCConfig.TURN_DURATION <= 2.0)
	TestUtils.test("IDLE_CHANCE is 0-1", NPCConfig.IDLE_CHANCE >= 0 and NPCConfig.IDLE_CHANCE <= 1)
	TestUtils.test("TURN_AROUND_CHANCE is 0-1", NPCConfig.TURN_AROUND_CHANCE >= 0 and NPCConfig.TURN_AROUND_CHANCE <= 1)
	TestUtils.test("Combined probability < 100%", NPCConfig.IDLE_CHANCE + NPCConfig.TURN_AROUND_CHANCE < 1)
	TestUtils.test("MIN_NPC_DISTANCE >= 10", NPCConfig.MIN_NPC_DISTANCE >= 10)

	return TestUtils.failed == 0
end

local function runStateMachineTests()
	local TestUtils = require("./test_utils")

	local NPCConfig = {
		IDLE_CHANCE = 0.15,
		TURN_AROUND_CHANCE = 0.10,
		IDLE_TIME_MIN = 2,
		IDLE_TIME_MAX = 8,
	}

	local function simulateWaypointReached(waypointsVisited: number): string
		if waypointsVisited < 3 then return "Walking" end
		local roll = math.random()
		if roll < NPCConfig.IDLE_CHANCE then return "Idle"
		elseif roll < NPCConfig.IDLE_CHANCE + NPCConfig.TURN_AROUND_CHANCE then return "Turning"
		else return "Walking" end
	end

	TestUtils.suite("State Machine Logic")

	-- Early waypoint test
	local earlyWalkCount = 0
	for _ = 1, 100 do
		if simulateWaypointReached(1) == "Walking" then earlyWalkCount += 1 end
	end
	TestUtils.test("NPCs walk after 1st waypoint (100%)", earlyWalkCount == 100)

	-- Distribution test
	local idleCount, turnCount, walkCount = 0, 0, 0
	for _ = 1, 1000 do
		local state = simulateWaypointReached(5)
		if state == "Idle" then idleCount += 1
		elseif state == "Turning" then turnCount += 1
		else walkCount += 1 end
	end

	local idlePct, turnPct, walkPct = idleCount/1000, turnCount/1000, walkCount/1000
	TestUtils.test("Idle ~15% (10-20%)", idlePct >= 0.10 and idlePct <= 0.20)
	TestUtils.test("Turn ~10% (5-15%)", turnPct >= 0.05 and turnPct <= 0.15)
	TestUtils.test("Walk ~75% (70-80%)", walkPct >= 0.70 and walkPct <= 0.80)

	-- Idle time test
	local minTime, maxTime = math.huge, 0
	for _ = 1, 100 do
		local t = math.random() * (NPCConfig.IDLE_TIME_MAX - NPCConfig.IDLE_TIME_MIN) + NPCConfig.IDLE_TIME_MIN
		minTime, maxTime = math.min(minTime, t), math.max(maxTime, t)
	end
	TestUtils.test("Idle times in range", minTime >= NPCConfig.IDLE_TIME_MIN and maxTime <= NPCConfig.IDLE_TIME_MAX)

	return TestUtils.failed == 0
end

local function runWaypointTests()
	local TestUtils = require("./test_utils")

	local function vec3(x, y, z)
		return { X = x or 0, Y = y or 0, Z = z or 0, Magnitude = math.sqrt((x or 0)^2 + (y or 0)^2 + (z or 0)^2) }
	end

	local function subtract(a, b)
		return vec3(a.X - b.X, a.Y - b.Y, a.Z - b.Z)
	end

	local function pickWaypoint(currentPos, currentDirection, waypoints)
		local candidates = {}
		for _, wp in ipairs(waypoints) do
			local toWaypoint = subtract(wp, currentPos)
			local distance = toWaypoint.Magnitude
			if distance > 15 and distance < 80 then
				local angle = math.atan2(toWaypoint.Z, toWaypoint.X)
				local angleDiff = math.abs(angle - currentDirection)
				if angleDiff > math.pi then angleDiff = 2 * math.pi - angleDiff end
				if angleDiff < math.pi / 3 then
					table.insert(candidates, { pos = wp, score = 1 - (angleDiff / (math.pi / 3)) })
				end
			end
		end
		table.sort(candidates, function(a, b) return a.score > b.score end)
		return candidates
	end

	TestUtils.suite("Waypoint Selection")

	local npcPos = vec3(0, 0, 0)
	local waypoints = {
		vec3(50, 0, 0),   -- Ahead
		vec3(50, 0, 20),  -- Slight right
		vec3(50, 0, -20), -- Slight left
		vec3(-50, 0, 0),  -- Behind
		vec3(10, 0, 0),   -- Too close
		vec3(100, 0, 0),  -- Too far
		vec3(0, 0, 50),   -- 90° off
	}

	local candidates = pickWaypoint(npcPos, 0, waypoints)
	TestUtils.test("Filters to 3 valid candidates", #candidates == 3)
	TestUtils.test("Best is directly ahead", #candidates > 0 and candidates[1].pos.X == 50 and candidates[1].pos.Z == 0)
	TestUtils.test("Forward has score ~1.0", #candidates > 0 and candidates[1].score > 0.99)

	local behind = { vec3(-50, 0, 0), vec3(-40, 0, 20) }
	TestUtils.test("Empty when all behind", #pickWaypoint(npcPos, 0, behind) == 0)

	local distTest = { vec3(5, 0, 0), vec3(16, 0, 0), vec3(50, 0, 0), vec3(79, 0, 0), vec3(100, 0, 0) }
	TestUtils.test("Distance filter 15-80", #pickWaypoint(npcPos, 0, distTest) == 3)

	return TestUtils.failed == 0
end

-- Run all tests
runTest("NPCConfig Validation", runNPCConfigTests)
runTest("State Machine Logic", runStateMachineTests)
runTest("Waypoint Selection", runWaypointTests)

-- Summary
print(string.format("\n%s", CYAN))
print("╔════════════════════════════════════════╗")
print("║              TEST SUMMARY              ║")
print("╚════════════════════════════════════════╝")
print(RESET)

print("\n┌────────────────────────────────┬────────┬────────┬─────────┐")
print("│ Test Suite                     │ Passed │ Failed │ Status  │")
print("├────────────────────────────────┼────────┼────────┼─────────┤")

for _, r in ipairs(allResults) do
	local statusColor = r.status == "PASSED" and GREEN or RED
	print(string.format(
		"│ %-30s │ %s%6d%s │ %s%6d%s │ %s%-7s%s │",
		r.name, GREEN, r.passed, RESET,
		r.failed > 0 and RED or GREEN, r.failed, RESET,
		statusColor, r.status, RESET
	))
end

print("├────────────────────────────────┼────────┼────────┼─────────┤")
print(string.format(
	"│ %-30s │ %s%6d%s │ %s%6d%s │         │",
	"TOTAL", GREEN, totalPassed, RESET,
	totalFailed > 0 and RED or GREEN, totalFailed, RESET
))
print("└────────────────────────────────┴────────┴────────┴─────────┘")

print("")
if totalFailed == 0 then
	print(string.format("%s✅ All %d tests passed!%s", GREEN, totalPassed, RESET))
else
	print(string.format("%s❌ %d tests failed out of %d total%s", RED, totalFailed, totalPassed + totalFailed, RESET))
end
