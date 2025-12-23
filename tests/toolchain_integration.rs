//! Integration tests for CLI toolchain (StyLua, Rojo, Wally, Selene)
//!
//! These tests invoke actual CLI binaries and require the tools to be installed.
//!
//! **NOTE**: These tests are marked with `#[ignore]` to skip them in CI by default.
//! They require external toolchain binaries to be installed first.
//!
//! Run with:
//! ```bash
//! # Ensure aftman tools are in PATH
//! export PATH="$HOME/.aftman/bin:$PATH"
//!
//! # Run ignored tests explicitly
//! cargo test --test toolchain_integration -- --ignored --test-threads=1
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
        home.join(".local")
            .join("share")
            .join("wally")
            .join("index"),
        home.join("AppData")
            .join("Local")
            .join("wally")
            .join("index"), // Windows
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
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
#[ignore]
fn test_rojo_build_with_formatted_scripts() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    setup_aftman_config(temp.path());
    let project_path = create_rojo_project(temp.path());

    // Get the script path from the project
    let script_path = temp
        .path()
        .join("src")
        .join("server")
        .join("Main.server.luau");

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
#[ignore]
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

    assert!(output.status.success(), "stylua should handle large files");
    assert!(
        duration.as_secs() < 30,
        "stylua should complete within 30 seconds for large file"
    );
}

#[test]
#[ignore]
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

// ============================================================================
// Unit Tests for Utility Functions
// ============================================================================
// These tests validate the helper functions themselves without requiring
// external toolchain binaries to be installed.

#[test]
fn test_project_root_returns_valid_path() {
    let root = project_root();
    assert!(root.exists(), "Project root should exist");
    assert!(root.is_dir(), "Project root should be a directory");
    
    // Verify it contains expected project files
    assert!(root.join("Cargo.toml").exists(), "Project root should contain Cargo.toml");
}

#[test]
fn test_aftman_bin_returns_expected_path() {
    let bin_path = aftman_bin();
    
    // Verify path structure (doesn't need to exist)
    assert!(bin_path.ends_with(".aftman/bin"), "Should end with .aftman/bin");
    assert!(bin_path.is_absolute(), "Should be an absolute path");
}

#[test]
fn test_tool_path_selene_special_case() {
    let selene_path = tool_path("selene");
    
    // Selene is installed via cargo, not aftman
    assert!(
        selene_path.to_string_lossy().contains(".cargo") || 
        selene_path.to_string_lossy().contains("cargo"),
        "Selene path should contain .cargo or cargo: {:?}",
        selene_path
    );
    
    #[cfg(windows)]
    {
        assert!(selene_path.ends_with("selene.exe"), "Windows should have .exe extension");
    }
    
    #[cfg(not(windows))]
    {
        assert!(selene_path.ends_with("selene"), "Unix should not have .exe extension");
    }
}

#[test]
fn test_tool_path_aftman_tools() {
    let tools = vec!["stylua", "rojo", "wally"];
    
    for tool in tools {
        let tool_path_result = tool_path(tool);
        
        // Verify it's in the aftman bin directory
        assert!(
            tool_path_result.to_string_lossy().contains(".aftman"),
            "Tool {} should be in .aftman directory",
            tool
        );
        
        // Check platform-specific extension
        #[cfg(windows)]
        {
            assert!(
                tool_path_result.ends_with(format!("{}.exe", tool)),
                "Windows tool {} should have .exe extension",
                tool
            );
        }
        
        #[cfg(not(windows))]
        {
            assert!(
                tool_path_result.ends_with(tool),
                "Unix tool {} should not have .exe extension",
                tool
            );
        }
    }
}

#[test]
fn test_tool_path_case_sensitivity() {
    // Tool names should be case-sensitive
    let lower = tool_path("stylua");
    let mixed = tool_path("StyLua");
    
    // Different casing should produce different paths
    assert_ne!(lower, mixed, "Tool paths should be case-sensitive");
}

#[test]
fn test_setup_aftman_config_creates_file() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    setup_aftman_config(temp.path());
    
    let dest = temp.path().join("aftman.toml");
    let source = project_root().join("aftman.toml");
    
    if source.exists() {
        assert!(dest.exists(), "aftman.toml should be copied to temp dir");
        
        // Verify content matches
        let source_content = std::fs::read_to_string(&source).expect("Failed to read source");
        let dest_content = std::fs::read_to_string(&dest).expect("Failed to read dest");
        assert_eq!(source_content, dest_content, "Copied content should match source");
    } else {
        // If source doesn't exist, dest shouldn't be created
        assert!(!dest.exists(), "aftman.toml should not be created if source doesn't exist");
    }
}

#[test]
fn test_setup_aftman_config_handles_missing_source() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    // This should not panic even if aftman.toml doesn't exist
    // (though it should exist in this project)
    setup_aftman_config(temp.path());
    
    // Function should complete without error
}

#[test]
fn test_create_test_script_basic() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let content = "print('Hello, World!')";
    
    let script_path = create_test_script(temp.path(), "test.luau", content);
    
    assert!(script_path.exists(), "Script file should exist");
    assert!(script_path.is_file(), "Script path should be a file");
    
    let read_content = std::fs::read_to_string(&script_path).expect("Failed to read script");
    assert_eq!(read_content, content, "Script content should match");
}

#[test]
fn test_create_test_script_with_different_extensions() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extensions = vec!["luau", "lua", "server.luau", "client.luau"];
    
    for ext in extensions {
        let filename = format!("test.{}", ext);
        let path = create_test_script(temp.path(), &filename, "-- test");
        
        assert!(path.exists(), "Script with extension {} should exist", ext);
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), filename);
    }
}

#[test]
fn test_create_test_script_with_special_characters() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    // Test various content types
    let test_cases = vec![
        ("empty.luau", ""),
        ("newlines.luau", "line1\nline2\nline3"),
        ("unicode.luau", "print('Hello 世界 🌍')"),
        ("quotes.luau", r#"local str = "test\"quote""#),
        ("multiline.luau", "local x = [[\n  multiline\n  string\n]]"),
    ];
    
    for (name, content) in test_cases {
        let path = create_test_script(temp.path(), name, content);
        let read_content = std::fs::read_to_string(&path)
            .expect(&format!("Failed to read {}", name));
        assert_eq!(read_content, content, "Content mismatch for {}", name);
    }
}

#[test]
fn test_create_rojo_project_structure() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    let project_path = create_rojo_project(temp.path());
    
    // Verify project.json was created
    assert!(project_path.exists(), "Project JSON should exist");
    assert_eq!(
        project_path.file_name().unwrap().to_str().unwrap(),
        "default.project.json"
    );
    
    // Verify JSON is valid
    let content = std::fs::read_to_string(&project_path).expect("Failed to read project JSON");
    let parsed: serde_json::Value = serde_json::from_str(&content)
        .expect("Project JSON should be valid");
    
    // Verify expected structure
    assert_eq!(parsed["name"], "test-project");
    assert!(parsed["tree"].is_object(), "Should have a tree object");
    assert_eq!(parsed["tree"]["$className"], "DataModel");
    
    // Verify directory structure
    let src_dir = temp.path().join("src").join("server");
    assert!(src_dir.exists(), "src/server directory should exist");
    
    let script_path = src_dir.join("Main.server.luau");
    assert!(script_path.exists(), "Main.server.luau should exist");
    
    let script_content = std::fs::read_to_string(&script_path).expect("Failed to read script");
    assert_eq!(script_content, "print('Hello from server!')");
}

#[test]
fn test_create_rojo_project_json_validity() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let project_path = create_rojo_project(temp.path());
    
    let content = std::fs::read_to_string(&project_path).expect("Failed to read project");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");
    
    // Verify ServerScriptService structure
    assert!(parsed["tree"]["ServerScriptService"].is_object());
    assert_eq!(
        parsed["tree"]["ServerScriptService"]["$className"],
        "ServerScriptService"
    );
    assert!(parsed["tree"]["ServerScriptService"]["Main"].is_object());
}

#[test]
fn test_create_wally_project_basic() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    let wally_path = create_wally_project(temp.path());
    
    assert!(wally_path.exists(), "wally.toml should exist");
    assert_eq!(wally_path.file_name().unwrap().to_str().unwrap(), "wally.toml");
    
    let content = std::fs::read_to_string(&wally_path).expect("Failed to read wally.toml");
    
    // Verify expected content
    assert!(content.contains("[package]"), "Should have [package] section");
    assert!(content.contains("name = \"test/test-package\""));
    assert!(content.contains("version = \"0.1.0\""));
    assert!(content.contains("registry = \"https://github.com/UpliftGames/wally-index\""));
    assert!(content.contains("realm = \"shared\""));
    assert!(content.contains("[dependencies]"), "Should have [dependencies] section");
}

#[test]
fn test_create_wally_project_with_deps() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    let wally_path = create_wally_project_with_deps(temp.path());
    
    assert!(wally_path.exists(), "wally.toml with deps should exist");
    
    let content = std::fs::read_to_string(&wally_path).expect("Failed to read wally.toml");
    
    // Verify basic structure
    assert!(content.contains("[package]"));
    assert!(content.contains("[dependencies]"));
    
    // Verify Promise dependency
    assert!(
        content.contains("Promise = \"evaera/promise@4.0.0\""),
        "Should contain Promise dependency"
    );
}

#[test]
fn test_create_wally_project_difference_between_variants() {
    let temp1 = TempDir::new().expect("Failed to create temp dir 1");
    let temp2 = TempDir::new().expect("Failed to create temp dir 2");
    
    let basic = create_wally_project(temp1.path());
    let with_deps = create_wally_project_with_deps(temp2.path());
    
    let basic_content = std::fs::read_to_string(&basic).expect("Failed to read basic");
    let deps_content = std::fs::read_to_string(&with_deps).expect("Failed to read deps");
    
    // Basic should not have Promise dependency
    assert!(!basic_content.contains("Promise"));
    
    // With deps should have Promise dependency
    assert!(deps_content.contains("Promise"));
}

#[test]
fn test_create_invalid_wally_project() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    let wally_path = create_invalid_wally_project(temp.path());
    
    assert!(wally_path.exists(), "Invalid wally.toml should be created");
    
    let content = std::fs::read_to_string(&wally_path).expect("Failed to read wally.toml");
    
    // Verify it's intentionally invalid
    assert!(content.contains("name = \"invalid\""));
    assert!(content.contains("# Missing required fields"));
    
    // Verify it's missing required fields
    assert!(!content.contains("version ="), "Should not have version");
    assert!(!content.contains("registry ="), "Should not have registry");
    assert!(!content.contains("realm ="), "Should not have realm");
}

#[test]
fn test_create_selene_config_structure() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    let config_path = create_selene_config(temp.path());
    
    assert!(config_path.exists(), "selene.toml should exist");
    assert_eq!(config_path.file_name().unwrap().to_str().unwrap(), "selene.toml");
    
    let content = std::fs::read_to_string(&config_path).expect("Failed to read selene.toml");
    
    assert!(content.contains("std = \"roblox\""), "Should set std to roblox");
}

#[test]
fn test_clear_wally_index_locks_does_not_panic() {
    // This function should not panic even if directories don't exist
    clear_wally_index_locks();
    
    // Call it multiple times to ensure idempotency
    clear_wally_index_locks();
    clear_wally_index_locks();
}

#[test]
fn test_clear_wally_index_locks_handles_missing_home() {
    // Function should handle edge cases gracefully
    // This is a smoke test - the function uses dirs::home_dir() which may fail
    // but we expect it to not panic in normal circumstances
    clear_wally_index_locks();
}

#[test]
fn test_multiple_test_scripts_in_same_directory() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    let script1 = create_test_script(temp.path(), "script1.luau", "print(1)");
    let script2 = create_test_script(temp.path(), "script2.luau", "print(2)");
    let script3 = create_test_script(temp.path(), "script3.luau", "print(3)");
    
    // All should exist independently
    assert!(script1.exists());
    assert!(script2.exists());
    assert!(script3.exists());
    
    // Verify content
    assert_eq!(std::fs::read_to_string(&script1).unwrap(), "print(1)");
    assert_eq!(std::fs::read_to_string(&script2).unwrap(), "print(2)");
    assert_eq!(std::fs::read_to_string(&script3).unwrap(), "print(3)");
}

#[test]
fn test_create_test_script_overwrite_behavior() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    // Create initial script
    let path = create_test_script(temp.path(), "overwrite.luau", "original");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    
    // Overwrite with new content
    let path2 = create_test_script(temp.path(), "overwrite.luau", "modified");
    assert_eq!(path, path2, "Path should be the same");
    assert_eq!(std::fs::read_to_string(&path2).unwrap(), "modified");
}

#[test]
fn test_rojo_project_paths_are_relative() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let project_path = create_rojo_project(temp.path());
    
    let content = std::fs::read_to_string(&project_path).expect("Failed to read project");
    
    // Verify paths in JSON are relative, not absolute
    assert!(content.contains("src/server/Main.server.luau"));
    assert!(!content.contains(temp.path().to_str().unwrap()), 
        "Should not contain absolute temp path");
}

#[test]
fn test_wally_manifest_format_consistency() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    let basic = create_wally_project(temp.path().join("basic"));
    let with_deps = create_wally_project_with_deps(temp.path().join("deps"));
    let invalid = create_invalid_wally_project(temp.path().join("invalid"));
    
    // All should create wally.toml
    assert!(basic.ends_with("wally.toml"));
    assert!(with_deps.ends_with("wally.toml"));
    assert!(invalid.ends_with("wally.toml"));
}

#[test]
fn test_tool_path_empty_string() {
    let path = tool_path("");
    
    // Should return a path (even if tool name is empty)
    #[cfg(windows)]
    {
        assert!(path.ends_with(".exe"));
    }
    
    #[cfg(not(windows))]
    {
        assert!(path.to_string_lossy().contains(".aftman"));
    }
}

#[test]
fn test_create_rojo_project_directory_creation() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    // Subdirectories shouldn't exist initially
    let src_path = temp.path().join("src");
    assert!(!src_path.exists(), "src should not exist initially");
    
    create_rojo_project(temp.path());
    
    // Now they should exist
    assert!(src_path.exists(), "src should be created");
    assert!(src_path.join("server").exists(), "src/server should be created");
}

#[test]
fn test_wally_project_toml_parse_validity() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let wally_path = create_wally_project(temp.path());
    
    let content = std::fs::read_to_string(&wally_path).expect("Failed to read wally.toml");
    
    // Verify TOML section markers are properly formatted
    assert!(content.contains("[package]"));
    assert!(content.contains("[dependencies]"));
    
    // Verify no obvious syntax errors
    assert!(!content.contains("[["));  // No array tables in our format
    assert!(content.lines().filter(|l| l.trim().starts_with('[') && l.trim().ends_with(']')).count() == 2);
}

#[test]
fn test_create_test_script_with_subdirectory() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    
    // Create a subdirectory first
    let subdir = temp.path().join("nested");
    std::fs::create_dir(&subdir).expect("Failed to create subdir");
    
    let script_path = create_test_script(&subdir, "nested_script.luau", "print('nested')");
    
    assert!(script_path.exists());
    assert!(script_path.starts_with(&subdir));
}

#[test]
fn test_project_root_is_consistent() {
    // Multiple calls should return the same path
    let root1 = project_root();
    let root2 = project_root();
    let root3 = project_root();
    
    assert_eq!(root1, root2);
    assert_eq!(root2, root3);
}

#[test]
fn test_aftman_bin_is_consistent() {
    let bin1 = aftman_bin();
    let bin2 = aftman_bin();
    
    assert_eq!(bin1, bin2, "aftman_bin should return consistent paths");
}

#[test]
fn test_create_multiple_projects_independent() {
    let temp1 = TempDir::new().expect("Failed to create temp dir 1");
    let temp2 = TempDir::new().expect("Failed to create temp dir 2");
    
    let rojo1 = create_rojo_project(temp1.path());
    let rojo2 = create_rojo_project(temp2.path());
    
    // Both should exist independently
    assert!(rojo1.exists());
    assert!(rojo2.exists());
    assert_ne!(rojo1, rojo2);
    
    // Verify both have their own directory structures
    assert!(temp1.path().join("src/server").exists());
    assert!(temp2.path().join("src/server").exists());
}

#[test]
fn test_selene_config_minimal_content() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let config = create_selene_config(temp.path());
    
    let content = std::fs::read_to_string(&config).expect("Failed to read config");
    
    // Should be minimal (just std setting)
    let lines: Vec<&str> = content.lines().collect();
    assert!(lines.len() <= 3, "Config should be minimal");
}

#[test]
fn test_tool_path_with_special_characters() {
    // Test tool names with hyphens and underscores
    let tool1 = tool_path("test-tool");
    let tool2 = tool_path("test_tool");
    
    assert_ne!(tool1, tool2, "Different tool names should produce different paths");
    
    #[cfg(windows)]
    {
        assert!(tool1.ends_with("test-tool.exe"));
        assert!(tool2.ends_with("test_tool.exe"));
    }
}
