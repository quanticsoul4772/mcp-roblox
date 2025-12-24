//! Filesystem tool implementations.
//!
//! Provides 8 tools for file operations on Luau scripts:
//! - `fs_get_tree` - List project structure with depth limits
//! - `fs_read_script` - Read .luau file contents
//! - `fs_write_script` - Create/write .luau files
//! - `fs_delete_script` - Delete .luau files
//! - `fs_search_content` - Search with regex patterns
//! - `fs_get_changes` - Get file modification timestamps
//! - `fs_lint_script` - Run Selene linter
//! - `fs_watch_changes` - Poll file watcher for changes

use std::collections::HashMap;
use std::path::PathBuf;

use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData;
use serde_json::json;
use tokio::fs;
use walkdir::WalkDir;

use crate::bridge::StudioBridge;
use crate::limits::{MAX_FILE_ENTRIES, MAX_SEARCH_RESULTS, MAX_TREE_ENTRIES};
use crate::mcp::params::{
    FsDeleteScriptParams, FsGetChangesParams, FsGetTreeParams, FsLintScriptParams,
    FsReadScriptParams, FsSearchContentParams, FsWatchChangesParams, FsWriteScriptParams,
};
use crate::mcp::server::RobloxMcpServer;
use crate::regex_safety::validate_regex_safety;
use crate::tools::filesystem::{build_tree, read_script, validate_path, write_script};
use crate::tools::formatting::Formatter;
use crate::tools::linting::Linter;
use crate::tools::moonwave::MoonwaveRunner;
use crate::tools::rojo::RojoRunner;
use crate::tools::wally::WallyRunner;

impl<B, L, F, R, W, M> RobloxMcpServer<B, L, F, R, W, M>
where
    B: StudioBridge + Clone + 'static,
    L: Linter + Clone + 'static,
    F: Formatter + Clone + 'static,
    R: RojoRunner + Clone + 'static,
    W: WallyRunner + Clone + 'static,
    M: MoonwaveRunner + Clone + 'static,
{
    // =========================================================================
    // fs_get_tree - List project file structure with depth limits
    // =========================================================================

    pub(crate) async fn fs_get_tree_impl(
        &self,
        params: FsGetTreeParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // max_depth default: 5 levels deep
        let max_depth = params.max_depth.unwrap_or(5);

        let result = build_tree(&validated_path, 0, max_depth)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let mut response = json!({
            "tree": result.tree,
            "skipped": result.skipped,
            "skipped_count": result.skipped.len()
        });

        if result.truncated {
            response["truncated"] = json!(true);
            response["limit"] = json!(MAX_TREE_ENTRIES);
            if let Some(total) = result.total_entries {
                response["total_entries"] = json!(total);
            }
            response["message"] = json!(format!(
                "Tree truncated at {} entries. Use max_depth parameter or more specific path.",
                MAX_TREE_ENTRIES
            ));
        }

        let json = serde_json::to_string(&response)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // =========================================================================
    // fs_read_script - Read a Luau script file
    // =========================================================================

    pub(crate) async fn fs_read_script_impl(
        &self,
        params: FsReadScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // Validate .luau extension
        if path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::invalid_params(
                "Only .luau files are supported",
                None,
            ));
        }

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

    // =========================================================================
    // fs_write_script - Write or create a Luau script file
    // =========================================================================

    pub(crate) async fn fs_write_script_impl(
        &self,
        params: FsWriteScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        let parent = path.parent().ok_or_else(|| {
            ErrorData::internal_error("Invalid file path: no parent directory".to_string(), None)
        })?;

        // create_directories default: false
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

        if parent.exists() {
            validate_path(parent, &self.project_root)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        } else {
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

    // =========================================================================
    // fs_delete_script - Delete a Luau script file
    // =========================================================================

    pub(crate) async fn fs_delete_script_impl(
        &self,
        params: FsDeleteScriptParams,
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

        fs::remove_file(&validated_path)
            .await
            .map_err(|e| ErrorData::internal_error(format!("Failed to delete file: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "deleted": validated_path.display().to_string(),
                "success": true
            })
            .to_string(),
        )]))
    }

    // =========================================================================
    // fs_search_content - Search for patterns in script files using regex
    // =========================================================================

    pub(crate) async fn fs_search_content_impl(
        &self,
        params: FsSearchContentParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate and compile regex pattern with DoS protection
        let regex = validate_regex_safety(&params.pattern)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

        let extension = params.extension.clone();

        // Move traversal to blocking thread pool
        let (results, errors, truncated) = tokio::task::spawn_blocking(move || {
            let mut results: Vec<serde_json::Value> = Vec::new();
            let mut errors: Vec<serde_json::Value> = Vec::new();
            let mut truncated = false;

            'outer: for entry in WalkDir::new(&validated_path).into_iter() {
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

                if entry_path.is_dir() {
                    continue;
                }

                if entry_path.extension() != Some(std::ffi::OsStr::new(&extension)) {
                    continue;
                }

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

                let content = match std::fs::read_to_string(entry_path) {
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
                        if results.len() >= MAX_SEARCH_RESULTS {
                            truncated = true;
                            break 'outer;
                        }
                        results.push(json!({
                            "file": entry_path.display().to_string(),
                            "line": line_num + 1,
                            "content": line.trim()
                        }));
                    }
                }
            }

            (results, errors, truncated)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("Task join error: {e}"), None))?;

        let mut response = json!({
            "matches": results.len(),
            "results": results,
            "errors": errors,
            "error_count": errors.len()
        });

        if truncated {
            response["truncated"] = json!(true);
            response["limit"] = json!(MAX_SEARCH_RESULTS);
            response["message"] = json!(format!(
                "Results truncated at {} matches. Refine your search pattern for more specific results.",
                MAX_SEARCH_RESULTS
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(
            response.to_string(),
        )]))
    }

    // =========================================================================
    // fs_get_changes - Get file modification times for change detection
    // =========================================================================

    pub(crate) async fn fs_get_changes_impl(
        &self,
        params: FsGetChangesParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Move traversal to blocking thread pool
        let (mtimes, errors, truncated) = tokio::task::spawn_blocking(move || {
            let mut mtimes: HashMap<String, u64> = HashMap::new();
            let mut errors: Vec<serde_json::Value> = Vec::new();
            let mut truncated = false;

            for entry in WalkDir::new(&validated_path).into_iter() {
                if mtimes.len() >= MAX_FILE_ENTRIES {
                    truncated = true;
                    break;
                }

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

                if entry_path.is_dir() {
                    continue;
                }

                if entry_path.extension() != Some(std::ffi::OsStr::new("luau")) {
                    continue;
                }

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

                let metadata = match std::fs::metadata(entry_path) {
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

            (mtimes, errors, truncated)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("Task join error: {e}"), None))?;

        let mut response = json!({
            "file_count": mtimes.len(),
            "files": mtimes,
            "errors": errors,
            "error_count": errors.len()
        });

        if truncated {
            response["truncated"] = json!(true);
            response["limit"] = json!(MAX_FILE_ENTRIES);
            response["message"] = json!(format!(
                "File list truncated at {} entries. Consider using a more specific path.",
                MAX_FILE_ENTRIES
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(
            response.to_string(),
        )]))
    }

    // =========================================================================
    // fs_lint_script - Run Selene linter on a Luau script file
    // =========================================================================

    pub(crate) async fn fs_lint_script_impl(
        &self,
        params: FsLintScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate .luau extension
        if validated_path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::internal_error(
                "Only .luau files can be linted".to_string(),
                None,
            ));
        }

        let config_path = params.config_path.map(PathBuf::from);

        let result = self
            .linter
            .lint(&validated_path, config_path.as_deref())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // fs_watch_changes - Poll for recent file changes from file watcher
    // =========================================================================

    pub(crate) async fn fs_watch_changes_impl(
        &self,
        params: FsWatchChangesParams,
    ) -> Result<CallToolResult, ErrorData> {
        let watcher = self.file_watcher.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "File watcher not available on this platform".to_string(),
                None,
            )
        })?;

        // limit default: 100 changes
        let limit = params.limit.unwrap_or(100);
        let changes = watcher.poll_changes(limit).await;
        let pending = watcher.pending_count().await;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&json!({
                "changes": changes,
                "returned_count": changes.len(),
                "pending_count": pending
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }
}
