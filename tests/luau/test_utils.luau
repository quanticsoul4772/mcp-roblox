-- test_utils.luau
-- Shared testing utilities for Luau tests

local TestUtils = {}

-- Test state
TestUtils.passed = 0
TestUtils.failed = 0
TestUtils.currentSuite = ""

-- Colors for terminal output
local GREEN = "\27[32m"
local RED = "\27[31m"
local YELLOW = "\27[33m"
local RESET = "\27[0m"

function TestUtils.suite(name: string)
	TestUtils.currentSuite = name
	print(string.format("\n%s=== %s ===%s", YELLOW, name, RESET))
end

function TestUtils.test(name: string, condition: boolean, msg: string?)
	if condition then
		TestUtils.passed += 1
		print(string.format("%s✅ %s%s", GREEN, name, RESET))
	else
		TestUtils.failed += 1
		local failMsg = msg and ": " .. msg or ""
		print(string.format("%s❌ %s%s%s", RED, name, failMsg, RESET))
	end
end

function TestUtils.assertEqual(name: string, actual: any, expected: any)
	local condition = actual == expected
	local msg = nil
	if not condition then
		msg = string.format("expected %s, got %s", tostring(expected), tostring(actual))
	end
	TestUtils.test(name, condition, msg)
end

function TestUtils.assertInRange(name: string, value: number, min: number, max: number)
	local condition = value >= min and value <= max
	local msg = nil
	if not condition then
		msg = string.format("%.2f not in range [%.2f, %.2f]", value, min, max)
	end
	TestUtils.test(name, condition, msg)
end

function TestUtils.summary(): boolean
	print(string.format("\n%s=== Results ===%s", YELLOW, RESET))
	print(string.format("Passed: %s%d%s", GREEN, TestUtils.passed, RESET))
	print(string.format("Failed: %s%d%s", TestUtils.failed > 0 and RED or GREEN, TestUtils.failed, RESET))

	local allPassed = TestUtils.failed == 0
	if allPassed then
		print(string.format("\n%s✅ All tests passed!%s", GREEN, RESET))
	else
		print(string.format("\n%s❌ Some tests failed!%s", RED, RESET))
	end

	return allPassed
end

function TestUtils.reset()
	TestUtils.passed = 0
	TestUtils.failed = 0
	TestUtils.currentSuite = ""
end

-- Mock Vector3 for spatial tests
function TestUtils.vec3(x: number, y: number, z: number)
	return {
		X = x or 0,
		Y = y or 0,
		Z = z or 0,
		Magnitude = math.sqrt((x or 0)^2 + (y or 0)^2 + (z or 0)^2)
	}
end

function TestUtils.subtractVec3(a, b)
	return TestUtils.vec3(a.X - b.X, a.Y - b.Y, a.Z - b.Z)
end

return TestUtils
