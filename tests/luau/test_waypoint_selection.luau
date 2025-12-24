-- test_waypoint_selection.luau
-- Tests the directional waypoint picking algorithm

local TestUtils = require("./test_utils")

local vec3 = TestUtils.vec3
local subtract = TestUtils.subtractVec3

-- Waypoint selection algorithm (from NPCStateMachine)
local function pickWaypoint(currentPos, currentDirection: number, waypoints)
	local candidates = {}

	for _, wp in ipairs(waypoints) do
		local toWaypoint = subtract(wp, currentPos)
		local distance = toWaypoint.Magnitude

		-- Distance filter: 15-80 studs
		if distance > 15 and distance < 80 then
			local angle = math.atan2(toWaypoint.Z, toWaypoint.X)
			local angleDiff = math.abs(angle - currentDirection)
			if angleDiff > math.pi then
				angleDiff = 2 * math.pi - angleDiff
			end

			-- Within 60 degrees (pi/3)
			if angleDiff < math.pi / 3 then
				local directionScore = 1 - (angleDiff / (math.pi / 3))
				table.insert(candidates, {
					pos = wp,
					score = directionScore,
					distance = distance,
					angleDeg = math.deg(angleDiff),
				})
			end
		end
	end

	-- Sort by score descending
	table.sort(candidates, function(a, b)
		return a.score > b.score
	end)

	return candidates
end

local function runTests()
	TestUtils.reset()
	TestUtils.suite("Waypoint Selection Tests")

	-- Setup: NPC at origin, facing +X direction
	local npcPos = vec3(0, 0, 0)
	local facingDirection = 0

	-- Test 1: Direction filtering
	print("\nTest: Direction filtering")
	local waypoints = {
		vec3(50, 0, 0), -- Directly ahead
		vec3(50, 0, 20), -- Slightly right (~22°)
		vec3(50, 0, -20), -- Slightly left (~22°)
		vec3(-50, 0, 0), -- Behind (excluded)
		vec3(10, 0, 0), -- Too close (excluded)
		vec3(100, 0, 0), -- Too far (excluded)
		vec3(0, 0, 50), -- 90° off (excluded)
	}

	local candidates = pickWaypoint(npcPos, facingDirection, waypoints)

	print(string.format("  Valid candidates: %d (expected 3)", #candidates))
	for i, c in ipairs(candidates) do
		print(string.format("  %d. (%d,0,%d) score=%.2f angle=%.1f°", i, c.pos.X, c.pos.Z, c.score, c.angleDeg))
	end

	TestUtils.assertEqual("Correct number of valid candidates", #candidates, 3)

	TestUtils.test(
		"Best candidate is directly ahead",
		#candidates > 0 and candidates[1].pos.X == 50 and candidates[1].pos.Z == 0
	)

	TestUtils.test("Forward waypoint has perfect score (1.0)", #candidates > 0 and candidates[1].score > 0.99)

	TestUtils.test(
		"Scores decrease with angle",
		#candidates >= 2 and candidates[1].score > candidates[2].score
	)

	-- Test 2: Edge case - nothing ahead
	print("\nTest: Edge case - all waypoints behind")
	local behindWaypoints = {
		vec3(-50, 0, 0),
		vec3(-40, 0, 20),
	}

	local noCandidates = pickWaypoint(npcPos, facingDirection, behindWaypoints)
	TestUtils.assertEqual("Returns empty when nothing ahead", #noCandidates, 0)

	-- Test 3: Distance filtering
	print("\nTest: Distance filtering (15-80 studs)")
	local distanceWaypoints = {
		vec3(5, 0, 0), -- Too close (<15)
		vec3(14, 0, 0), -- Too close (<15)
		vec3(16, 0, 0), -- Valid (just inside)
		vec3(50, 0, 0), -- Valid (middle)
		vec3(79, 0, 0), -- Valid (just inside)
		vec3(81, 0, 0), -- Too far (>80)
		vec3(100, 0, 0), -- Too far (>80)
	}

	local distanceCandidates = pickWaypoint(npcPos, facingDirection, distanceWaypoints)
	TestUtils.assertEqual("Only 15-80 studs accepted", #distanceCandidates, 3)

	-- Test 4: Angle boundary (60 degrees)
	print("\nTest: Angle boundary (60° = π/3)")
	local angleWaypoints = {
		vec3(50, 0, 0), -- 0° (valid)
		vec3(43, 0, 25), -- ~30° (valid)
		vec3(25, 0, 43), -- ~60° (boundary, excluded)
		vec3(0, 0, 50), -- 90° (excluded)
	}

	local angleCandidates = pickWaypoint(npcPos, facingDirection, angleWaypoints)
	TestUtils.assertEqual("60° boundary respected", #angleCandidates, 2)

	-- Test 5: Different facing direction
	print("\nTest: NPC facing +Z direction")
	local facingZ = math.pi / 2 -- 90 degrees = +Z

	local zWaypoints = {
		vec3(0, 0, 50), -- Ahead in +Z
		vec3(50, 0, 0), -- To the right (90° off)
	}

	local zCandidates = pickWaypoint(npcPos, facingZ, zWaypoints)
	TestUtils.assertEqual("Correct waypoint when facing +Z", #zCandidates, 1)
	TestUtils.test("Selected waypoint is in +Z direction", #zCandidates > 0 and zCandidates[1].pos.Z == 50)

	return TestUtils.summary()
end

local success = runTests()
return success
