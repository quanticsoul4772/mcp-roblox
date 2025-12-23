//! Integration tests for CLI toolchain (StyLua, Rojo, Wally, Selene)
//!
//! These tests invoke actual CLI binaries and require the tools to be installed.
//!
//! Run with:
//! ```bash
//! # Ensure aftman tools are in PATH
//! export PATH="$HOME/.aftman/bin:$PATH"
//!
//! # Run tests serially (required for wally tests due to global index lock)
//! cargo test --test toolchain_integration -- --test-threads=1
//! ```
//!
//! Tool installation:
//! - `aftman install` (installs StyLua, Rojo, Wally from aftman.toml)
//! - `cargo install selene` (linter)

use serial_test::serial;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

// ============================================================================
// Test Utilities
// ============================================================================

/// Get the project root directory (where aftman.toml lives)
fn project_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Get the aftman bin directory
fn aftman_bin() -> std::path::PathBuf {
    dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".aftman")
        .join("bin")
}

/// Get the path to an aftman-managed tool
fn tool_path(name: &str) -> std::path::PathBuf {
    // Selene is installed via cargo, not aftman
    if name == "selene" {
        let cargo_bin = dirs::home_dir()
            .expect("Failed to get home directory")
            .join(".cargo")
            .join("bin");
        if cfg!(windows) {
            return cargo_bin.join("selene.exe");
        } else {
            return cargo_bin.join("selene");
        }
    }

    let bin = aftman_bin();
    if cfg!(windows) {
        bin.join(format!("{}.exe", name))
    } else {
        bin.join(name)
    }
}

/// Copy aftman.toml to a directory so aftman-managed tools work there
fn setup_aftman_config(dir: &Path) {
    let source = project_root().join("aftman.toml");
    let dest = dir.join("aftman.toml");
    if source.exists() {
        std::fs::copy(&source, &dest).expect("Failed to copy aftman.toml");
    }
}

/// Create a test Luau script file
fn create_test_script(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("Failed to write test script");
    path
}

/// Create a minimal Rojo project structure
fn create_rojo_project(dir: &Path) -> std::path::PathBuf {
    let project_json = dir.join("default.project.json");
    // Use $path pointing to a folder without conflicting $className
    let content = r#"{
  "name": "test-project",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": {
      "$className": "ServerScriptService",
      "Main": {
        "$path": "src/server/Main.server.luau"
      }
    }
  }
}"#;
    std::fs::write(&project_json, content).expect("Failed to write project.json");

    // Create the source directory
    let src_dir = dir.join("src").join("server");
    std::fs::create_dir_all(&src_dir).expect("Failed to create src/server");

    // Create a sample script (not init.server.luau to avoid Rojo confusion)
    let script_path = src_dir.join("Main.server.luau");
    std::fs::write(&script_path, "print('Hello from server!')").expect("Failed to write script");

    project_json
}

/// Create a minimal Wally project structure
fn create_wally_project(dir: &Path) -> std::path::PathBuf {
    let wally_toml = dir.join("wally.toml");
    let content = r#"[package]
name = "test/test-package"
version = "0.1.0"
registry = "https://github.com/UpliftGames/wally-index"
realm = "shared"

[dependencies]
"#;
    std::fs::write(&wally_toml, content).expect("Failed to write wally.toml");
    wally_toml
}

/// Create a Wally project with dependencies for install testing
fn create_wally_project_with_deps(dir: &Path) -> std::path::PathBuf {
    let wally_toml = dir.join("wally.toml");
    let content = r#"[package]
name = "test/test-package"
version = "0.1.0"
registry = "https://github.com/UpliftGames/wally-index"
realm = "shared"

[dependencies]
Promise = "evaera/promise@4.0.0"
"#;
    std::fs::write(&wally_toml, content).expect("Failed to write wally.toml");
    wally_toml
}

/// Create an invalid Wally manifest for error testing
fn create_invalid_wally_project(dir: &Path) -> std::path::PathBuf {
    let wally_toml = dir.join("wally.toml");
    let content = r#"[package]
name = "invalid"
# Missing required fields
"#;
    std::fs::write(&wally_toml, content).expect("Failed to write wally.toml");
    wally_toml
}

/// Create a Selene configuration file
fn create_selene_config(dir: &Path) -> std::path::PathBuf {
    let selene_toml = dir.join("selene.toml");
    let content = r#"std = "roblox"
"#;
    std::fs::write(&selene_toml, content).expect("Failed to write selene.toml");
    selene_toml
}

/// Clear stale wally index lock files that can cause "index is locked" errors
///
/// Wally uses a global index at ~/.wally/index (or ~/.local/share/wally on some systems)
/// that can get locked if a previous wally process crashed or tests run concurrently.
fn clear_wally_index_locks() {
    let home = dirs::home_dir().expect("Failed to get home directory");

    // Try multiple possible wally index locations
    let possible_paths = [
        home.join(".wally").join("index"),
        home.join(".local").join("share").join("wally").join("index"),
        home.join("AppData").join("Local").join("wally").join("index"), // Windows
    ];

    for index_path in &possible_paths {
        if index_path.exists() {
            // Look for .git/index.lock files in subdirectories
            if let Ok(entries) = std::fs::read_dir(index_path) {
                for entry in entries.flatten() {
                    let lock_file = entry.path().join(".git").join("index.lock");
                    if lock_file.exists() {
                        let _ = std::fs::remove_file(&lock_file);
                    }
                }
            }
        }
    }
}

// ============================================================================
// StyLua Integration Tests
// ============================================================================

#[test]
fn test_stylua_format_unformatted_script() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let unformatted = r#"local x=1
local y=2
local z=x+y
print(z)"#;

    let script_path = create_test_script(temp.path(), "test.luau", unformatted);

    // Run stylua
    let output = Command::new(tool_path("stylua"))
        .arg(&script_path)
        .output()
        .expect("Failed to run stylua");

    assert!(output.status.success(), "stylua failed: {:?}", output);

    // Verify formatting was applied
    let formatted = std::fs::read_to_string(&script_path).expect("Failed to read formatted file");
    assert!(
        formatted.contains("local x = 1"),
        "Expected formatted output with spaces around '='"
    );
}

#[test]
fn test_stylua_check_mode_detects_unformatted() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let unformatted = "local x=1\nlocal y=2";
    let script_path = create_test_script(temp.path(), "test.luau", unformatted);

    // Run stylua in check mode
    let output = Command::new(tool_path("stylua"))
        .arg("--check")
        .arg(&script_path)
        .output()
        .expect("Failed to run stylua");

    // Check mode should return non-zero for unformatted code
    assert!(
        !output.status.success(),
        "stylua --check should fail for unformatted code"
    );
}

#[test]
fn test_stylua_check_mode_passes_formatted() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let formatted = "local x = 1\nlocal y = 2\n";
    let script_path = create_test_script(temp.path(), "test.luau", formatted);

    // Run stylua in check mode
    let output = Command::new(tool_path("stylua"))
        .arg("--check")
        .arg(&script_path)
        .output()
        .expect("Failed to run stylua");

    assert!(
        output.status.success(),
        "stylua --check should pass for formatted code"
    );
}

#[test]
fn test_stylua_with_config() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());

    // Create a custom stylua config
    let config_path = temp.path().join("stylua.toml");
    std::fs::write(
        &config_path,
        r#"column_width = 80
line_endings = "Unix"
indent_type = "Tabs"
indent_width = 4
"#,
    )
    .expect("Failed to write stylua.toml");

    let script = "local x = 1\n";
    let script_path = create_test_script(temp.path(), "test.luau", script);

    // Run stylua with config
    let output = Command::new(tool_path("stylua"))
        .arg("--config-path")
        .arg(&config_path)
        .arg(&script_path)
        .output()
        .expect("Failed to run stylua");

    assert!(
        output.status.success(),
        "stylua with config should succeed: {:?}",
        output
    );
}

#[test]
fn test_stylua_handles_syntax_error() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let invalid_syntax = "local x = \nfunction end";
    let script_path = create_test_script(temp.path(), "test.luau", invalid_syntax);

    // Run stylua - should handle gracefully
    let output = Command::new(tool_path("stylua"))
        .arg(&script_path)
        .output()
        .expect("Failed to run stylua");

    // StyLua returns non-zero for syntax errors
    assert!(
        !output.status.success(),
        "stylua should fail on syntax errors"
    );
}

// ============================================================================
// Rojo Integration Tests
// ============================================================================

#[test]
fn test_rojo_build_project() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let project_path = create_rojo_project(temp.path());
    let output_path = temp.path().join("output.rbxl");

    // Run rojo build
    let output = Command::new(tool_path("rojo"))
        .arg("build")
        .arg(&project_path)
        .arg("--output")
        .arg(&output_path)
        .output()
        .expect("Failed to run rojo build");

    assert!(
        output.status.success(),
        "rojo build failed: {:?}\nstderr: {}",
        output,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_path.exists(), "Output file should be created");
}

#[test]
fn test_rojo_build_model() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let project_path = create_rojo_project(temp.path());
    let output_path = temp.path().join("output.rbxm");

    // Run rojo build for model output
    let output = Command::new(tool_path("rojo"))
        .arg("build")
        .arg(&project_path)
        .arg("--output")
        .arg(&output_path)
        .output()
        .expect("Failed to run rojo build");

    assert!(
        output.status.success(),
        "rojo build for model failed: {:?}",
        output
    );
    assert!(output_path.exists(), "Output model file should be created");
}

#[test]
fn test_rojo_sourcemap() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let project_path = create_rojo_project(temp.path());
    let sourcemap_path = temp.path().join("sourcemap.json");

    // Run rojo sourcemap
    let output = Command::new(tool_path("rojo"))
        .arg("sourcemap")
        .arg(&project_path)
        .arg("--output")
        .arg(&sourcemap_path)
        .output()
        .expect("Failed to run rojo sourcemap");

    assert!(
        output.status.success(),
        "rojo sourcemap failed: {:?}\nstderr: {}",
        output,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(sourcemap_path.exists(), "Sourcemap file should be created");

    // Verify it's valid JSON
    let content = std::fs::read_to_string(&sourcemap_path).expect("Failed to read sourcemap");
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("Sourcemap should be valid JSON");
    assert!(parsed.is_object(), "Sourcemap should be a JSON object");
}

#[test]
fn test_rojo_sourcemap_stdout() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let project_path = create_rojo_project(temp.path());

    // Run rojo sourcemap without output file (prints to stdout)
    let output = Command::new(tool_path("rojo"))
        .arg("sourcemap")
        .arg(&project_path)
        .output()
        .expect("Failed to run rojo sourcemap");

    assert!(
        output.status.success(),
        "rojo sourcemap to stdout failed: {:?}",
        output
    );

    // Verify stdout contains valid JSON
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("Sourcemap stdout should be valid JSON");
    assert!(parsed.is_object(), "Sourcemap should be a JSON object");
}

#[test]
fn test_rojo_invalid_project() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let invalid_project = temp.path().join("invalid.project.json");
    std::fs::write(&invalid_project, "{ invalid json }").expect("Failed to write invalid project");

    // Run rojo build with invalid project
    let output = Command::new(tool_path("rojo"))
        .arg("build")
        .arg(&invalid_project)
        .arg("--output")
        .arg(temp.path().join("output.rbxl"))
        .output()
        .expect("Failed to run rojo");

    assert!(
        !output.status.success(),
        "rojo build should fail with invalid project"
    );
}

// ============================================================================
// Wally Integration Tests
// ============================================================================

#[test]
#[serial]
fn test_wally_manifest_validation() {
    clear_wally_index_locks();

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let _wally_toml = create_wally_project(temp.path());

    // Run wally install (validates manifest)
    let output = Command::new(tool_path("wally"))
        .arg("install")
        .current_dir(temp.path())
        .output()
        .expect("Failed to run wally");

    assert!(
        output.status.success(),
        "wally install with valid manifest should succeed: {:?}\nstderr: {}",
        output,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[serial]
fn test_wally_install_dependencies() {
    clear_wally_index_locks();

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let _wally_toml = create_wally_project_with_deps(temp.path());

    // Run wally install
    let output = Command::new(tool_path("wally"))
        .arg("install")
        .current_dir(temp.path())
        .output()
        .expect("Failed to run wally install");

    assert!(
        output.status.success(),
        "wally install should succeed: {:?}\nstderr: {}",
        output,
        String::from_utf8_lossy(&output.stderr)
    );

    // Check Packages directory was created
    let packages_dir = temp.path().join("Packages");
    assert!(
        packages_dir.exists(),
        "Packages directory should be created"
    );
}

#[test]
#[serial]
fn test_wally_invalid_manifest() {
    clear_wally_index_locks();

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let _wally_toml = create_invalid_wally_project(temp.path());

    // Run wally install with invalid manifest
    let output = Command::new(tool_path("wally"))
        .arg("install")
        .current_dir(temp.path())
        .output()
        .expect("Failed to run wally");

    assert!(
        !output.status.success(),
        "wally install should fail with invalid manifest"
    );
}

// ============================================================================
// Selene (Linting) Integration Tests
// ============================================================================

#[test]
fn test_selene_lint_clean_script() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    create_selene_config(temp.path());

    let clean_script = r#"local Players = game:GetService("Players")

local function onPlayerAdded(player: Player)
    print("Player joined:", player.Name)
end

Players.PlayerAdded:Connect(onPlayerAdded)
"#;
    let script_path = create_test_script(temp.path(), "clean.luau", clean_script);

    // Run selene
    let output = Command::new(tool_path("selene"))
        .arg(&script_path)
        .current_dir(temp.path())
        .output()
        .expect("Failed to run selene");

    assert!(
        output.status.success(),
        "selene should pass for clean script: {:?}\nstdout: {}\nstderr: {}",
        output,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_selene_lint_with_warnings() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    create_selene_config(temp.path());

    // Script with unused variable (warning)
    let script_with_warnings = r#"local unused = 5
print("hello")
"#;
    let script_path = create_test_script(temp.path(), "warnings.luau", script_with_warnings);

    // Run selene
    let output = Command::new(tool_path("selene"))
        .arg(&script_path)
        .current_dir(temp.path())
        .output()
        .expect("Failed to run selene");

    // Selene returns non-zero for warnings by default
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("unused") || !output.status.success(),
        "selene should detect unused variable"
    );
}

#[test]
fn test_selene_with_config() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());

    // Create config that allows unused variables
    let config_path = temp.path().join("selene.toml");
    std::fs::write(
        &config_path,
        r#"std = "roblox"

[rules]
unused_variable = "allow"
"#,
    )
    .expect("Failed to write selene.toml");

    let script = "local unused = 5\nprint('hello')\n";
    let script_path = create_test_script(temp.path(), "test.luau", script);

    // Run selene with config
    let output = Command::new(tool_path("selene"))
        .arg("--config")
        .arg(&config_path)
        .arg(&script_path)
        .output()
        .expect("Failed to run selene");

    assert!(
        output.status.success(),
        "selene with allow rule should pass: {:?}",
        output
    );
}

// ============================================================================
// Cross-Tool Integration Tests
// ============================================================================

#[test]
fn test_format_then_lint_workflow() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    create_selene_config(temp.path());

    // Unformatted but valid script
    let script = r#"local Players=game:GetService("Players")
Players.PlayerAdded:Connect(function(player)
print("Player joined:",player.Name)
end)"#;
    let script_path = create_test_script(temp.path(), "workflow.luau", script);

    // Step 1: Format with stylua
    let format_output = Command::new(tool_path("stylua"))
        .arg(&script_path)
        .output()
        .expect("Failed to run stylua");
    assert!(format_output.status.success(), "stylua should succeed");

    // Step 2: Lint with selene
    let lint_output = Command::new(tool_path("selene"))
        .arg(&script_path)
        .current_dir(temp.path())
        .output()
        .expect("Failed to run selene");

    assert!(
        lint_output.status.success(),
        "selene should pass after formatting: stdout: {}, stderr: {}",
        String::from_utf8_lossy(&lint_output.stdout),
        String::from_utf8_lossy(&lint_output.stderr)
    );
}

#[test]
fn test_rojo_build_with_formatted_scripts() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let project_path = create_rojo_project(temp.path());

    // Get the script path from the project
    let script_path = temp.path().join("src").join("server").join("Main.server.luau");

    // Format the script first
    let format_output = Command::new(tool_path("stylua"))
        .arg(&script_path)
        .output()
        .expect("Failed to run stylua");
    assert!(format_output.status.success(), "stylua should succeed");

    // Build the project
    let output_path = temp.path().join("output.rbxl");
    let build_output = Command::new(tool_path("rojo"))
        .arg("build")
        .arg(&project_path)
        .arg("--output")
        .arg(&output_path)
        .output()
        .expect("Failed to run rojo build");

    assert!(
        build_output.status.success(),
        "rojo build should succeed with formatted scripts"
    );
    assert!(output_path.exists(), "Output file should be created");
}

// ============================================================================
// Performance and Edge Case Tests
// ============================================================================

#[test]
fn test_stylua_large_file() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());

    // Generate a large script (1000+ lines)
    let mut large_script = String::new();
    for i in 0..1000 {
        large_script.push_str(&format!("local var{} = {}\n", i, i));
    }
    large_script.push_str("return {\n");
    for i in 0..1000 {
        large_script.push_str(&format!("    var{} = var{},\n", i, i));
    }
    large_script.push_str("}\n");

    let script_path = create_test_script(temp.path(), "large.luau", &large_script);

    // Run stylua - should complete in reasonable time
    let start = std::time::Instant::now();
    let output = Command::new(tool_path("stylua"))
        .arg(&script_path)
        .output()
        .expect("Failed to run stylua");
    let duration = start.elapsed();

    assert!(
        output.status.success(),
        "stylua should handle large files"
    );
    assert!(
        duration.as_secs() < 30,
        "stylua should complete within 30 seconds for large file"
    );
}

#[test]
fn test_rojo_nested_project_structure() {

    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());

    // Create a more complex project structure
    // Use proper Rojo patterns: $path pointing directly to files, not folders with $className conflicts
    let project_json = temp.path().join("default.project.json");
    let content = r#"{
  "name": "complex-project",
  "tree": {
    "$className": "DataModel",
    "ServerScriptService": {
      "$className": "ServerScriptService",
      "Services": {
        "$path": "src/server/Services"
      }
    },
    "ReplicatedStorage": {
      "$className": "ReplicatedStorage",
      "Shared": {
        "$path": "src/shared"
      }
    },
    "StarterPlayer": {
      "$className": "StarterPlayer",
      "StarterPlayerScripts": {
        "$className": "StarterPlayerScripts",
        "Client": {
          "$path": "src/client"
        }
      }
    }
  }
}"#;
    std::fs::write(&project_json, content).expect("Failed to write project.json");

    // Create nested directories with proper init files for folders
    std::fs::create_dir_all(temp.path().join("src/server/Services")).expect("Failed to create dir");
    std::fs::create_dir_all(temp.path().join("src/shared")).expect("Failed to create dir");
    std::fs::create_dir_all(temp.path().join("src/client")).expect("Failed to create dir");

    // Create init.luau files for each folder (makes them ModuleScripts)
    std::fs::write(
        temp.path().join("src/server/Services/init.luau"),
        "return {}",
    )
    .expect("Failed to write init.luau");
    std::fs::write(temp.path().join("src/shared/init.luau"), "return {}")
        .expect("Failed to write init.luau");
    std::fs::write(temp.path().join("src/client/init.luau"), "return {}")
        .expect("Failed to write init.luau");

    // Build the project
    let output_path = temp.path().join("output.rbxl");
    let output = Command::new(tool_path("rojo"))
        .arg("build")
        .arg(&project_json)
        .arg("--output")
        .arg(&output_path)
        .output()
        .expect("Failed to run rojo build");

    assert!(
        output.status.success(),
        "rojo build should handle nested structure: {:?}\nstderr: {}",
        output,
        String::from_utf8_lossy(&output.stderr)
    );
}
