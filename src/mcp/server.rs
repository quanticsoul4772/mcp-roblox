use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use regex::Regex;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde_json::json;
use tokio::fs;
use walkdir::WalkDir;

use crate::bridge::http::PluginBridge;
use crate::mcp::params::{
    // Filesystem params
    FsDeleteScriptParams, FsGetChangesParams, FsGetTreeParams, FsReadScriptParams,
    FsSearchContentParams, FsWriteScriptParams,
    // Studio params
    StudioCreateInstanceParams, StudioDeleteInstanceParams, StudioFindInstancesParams,
    StudioGetDataModelParams, StudioGetScriptSourceParams, StudioModifyScriptParams,
    StudioSetPropertyParams,
};
use crate::tools::filesystem::{build_tree, read_script, validate_path, write_script};

#[derive(Clone)]
pub struct RobloxMcpServer {
    tool_router: ToolRouter<Self>,
    bridge: Arc<PluginBridge>,
    project_root: PathBuf,
}

impl RobloxMcpServer {
    pub fn new(bridge: Arc<PluginBridge>, project_root: PathBuf) -> Self {
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
        }
    }
}

#[tool_router]
impl RobloxMcpServer {
    // === FILESYSTEM TOOLS (6) ===

    #[tool(description = "List project file structure with depth limits. Returns a tree of files and directories, plus any skipped entries.")]
    async fn fs_get_tree(
        &self,
        Parameters(params): Parameters<FsGetTreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // max_depth is optional with documented default - this is acceptable
        let max_depth = params.max_depth.unwrap_or(5);

        // build_tree now returns TreeBuildResult with both tree and skipped entries
        let result = build_tree(&validated_path, 0, max_depth)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Return both tree AND skipped entries so caller knows what was excluded
        let json = serde_json::to_string_pretty(&json!({
            "tree": result.tree,
            "skipped": result.skipped,
            "skipped_count": result.skipped.len()
        }))
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Read a Luau script file. Only .luau files are supported.")]
    async fn fs_read_script(
        &self,
        Parameters(params): Parameters<FsReadScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let script_content = read_script(&validated_path)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let json = serde_json::to_string_pretty(&script_content)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Write or create a Luau script file. Optionally create parent directories.")]
    async fn fs_write_script(
        &self,
        Parameters(params): Parameters<FsWriteScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // For new files, validate the parent directory is within project root
        let parent = path.parent().ok_or_else(|| {
            ErrorData::internal_error("Invalid file path: no parent directory".to_string(), None)
        })?;

        // Check if parent exists or if we're allowed to create it
        let create_dirs = params.create_directories.unwrap_or(false);

        if !parent.exists() && !create_dirs {
            return Err(ErrorData::internal_error(
                format!(
                    "Parent directory does not exist: {}. Set create_directories=true to create it.",
                    parent.display()
                ),
                None,
            ));
        }

        // If parent exists, validate it's within project root
        if parent.exists() {
            validate_path(parent, &self.project_root)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        } else {
            // Validate that the target would be within project root
            // by checking if the path starts with project_root
            let abs_path = if path.is_absolute() {
                path.clone()
            } else {
                self.project_root.join(&path)
            };

            if !abs_path.starts_with(&self.project_root) {
                return Err(ErrorData::internal_error(
                    format!("Path traversal detected: {}", abs_path.display()),
                    None,
                ));
            }
        }

        // Validate .luau extension
        if path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::internal_error(
                "Only .luau files are supported".to_string(),
                None,
            ));
        }

        let write_result = write_script(&path, &params.content, create_dirs)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let json = serde_json::to_string_pretty(&write_result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Delete a Luau script file. Only .luau files can be deleted.")]
    async fn fs_delete_script(
        &self,
        Parameters(params): Parameters<FsDeleteScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate .luau extension
        if validated_path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::internal_error(
                "Only .luau files can be deleted".to_string(),
                None,
            ));
        }

        // Check file exists
        if !validated_path.exists() {
            return Err(ErrorData::internal_error(
                format!("File not found: {}", validated_path.display()),
                None,
            ));
        }

        fs::remove_file(&validated_path).await.map_err(|e| {
            ErrorData::internal_error(format!("Failed to delete file: {e}"), None)
        })?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "deleted": validated_path.display().to_string(),
                "success": true
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Search for patterns in script files using regex. Returns matching lines with context.")]
    async fn fs_search_content(
        &self,
        Parameters(params): Parameters<FsSearchContentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Compile regex pattern
        let regex = Regex::new(&params.pattern).map_err(|e| {
            ErrorData::internal_error(format!("Invalid regex pattern: {e}"), None)
        })?;

        // Extension is REQUIRED (enforced by schema)
        let extension = params.extension.as_str();

        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut errors: Vec<serde_json::Value> = Vec::new();

        // Walk directory and search files - REPORT ALL ERRORS
        for entry in WalkDir::new(&validated_path).into_iter() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(json!({
                        "type": "enumeration_error",
                        "path": e.path().map(|p| p.display().to_string()),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            let entry_path = entry.path();

            // Skip directories
            if entry_path.is_dir() {
                continue;
            }

            // Check extension
            if entry_path.extension() != Some(std::ffi::OsStr::new(extension)) {
                continue;
            }

            // Skip hidden files - but REPORT that we're skipping
            if let Some(name) = entry_path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    errors.push(json!({
                        "type": "skipped_hidden",
                        "path": entry_path.display().to_string()
                    }));
                    continue;
                }
            }

            // Read and search file - REPORT READ FAILURES
            let content = match fs::read_to_string(entry_path).await {
                Ok(c) => c,
                Err(e) => {
                    errors.push(json!({
                        "type": "read_error",
                        "path": entry_path.display().to_string(),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            for (line_num, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(json!({
                        "file": entry_path.display().to_string(),
                        "line": line_num + 1,
                        "content": line.trim()
                    }));
                }
            }
        }

        // Always include errors in response so caller knows what was skipped
        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "matches": results.len(),
                "results": results,
                "errors": errors,
                "error_count": errors.len()
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Get file modification times for change detection. Returns a map of file paths to modification timestamps.")]
    async fn fs_get_changes(
        &self,
        Parameters(params): Parameters<FsGetChangesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let mut mtimes: HashMap<String, u64> = HashMap::new();
        let mut errors: Vec<serde_json::Value> = Vec::new();

        // Walk directory and collect mtimes - REPORT ALL ERRORS
        for entry in WalkDir::new(&validated_path).into_iter() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(json!({
                        "type": "enumeration_error",
                        "path": e.path().map(|p| p.display().to_string()),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            let entry_path = entry.path();

            // Skip directories
            if entry_path.is_dir() {
                continue;
            }

            // Only track .luau files
            if entry_path.extension() != Some(std::ffi::OsStr::new("luau")) {
                continue;
            }

            // Skip hidden files - but REPORT
            if let Some(name) = entry_path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    errors.push(json!({
                        "type": "skipped_hidden",
                        "path": entry_path.display().to_string()
                    }));
                    continue;
                }
            }

            // Get modification time - REPORT ALL FAILURES
            let metadata = match entry_path.metadata() {
                Ok(m) => m,
                Err(e) => {
                    errors.push(json!({
                        "type": "metadata_error",
                        "path": entry_path.display().to_string(),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            let modified = match metadata.modified() {
                Ok(m) => m,
                Err(e) => {
                    errors.push(json!({
                        "type": "modified_time_error",
                        "path": entry_path.display().to_string(),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            let duration = match modified.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(json!({
                        "type": "timestamp_error",
                        "path": entry_path.display().to_string(),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            mtimes.insert(entry_path.display().to_string(), duration.as_secs());
        }

        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "file_count": mtimes.len(),
                "files": mtimes,
                "errors": errors,
                "error_count": errors.len()
            })
            .to_string(),
        )]))
    }

    // === STUDIO TOOLS (8) ===
    // These tools communicate with Roblox Studio via the HTTP plugin bridge.
    // The plugin must be connected for these tools to work.

    #[tool(description = "Get currently selected instances in Roblox Studio. Returns array of selected instances with Name, ClassName, and Path.")]
    async fn studio_get_selection(&self) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command("getSelection", json!({}))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Explore the live Studio DataModel hierarchy. Returns nested structure of instances with Name, ClassName, Path, and Children.")]
    async fn studio_get_datamodel(
        &self,
        Parameters(params): Parameters<StudioGetDataModelParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "getDataModel",
                json!({ "maxDepth": params.max_depth.unwrap_or(3) }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Read script source from a script instance in Studio. Works with Script, LocalScript, and ModuleScript.")]
    async fn studio_get_script_source(
        &self,
        Parameters(params): Parameters<StudioGetScriptSourceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command("getScriptSource", json!({ "path": params.path }))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Modify script source in Studio with undo support. Creates a waypoint for undo/redo functionality.")]
    async fn studio_modify_script(
        &self,
        Parameters(params): Parameters<StudioModifyScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "modifyScript",
                json!({
                    "path": params.path,
                    "newSource": params.new_source,
                    "recordUndo": params.record_undo.unwrap_or(true)
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Create a new instance in Studio. Supports setting initial properties and creates an undo waypoint.")]
    async fn studio_create_instance(
        &self,
        Parameters(params): Parameters<StudioCreateInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "createInstance",
                json!({
                    "className": params.class_name,
                    "parent": params.parent,
                    "name": params.name,
                    "properties": params.properties,
                    "recordUndo": params.record_undo.unwrap_or(true)
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Set a property on an instance in Studio. Supports common property types and creates an undo waypoint.")]
    async fn studio_set_property(
        &self,
        Parameters(params): Parameters<StudioSetPropertyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "setProperty",
                json!({
                    "path": params.path,
                    "property": params.property,
                    "value": params.value,
                    "recordUndo": params.record_undo.unwrap_or(true)
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Delete an instance from Studio. Creates an undo waypoint for recovery.")]
    async fn studio_delete_instance(
        &self,
        Parameters(params): Parameters<StudioDeleteInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "deleteInstance",
                json!({
                    "path": params.path,
                    "recordUndo": params.record_undo.unwrap_or(true)
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Find all instances of a specific class in Studio. Searches descendants from the specified root.")]
    async fn studio_find_instances(
        &self,
        Parameters(params): Parameters<StudioFindInstancesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "findInstances",
                json!({
                    "className": params.class_name,
                    "root": params.root
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }
}

#[tool_handler]
impl ServerHandler for RobloxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Roblox Studio MCP Server. Provides 6 filesystem tools for .luau script management and 8 Studio bridge tools for live Roblox Studio interaction. Studio tools require the plugin to be connected."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
