# Lune MCP Integration - Implementation Plan

## Overview

Integrate [Lune](https://github.com/lune-org/lune) (standalone Luau runtime) into mcp-roblox as MCP tools, enabling Claude to test Luau logic without Studio round-trips.

**Value Proposition:**
- Fastest feedback loop - no Studio required
- Test pure Luau logic (math, data structures, algorithms)
- CI/CD integration - run tests in GitHub Actions
- Access to Lune's built-in libraries (fs, net, process, task, serde, roblox)

---

## Architecture

### Pattern: Follow Existing Toolchain Design

```
src/tools/lune.rs          # LuneRunner trait + DefaultLuneRunner + MockLuneRunner
src/mcp/params.rs          # LuneRunParams, LuneEvalParams
src/mcp/tools/toolchain.rs # lune_run_impl, lune_eval_impl
src/mcp/server.rs          # Tool registration via #[tool] macro
```

### Type Hierarchy

```rust
// Follows same pattern as RojoRunner, WallyRunner, etc.
RobloxMcpServer<B, L, F, R, W, M, LN>
                              └── LN: LuneRunner
```

---

## New MCP Tools

### 1. `lune_run` - Execute a Luau Script File

**Description:** Run a Luau script file using Lune runtime. Returns stdout, stderr, and exit code.

**Parameters:**
```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LuneRunParams {
    #[schemars(description = "Path to .luau script file to run")]
    pub script_path: String,

    #[schemars(description = "Command-line arguments to pass to the script")]
    pub args: Option<Vec<String>>,

    #[schemars(description = "Timeout in seconds (default: 30)")]
    pub timeout: Option<u64>,
}
```

**Response:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LuneRunResult {
    /// Whether the script executed successfully (exit code 0)
    pub success: bool,
    /// Exit code from the script
    pub exit_code: i32,
    /// Stdout from the script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Stderr from the script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Execution time in milliseconds
    pub duration_ms: u64,
}
```

**Use Cases:**
- Run test scripts: `lune_run("tests/unit/math_utils.luau")`
- Execute build scripts: `lune_run("scripts/generate_types.luau")`
- Validate game logic offline

---

### 2. `lune_eval` - Evaluate Inline Luau Code

**Description:** Evaluate Luau code directly without a file. Useful for quick tests and REPL-like interactions.

**Parameters:**
```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LuneEvalParams {
    #[schemars(description = "Luau code to evaluate")]
    pub code: String,

    #[schemars(description = "Timeout in seconds (default: 10)")]
    pub timeout: Option<u64>,
}
```

**Response:** Same as `LuneRunResult`

**Use Cases:**
- Test a function: `lune_eval("print(require('@lune/fs').isFile('src/init.luau'))")`
- Validate JSON parsing: `lune_eval("local serde = require('@lune/serde'); print(serde.decode('json', '{\"a\":1}').a)")`
- Quick calculations: `lune_eval("print(2^10)")`

---

### 3. `lune_test` - Run Test Suite (Optional Enhancement)

**Description:** Run a directory of test files with structured reporting.

**Parameters:**
```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LuneTestParams {
    #[schemars(description = "Path to test directory or file")]
    pub path: String,

    #[schemars(description = "Pattern to match test files (default: '*.spec.luau')")]
    pub pattern: Option<String>,

    #[schemars(description = "Continue on failure (default: false)")]
    pub continue_on_failure: Option<bool>,
}
```

**Response:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LuneTestResult {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
    pub results: Vec<TestCaseResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseResult {
    pub name: String,
    pub file: String,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
}
```

---

## Implementation Steps

### Phase 1: Core Infrastructure (Est: 2-3 hours)

1. **Create `src/tools/lune.rs`**
   ```rust
   //! Lune runtime integration
   //!
   //! Provides a trait-based abstraction for Lune operations to enable testing
   //! without requiring the external Lune binary.

   #[async_trait]
   pub trait LuneRunner: Send + Sync {
       async fn run(&self, script_path: &Path, args: &[String], timeout: Option<Duration>)
           -> Result<LuneRunResult, RobloxMcpError>;

       async fn eval(&self, code: &str, timeout: Option<Duration>)
           -> Result<LuneRunResult, RobloxMcpError>;
   }

   pub struct DefaultLuneRunner;

   #[cfg(test)]
   pub mod mock {
       pub struct MockLuneRunner { /* ... */ }
   }
   ```

2. **Add params to `src/mcp/params.rs`**
   ```rust
   // === LUNE PARAMS ===

   #[derive(Debug, Deserialize, JsonSchema)]
   pub struct LuneRunParams { /* ... */ }

   #[derive(Debug, Deserialize, JsonSchema)]
   pub struct LuneEvalParams { /* ... */ }
   ```

3. **Update `src/tools/mod.rs`**
   ```rust
   pub mod lune;
   pub use lune::{DefaultLuneRunner, LuneRunner, LuneRunResult};
   ```

### Phase 2: MCP Tool Implementations (Est: 2 hours)

4. **Add to `src/mcp/tools/toolchain.rs`**
   ```rust
   // =========================================================================
   // lune_run - Run a Luau script using Lune runtime
   // =========================================================================

   pub(crate) async fn lune_run_impl(&self, params: LuneRunParams)
       -> Result<CallToolResult, ErrorData> {
       // Path validation
       // Timeout handling
       // Execute via self.lune.run()
       // Return JSON result
   }

   pub(crate) async fn lune_eval_impl(&self, params: LuneEvalParams)
       -> Result<CallToolResult, ErrorData> {
       // Write temp file with code
       // Execute via self.lune.run()
       // Clean up temp file
       // Return JSON result
   }
   ```

5. **Register tools in `src/mcp/server.rs`**
   ```rust
   #[tool(description = "Run a Luau script using Lune runtime. Returns stdout, stderr, and exit code.")]
   async fn lune_run(&self, Parameters(params): Parameters<LuneRunParams>)
       -> Result<CallToolResult, ErrorData> {
       let call = self.start_instrumentation("lune_run");
       let result = self.lune_run_impl(params).await;
       call.finish_with(result).await
   }

   #[tool(description = "Evaluate inline Luau code using Lune runtime.")]
   async fn lune_eval(&self, Parameters(params): Parameters<LuneEvalParams>)
       -> Result<CallToolResult, ErrorData> {
       let call = self.start_instrumentation("lune_eval");
       let result = self.lune_eval_impl(params).await;
       call.finish_with(result).await
   }
   ```

### Phase 3: Server Type Updates (Est: 1 hour)

6. **Update `RobloxMcpServer` type signature**
   ```rust
   pub struct RobloxMcpServer<B, L, F, R, W, M, LN>
   where
       B: StudioBridge,
       L: Linter,
       F: Formatter,
       R: RojoRunner,
       W: WallyRunner,
       M: MoonwaveRunner,
       LN: LuneRunner,  // NEW
   {
       // ... existing fields ...
       lune: LN,  // NEW
   }
   ```

7. **Update constructors** (`new()`, `new_for_testing()`, `with_mock_*()`)

### Phase 4: Testing (Est: 2 hours)

8. **Unit tests in `src/tools/lune.rs`**
   - `test_lune_run_result_serialization`
   - `test_default_lune_runner_new`
   - Mock tests for success/failure scenarios

9. **MCP tool tests in `src/mcp/server.rs`**
   - `test_lune_run_success`
   - `test_lune_run_timeout`
   - `test_lune_run_script_error`
   - `test_lune_eval_success`
   - `test_lune_eval_syntax_error`
   - `test_lune_path_traversal_blocked`

10. **Integration tests in `tests/toolchain_integration.rs`**
    - Real Lune execution tests (require Lune installed)

---

## CLI Command Mapping

| MCP Tool | Lune CLI Equivalent |
|----------|---------------------|
| `lune_run("script.luau")` | `lune run script.luau` |
| `lune_run("script.luau", args=["--flag", "value"])` | `lune run script.luau -- --flag value` |
| `lune_eval("print(1+1)")` | `echo "print(1+1)" > /tmp/eval.luau && lune run /tmp/eval.luau` |

---

## Lune Built-in Libraries Available

Scripts executed via `lune_run`/`lune_eval` have access to:

| Library | Purpose | Example |
|---------|---------|---------|
| `@lune/fs` | Filesystem operations | `require("@lune/fs").readFile("data.json")` |
| `@lune/net` | HTTP requests | `require("@lune/net").request({url = "..."})` |
| `@lune/process` | Process spawning | `require("@lune/process").spawn("git", {"status"})` |
| `@lune/task` | Task scheduling | `require("@lune/task").wait(1)` |
| `@lune/serde` | JSON/YAML/TOML | `require("@lune/serde").decode("json", data)` |
| `@lune/roblox` | .rbxl/.rbxm manipulation | `require("@lune/roblox").readPlaceFile("game.rbxl")` |
| `@lune/regex` | Regular expressions | `require("@lune/regex").new("\\d+")` |
| `@lune/stdio` | Terminal I/O | `require("@lune/stdio").prompt("confirm", "Continue?")` |
| `@lune/datetime` | Date/time handling | `require("@lune/datetime").now()` |
| `@lune/luau` | Luau utilities | `require("@lune/luau").compile(source)` |

---

## Error Handling

```rust
// In src/error.rs - no changes needed, reuse existing errors:

RobloxMcpError::ToolNotInstalled {
    tool: "lune".to_string(),
    install_hint: "Install via: rokit add lune-org/lune".to_string()
}

RobloxMcpError::ToolExecutionError {
    tool: "lune".to_string(),
    message: "Script exited with code 1: ...".to_string()
}
```

---

## Security Considerations

1. **Path Validation**: Use existing `validate_path()` to prevent traversal attacks
2. **Timeout Protection**: Use existing `execute_with_timeout()` (default 30s)
3. **No Network by Default**: Lune's `@lune/net` is available but sandboxed to localhost
4. **Temp File Cleanup**: For `lune_eval`, ensure temp files are deleted even on error

---

## Installation Requirements

```toml
# aftman.toml or rokit.toml
[tools]
lune = "lune-org/lune@0.9.0"
```

Or via cargo:
```bash
cargo install lune
```

---

## Example Usage Scenarios

### Scenario 1: Test a Utility Function

```
User: "Test if my lerp function works correctly"

Claude:
1. Writes test script to tests/lerp_test.luau
2. Calls lune_run("tests/lerp_test.luau")
3. Reports: "All 5 tests passed in 12ms"
```

### Scenario 2: Validate JSON Data Structure

```
User: "Check if my config.json is valid and has required fields"

Claude:
1. Calls lune_eval with:
   local fs = require("@lune/fs")
   local serde = require("@lune/serde")
   local config = serde.decode("json", fs.readFile("config.json"))
   assert(config.version, "Missing version field")
   assert(config.name, "Missing name field")
   print("Config valid!")
2. Reports success or specific missing field
```

### Scenario 3: Pre-sync Validation

```
User: "Before syncing, verify my module has no syntax errors"

Claude:
1. Calls lune_eval with:
   local luau = require("@lune/luau")
   local source = require("@lune/fs").readFile("src/MyModule.luau")
   local success, err = pcall(luau.compile, source)
   if not success then error("Syntax error: " .. err) end
   print("Syntax valid!")
2. Either syncs to Studio or reports syntax errors
```

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `src/tools/lune.rs` | CREATE | LuneRunner trait, DefaultLuneRunner, MockLuneRunner |
| `src/tools/mod.rs` | MODIFY | Add `pub mod lune` export |
| `src/mcp/params.rs` | MODIFY | Add LuneRunParams, LuneEvalParams |
| `src/mcp/tools/toolchain.rs` | MODIFY | Add lune_run_impl, lune_eval_impl |
| `src/mcp/server.rs` | MODIFY | Add LN generic, lune field, tool registrations |
| `tests/toolchain_integration.rs` | MODIFY | Add Lune integration tests |
| `docs/API_REFERENCE.md` | MODIFY | Document new tools |
| `CLAUDE.md` | MODIFY | Update tool count (44 → 46) |

---

## Estimated Total Effort

| Phase | Time |
|-------|------|
| Core Infrastructure | 2-3 hours |
| MCP Tool Implementations | 2 hours |
| Server Type Updates | 1 hour |
| Testing | 2 hours |
| Documentation | 0.5 hours |
| **Total** | **7-8 hours** |

---

## Success Criteria

- [ ] `lune_run` executes scripts and returns structured results
- [ ] `lune_eval` evaluates inline code without temp file leaks
- [ ] Path traversal attacks are blocked
- [ ] Timeout protection prevents runaway scripts
- [ ] Mock infrastructure enables testing without Lune binary
- [ ] All existing tests continue to pass
- [ ] Tool count updated to 46 in documentation

---

## References

- [Lune GitHub](https://github.com/lune-org/lune)
- [Lune Documentation](https://lune-org.github.io/docs)
- Existing patterns: `src/tools/rojo.rs`, `src/tools/wally.rs`
