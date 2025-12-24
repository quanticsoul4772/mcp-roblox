-- test_npc_config.luau
-- Validates NPCConfig values for sanity

local TestUtils = require("./test_utils")

-- NPCConfig values (copy from Studio for testing)
local NPCConfig = {
	-- Spawn settings
	MAX_NPCS = 8,
	SPAWN_RADIUS = 150,
	DESPAWN_RADIUS = 200,
	MIN_SPAWN_DISTANCE = 50,
	MIN_NPC_DISTANCE = 15,

	-- Movement settings
	WALK_SPEED = 8,
	MIN_WALK_DISTANCE = 20,
	MAX_WALK_DISTANCE = 60,

	-- Timing settings
	IDLE_TIME_MIN = 2,
	IDLE_TIME_MAX = 8,
	TURN_DURATION = 0.5,

	-- State probabilities
	IDLE_CHANCE = 0.15,
	TURN_AROUND_CHANCE = 0.10,
}

local function runTests()
	TestUtils.reset()
	TestUtils.suite("NPCConfig Validation Tests")

	-- Spawn settings
	TestUtils.test(
		"MAX_NPCS is mobile-friendly (5-15)",
		NPCConfig.MAX_NPCS >= 5 and NPCConfig.MAX_NPCS <= 15
	)

	TestUtils.test(
		"DESPAWN_RADIUS > SPAWN_RADIUS (hysteresis)",
		NPCConfig.DESPAWN_RADIUS > NPCConfig.SPAWN_RADIUS,
		"Prevents spawn/despawn flickering"
	)

	TestUtils.test(
		"MIN_SPAWN_DISTANCE < SPAWN_RADIUS",
		NPCConfig.MIN_SPAWN_DISTANCE < NPCConfig.SPAWN_RADIUS,
		"Must have valid spawn zone"
	)

	local spawnZone = NPCConfig.SPAWN_RADIUS - NPCConfig.MIN_SPAWN_DISTANCE
	TestUtils.test(
		"Spawn zone is large enough (>50 studs)",
		spawnZone > 50,
		"Zone: " .. spawnZone .. " studs"
	)

	-- Movement settings
	TestUtils.test(
		"WALK_SPEED slower than player (16)",
		NPCConfig.WALK_SPEED < 16,
		"NPCs shouldn't outrun players"
	)

	TestUtils.test(
		"Walk distance range valid",
		NPCConfig.MAX_WALK_DISTANCE > NPCConfig.MIN_WALK_DISTANCE
	)

	-- Timing settings
	TestUtils.test(
		"Idle time range valid",
		NPCConfig.IDLE_TIME_MAX > NPCConfig.IDLE_TIME_MIN
	)

	TestUtils.test(
		"Turn duration reasonable (0.2-2.0s)",
		NPCConfig.TURN_DURATION >= 0.2 and NPCConfig.TURN_DURATION <= 2.0
	)

	-- Probability settings
	TestUtils.test(
		"IDLE_CHANCE is percentage (0-1)",
		NPCConfig.IDLE_CHANCE >= 0 and NPCConfig.IDLE_CHANCE <= 1
	)

	TestUtils.test(
		"TURN_AROUND_CHANCE is percentage (0-1)",
		NPCConfig.TURN_AROUND_CHANCE >= 0 and NPCConfig.TURN_AROUND_CHANCE <= 1
	)

	TestUtils.test(
		"Combined probability < 100%",
		NPCConfig.IDLE_CHANCE + NPCConfig.TURN_AROUND_CHANCE < 1,
		"Otherwise NPCs never keep walking"
	)

	-- Performance considerations
	TestUtils.test(
		"MIN_NPC_DISTANCE prevents clustering (>=10)",
		NPCConfig.MIN_NPC_DISTANCE >= 10
	)

	return TestUtils.summary()
end

-- Run if executed directly
local success = runTests()
return success
