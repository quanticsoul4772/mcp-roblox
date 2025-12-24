-- test_state_machine.luau
-- Tests NPC state machine transition logic

local TestUtils = require("./test_utils")

-- Config values
local NPCConfig = {
	IDLE_CHANCE = 0.15,
	TURN_AROUND_CHANCE = 0.10,
	IDLE_TIME_MIN = 2,
	IDLE_TIME_MAX = 8,
	TURN_DURATION = 0.5,
}

type NPCState = "Idle" | "Walking" | "Turning"

-- Simulate state transition at waypoint
local function simulateWaypointReached(waypointsVisited: number): NPCState
	local minWaypointsBeforeChange = 3

	if waypointsVisited < minWaypointsBeforeChange then
		return "Walking"
	end

	local roll = math.random()
	if roll < NPCConfig.IDLE_CHANCE then
		return "Idle"
	elseif roll < NPCConfig.IDLE_CHANCE + NPCConfig.TURN_AROUND_CHANCE then
		return "Turning"
	else
		return "Walking"
	end
end

local function runTests()
	TestUtils.reset()
	TestUtils.suite("State Machine Logic Tests")

	-- Test 1: NPCs always keep walking for first few waypoints
	print("\nTesting early waypoint behavior:")
	local earlyWalkCount = 0
	for _ = 1, 100 do
		local state = simulateWaypointReached(1)
		if state == "Walking" then
			earlyWalkCount += 1
		end
	end
	TestUtils.assertEqual("NPCs walk after 1st waypoint (100%)", earlyWalkCount, 100)

	-- Test 2: State distribution after enough waypoints
	print("\nTesting state distribution (1000 trials):")
	local idleCount, turnCount, walkCount = 0, 0, 0
	for _ = 1, 1000 do
		local state = simulateWaypointReached(5)
		if state == "Idle" then
			idleCount += 1
		elseif state == "Turning" then
			turnCount += 1
		else
			walkCount += 1
		end
	end

	local idlePct = idleCount / 1000
	local turnPct = turnCount / 1000
	local walkPct = walkCount / 1000

	print(string.format("  Idle: %.1f%% (expected ~15%%)", idlePct * 100))
	print(string.format("  Turn: %.1f%% (expected ~10%%)", turnPct * 100))
	print(string.format("  Walk: %.1f%% (expected ~75%%)", walkPct * 100))

	TestUtils.assertInRange("Idle chance within tolerance", idlePct, 0.10, 0.20)
	TestUtils.assertInRange("Turn chance within tolerance", turnPct, 0.05, 0.15)
	TestUtils.assertInRange("Walk chance within tolerance", walkPct, 0.70, 0.80)

	-- Test 3: Idle time calculation
	print("\nTesting idle time generation:")
	local minTime, maxTime = math.huge, 0
	for _ = 1, 100 do
		local idleTime = math.random() * (NPCConfig.IDLE_TIME_MAX - NPCConfig.IDLE_TIME_MIN) + NPCConfig.IDLE_TIME_MIN
		minTime = math.min(minTime, idleTime)
		maxTime = math.max(maxTime, idleTime)
	end

	TestUtils.test(
		"Idle times within config range",
		minTime >= NPCConfig.IDLE_TIME_MIN and maxTime <= NPCConfig.IDLE_TIME_MAX,
		string.format("Range: %.2f - %.2f", minTime, maxTime)
	)

	-- Test 4: Turn duration
	TestUtils.test(
		"Turn duration is positive",
		NPCConfig.TURN_DURATION > 0
	)

	return TestUtils.summary()
end

local success = runTests()
return success
