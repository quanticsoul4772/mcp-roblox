//! MCP Server Integration Tests
//!
//! Tests the STDIO transport end-to-end by spawning the server process
//! and sending JSON-RPC messages according to MCP protocol spec (2024-11-05).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;

/// JSON-RPC 2.0 Request structure
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

impl JsonRpcRequest {
    fn new(id: u64, method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        }
    }
}

/// JSON-RPC 2.0 Response structure
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Fields required for JSON deserialization but not always accessed
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Fields required for JSON deserialization but not always accessed
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

/// Helper to spawn the MCP server and communicate via STDIO
struct McpTestClient {
    child: Child,
    request_id: u64,
    temp_dir: TempDir,
}

impl McpTestClient {
    /// Spawn the server in the given temp directory
    fn spawn() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;

        // Build path to the binary
        let binary_path = Self::find_binary()?;

        // Spawn the server process in the temp directory
        // The server uses current_dir as project_root for path validation
        let child = Command::new(&binary_path)
            .current_dir(temp_dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // Suppress log output
            .spawn()?;

        Ok(Self {
            child,
            request_id: 0,
            temp_dir,
        })
    }

    /// Get the server's working directory (where files should be created)
    fn working_dir(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    /// Find the compiled binary
    fn find_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Try debug first, then release
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let debug_path = PathBuf::from(manifest_dir)
            .join("target")
            .join("debug")
            .join("roblox-studio-mcp.exe");

        if debug_path.exists() {
            return Ok(debug_path);
        }

        let release_path = PathBuf::from(manifest_dir)
            .join("target")
            .join("release")
            .join("roblox-studio-mcp.exe");

        if release_path.exists() {
            return Ok(release_path);
        }

        // Try without .exe for Unix
        let debug_unix = PathBuf::from(manifest_dir)
            .join("target")
            .join("debug")
            .join("roblox-studio-mcp");

        if debug_unix.exists() {
            return Ok(debug_unix);
        }

        Err("Binary not found. Run `cargo build` first.".into())
    }

    /// Send a JSON-RPC request and get the response
    fn send_request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<JsonRpcResponse, Box<dyn std::error::Error>> {
        self.request_id += 1;
        let request = JsonRpcRequest::new(self.request_id, method, params);

        let stdin = self.child.stdin.as_mut().ok_or("No stdin")?;
        let request_json = serde_json::to_string(&request)?;

        // Write request with newline delimiter
        writeln!(stdin, "{request_json}")?;
        stdin.flush()?;

        // Read response (with timeout would be better, but keeping simple)
        let stdout = self.child.stdout.as_mut().ok_or("No stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut response_line = String::new();

        // Read until we get a complete JSON response
        reader.read_line(&mut response_line)?;

        let response: JsonRpcResponse = serde_json::from_str(&response_line)?;
        Ok(response)
    }

    /// Send initialize request per MCP protocol
    fn initialize(&mut self) -> Result<JsonRpcResponse, Box<dyn std::error::Error>> {
        self.send_request(
            "initialize",
            Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "roots": {
                        "listChanged": true
                    }
                },
                "clientInfo": {
                    "name": "mcp-integration-test",
                    "version": "1.0.0"
                }
            })),
        )
    }

    /// Send initialized notification (no response expected, but rmcp may respond)
    fn send_initialized(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let stdin = self.child.stdin.as_mut().ok_or("No stdin")?;
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        writeln!(stdin, "{}", serde_json::to_string(&notification)?)?;
        stdin.flush()?;
        Ok(())
    }

    /// Send a JSON-RPC notification (no response expected)
    fn send_notification(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(json!({}))
        });

        let stdin = self.child.stdin.as_mut().ok_or("No stdin")?;
        writeln!(stdin, "{}", serde_json::to_string(&notification)?)?;
        stdin.flush()?;
        Ok(())
    }

    /// List available tools
    fn list_tools(&mut self) -> Result<JsonRpcResponse, Box<dyn std::error::Error>> {
        self.send_request("tools/list", None)
    }

    /// Call a tool
    fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<JsonRpcResponse, Box<dyn std::error::Error>> {
        self.send_request(
            "tools/call",
            Some(json!({
                "name": name,
                "arguments": arguments
            })),
        )
    }

    /// Create a test .luau file in the server's working directory
    /// Returns the relative path that can be used with the MCP tools
    fn create_test_file(&self, relative_path: &str, content: &str) -> std::io::Result<String> {
        let path = self.working_dir().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        // Return relative path for use with MCP tools
        Ok(relative_path.to_string())
    }

    /// Get the absolute path for a relative path in the server's working directory
    fn abs_path(&self, relative_path: &str) -> PathBuf {
        self.working_dir().join(relative_path)
    }
}

impl Drop for McpTestClient {
    fn drop(&mut self) {
        // Kill the server process
        // Log cleanup errors but don't panic - process may already be dead
        if let Err(e) = self.child.kill() {
            eprintln!("[test cleanup] Failed to kill MCP server process: {e}");
        }
        if let Err(e) = self.child.wait() {
            eprintln!("[test cleanup] Failed to wait for MCP server process: {e}");
        }
    }
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_mcp_initialize_handshake() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Send initialize request
    let response = client.initialize().expect("Initialize request failed");

    // Verify response structure
    assert!(
        response.error.is_none(),
        "Initialize returned error: {:?}",
        response.error
    );
    assert!(response.result.is_some(), "Initialize returned no result");

    let result = response.result.unwrap();

    // Check protocol version
    assert!(
        result.get("protocolVersion").is_some(),
        "Missing protocolVersion in response"
    );

    // Check server capabilities include tools
    let capabilities = result.get("capabilities").expect("Missing capabilities");
    assert!(
        capabilities.get("tools").is_some(),
        "Server should advertise tools capability"
    );

    // Check server info
    let server_info = result.get("serverInfo").expect("Missing serverInfo");
    assert!(server_info.get("name").is_some(), "Missing server name");
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_tools_list_returns_filesystem_tools() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize first
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    // Small delay to ensure server is ready
    std::thread::sleep(Duration::from_millis(100));

    // List tools
    let response = client.list_tools().expect("tools/list failed");

    assert!(
        response.error.is_none(),
        "tools/list returned error: {:?}",
        response.error
    );

    let result = response.result.expect("No result from tools/list");
    let tools = result.get("tools").expect("No tools array in result");
    let tools_array = tools.as_array().expect("tools is not an array");

    // Should have 14 tools (6 filesystem + 8 studio)
    assert!(
        tools_array.len() >= 14,
        "Expected at least 14 tools, got {}",
        tools_array.len()
    );

    // Check for expected tool names
    let tool_names: Vec<&str> = tools_array
        .iter()
        .filter_map(|t| t.get("name")?.as_str())
        .collect();

    // Filesystem tools (6)
    let expected_fs_tools = [
        "fs_get_tree",
        "fs_read_script",
        "fs_write_script",
        "fs_delete_script",
        "fs_search_content",
        "fs_get_changes",
    ];

    // Studio tools (8)
    let expected_studio_tools = [
        "studio_get_selection",
        "studio_get_datamodel",
        "studio_get_script_source",
        "studio_modify_script",
        "studio_create_instance",
        "studio_set_property",
        "studio_delete_instance",
        "studio_find_instances",
    ];

    for expected in expected_fs_tools {
        assert!(
            tool_names.contains(&expected),
            "Missing filesystem tool: {expected}. Found: {tool_names:?}"
        );
    }

    for expected in expected_studio_tools {
        assert!(
            tool_names.contains(&expected),
            "Missing studio tool: {expected}. Found: {tool_names:?}"
        );
    }

    // Verify tools have required fields
    for tool in tools_array {
        assert!(tool.get("name").is_some(), "Tool missing name");
        assert!(
            tool.get("description").is_some(),
            "Tool missing description"
        );
        assert!(
            tool.get("inputSchema").is_some(),
            "Tool missing inputSchema"
        );
    }
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_fs_get_tree_tool() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    // Create some test files
    client
        .create_test_file("src/main.luau", "-- Main script")
        .expect("Failed to create test file");
    client
        .create_test_file("src/utils.luau", "-- Utils")
        .expect("Failed to create test file");

    std::thread::sleep(Duration::from_millis(100));

    // Call fs_get_tree
    let response = client
        .call_tool(
            "fs_get_tree",
            json!({
                "path": ".",
                "max_depth": 3
            }),
        )
        .expect("fs_get_tree call failed");

    assert!(
        response.error.is_none(),
        "fs_get_tree returned error: {:?}",
        response.error
    );

    let result = response.result.expect("No result from fs_get_tree");

    // Check for content array (MCP tool response format)
    let content = result.get("content").expect("No content in result");
    let content_array = content.as_array().expect("content is not an array");
    assert!(!content_array.is_empty(), "Content array is empty");

    // The text content should contain our files
    let text_content = content_array[0]
        .get("text")
        .expect("No text in content")
        .as_str()
        .expect("text is not a string");

    // Parse the JSON response - now includes tree + skipped info
    let response: Value = serde_json::from_str(text_content).expect("Failed to parse tree JSON");

    // Response should have tree, skipped, and skipped_count fields
    assert!(
        response.get("tree").is_some(),
        "Response should have tree field"
    );
    assert!(
        response.get("skipped").is_some(),
        "Response should have skipped field"
    );
    assert!(
        response.get("skipped_count").is_some(),
        "Response should have skipped_count field"
    );

    // The tree should have children
    let tree = response.get("tree").expect("No tree in response");
    assert!(tree.get("children").is_some(), "Tree should have children");
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_fs_read_script_tool() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    // Create a test file - returns relative path
    let test_content = "-- Test Luau Script\nlocal x = 42\nprint(x)";
    let relative_path = client
        .create_test_file("test_script.luau", test_content)
        .expect("Failed to create test file");

    std::thread::sleep(Duration::from_millis(100));

    // Call fs_read_script with relative path
    let response = client
        .call_tool(
            "fs_read_script",
            json!({
                "file_path": relative_path
            }),
        )
        .expect("fs_read_script call failed");

    assert!(
        response.error.is_none(),
        "fs_read_script returned error: {:?}",
        response.error
    );

    let result = response.result.expect("No result from fs_read_script");
    let content = result.get("content").expect("No content");
    let text = content[0].get("text").expect("No text").as_str().unwrap();

    // Parse the script content response
    let script_data: Value = serde_json::from_str(text).expect("Failed to parse response");

    assert_eq!(
        script_data.get("content").unwrap().as_str().unwrap(),
        test_content
    );
    assert_eq!(
        script_data.get("size_bytes").unwrap().as_u64().unwrap(),
        test_content.len() as u64
    );
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_fs_write_script_tool() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    std::thread::sleep(Duration::from_millis(100));

    let new_content = "-- New script created via MCP\nreturn {}";
    let relative_path = "scripts/output.luau";

    // Call fs_write_script with relative path, creating directories
    let response = client
        .call_tool(
            "fs_write_script",
            json!({
                "file_path": relative_path,
                "content": new_content,
                "create_directories": true
            }),
        )
        .expect("fs_write_script call failed");

    assert!(
        response.error.is_none(),
        "fs_write_script returned error: {:?}",
        response.error
    );

    // Verify file was created in server's working directory
    let output_path = client.abs_path(relative_path);
    assert!(output_path.exists(), "Output file was not created");

    let written_content = std::fs::read_to_string(&output_path).expect("Failed to read output");
    assert_eq!(written_content, new_content);
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_fs_search_content_tool() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    // Create test files with searchable content
    client
        .create_test_file(
            "scripts/player.luau",
            "-- Player module\nlocal Player = {}\nfunction Player:new() end",
        )
        .expect("Failed to create test file");
    client
        .create_test_file(
            "scripts/enemy.luau",
            "-- Enemy module\nlocal Enemy = {}\nfunction Enemy:spawn() end",
        )
        .expect("Failed to create test file");

    std::thread::sleep(Duration::from_millis(100));

    // Search for "function"
    let response = client
        .call_tool(
            "fs_search_content",
            json!({
                "path": ".",
                "pattern": "function",
                "extension": "luau"
            }),
        )
        .expect("fs_search_content call failed");

    assert!(
        response.error.is_none(),
        "fs_search_content returned error: {:?}",
        response.error
    );

    let result = response.result.expect("No result");
    let content = result.get("content").expect("No content");
    let text = content[0].get("text").expect("No text").as_str().unwrap();

    let search_result: Value = serde_json::from_str(text).expect("Failed to parse response");

    // Should find matches in both files
    let matches = search_result.get("matches").unwrap().as_u64().unwrap();
    assert!(matches >= 2, "Expected at least 2 matches, got {matches}");
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_fs_delete_script_tool() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    // Create a file to delete - returns relative path
    let relative_path = client
        .create_test_file("to_delete.luau", "-- Will be deleted")
        .expect("Failed to create test file");

    let abs_path = client.abs_path(&relative_path);
    assert!(abs_path.exists(), "Test file should exist before deletion");

    std::thread::sleep(Duration::from_millis(100));

    // Delete the file using relative path
    let response = client
        .call_tool(
            "fs_delete_script",
            json!({
                "file_path": relative_path
            }),
        )
        .expect("fs_delete_script call failed");

    assert!(
        response.error.is_none(),
        "fs_delete_script returned error: {:?}",
        response.error
    );

    // Verify file was deleted
    assert!(!abs_path.exists(), "File should have been deleted");
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_path_traversal_protection() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    std::thread::sleep(Duration::from_millis(100));

    // Try to read outside project root
    let response = client
        .call_tool(
            "fs_read_script",
            json!({
                "file_path": "../../../etc/passwd.luau"
            }),
        )
        .expect("Request failed");

    // Should fail with an error (path traversal or file not found)
    // The response will have isError: true in content or error in response
    let is_protected = response.error.is_some()
        || response
            .result
            .as_ref()
            .and_then(|r| r.get("isError"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        || response
            .result
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.contains("traversal") || s.contains("Invalid") || s.contains("error"))
            .unwrap_or(false);

    assert!(
        is_protected,
        "Path traversal should be blocked. Response: {:?}",
        response
    );
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_non_luau_file_rejection() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    // Create a non-.luau file in the server's working directory
    let txt_path = client.working_dir().join("test.txt");
    std::fs::write(&txt_path, "Not a Luau file").expect("Failed to create txt file");

    std::thread::sleep(Duration::from_millis(100));

    // Try to read non-.luau file using relative path
    let response = client
        .call_tool(
            "fs_read_script",
            json!({
                "file_path": "test.txt"
            }),
        )
        .expect("Request failed");

    // Should fail - only .luau files are supported
    let has_error = response.error.is_some()
        || response
            .result
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.contains("luau") || s.contains("supported"))
            .unwrap_or(false);

    assert!(
        has_error,
        "Non-.luau files should be rejected. Response: {:?}",
        response
    );
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_studio_tool_returns_timeout_when_plugin_not_connected() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Initialize
    client.initialize().expect("Initialize failed");
    client
        .send_initialized()
        .expect("Initialized notification failed");

    std::thread::sleep(Duration::from_millis(100));

    // Call studio tool without plugin connected - should timeout immediately
    // because the plugin heartbeat is stale (never updated)
    let response = client
        .call_tool("studio_get_selection", json!({}))
        .expect("Request failed");

    // Should fail with timeout error since plugin is not connected
    let has_timeout_error = response.error.is_some()
        || response
            .result
            .as_ref()
            .and_then(|r| r.get("isError"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        || response
            .result
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| {
                s.contains("timeout")
                    || s.contains("disconnected")
                    || s.contains("heartbeat")
                    || s.contains("plugin")
            })
            .unwrap_or(false);

    assert!(
        has_timeout_error,
        "Studio tool should fail with timeout when plugin not connected. Response: {:?}",
        response
    );
}

#[test]
#[ignore = "Requires compiled binary - run with: cargo test --test mcp_integration -- --ignored"]
fn test_server_bootstrap_and_tool_count() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Send initialize request (MCP protocol handshake)
    let response = client.initialize().expect("Failed to send initialize");

    // Verify server responds with its info
    let result = response.result.expect("Expected result from initialize");
    assert!(
        result.get("serverInfo").is_some(),
        "Missing serverInfo in initialize response"
    );
    assert!(
        result.get("protocolVersion").is_some(),
        "Missing protocolVersion in initialize response"
    );

    // Send initialized notification (completes handshake)
    client
        .send_notification("notifications/initialized", None)
        .expect("Failed to send initialized notification");

    // Small delay to ensure server is ready
    std::thread::sleep(Duration::from_millis(100));

    // Verify we can list tools after initialization
    let tools_response = client.list_tools().expect("Failed to list tools");

    let tools = tools_response
        .result
        .expect("Expected tools result")
        .get("tools")
        .expect("Expected tools array")
        .as_array()
        .expect("Tools should be array")
        .clone();

    // Should have all 24 tools (7 fs + 10 studio + 5 cloud + 2 monitoring)
    assert!(
        tools.len() >= 24,
        "Expected at least 24 tools, got {}. Tools: {:?}",
        tools.len(),
        tools
            .iter()
            .filter_map(|t| t.get("name"))
            .collect::<Vec<_>>()
    );

    // Verify all tool categories are present
    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name")?.as_str())
        .collect();

    // Check for key tools from each category
    assert!(tool_names.contains(&"fs_get_tree"), "Missing fs_get_tree");
    assert!(
        tool_names.contains(&"studio_health_check"),
        "Missing studio_health_check"
    );
    assert!(
        tool_names.contains(&"cloud_datastore_get"),
        "Missing cloud_datastore_get"
    );
    assert!(
        tool_names.contains(&"server_get_metrics"),
        "Missing server_get_metrics"
    );
}
