use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

// === FILESYSTEM PARAMS ===
// These parameter structs define the JSON schema for filesystem MCP tools

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsGetTreeParams {
    #[schemars(description = "Root path to explore")]
    pub path: String,
    #[schemars(description = "Maximum depth (default: 5)")]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsReadScriptParams {
    #[schemars(description = "Path to .luau file")]
    pub file_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsWriteScriptParams {
    #[schemars(description = "Path to .luau file")]
    pub file_path: String,
    #[schemars(description = "Script content")]
    pub content: String,
    #[schemars(description = "Create parent directories if missing")]
    pub create_directories: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsDeleteScriptParams {
    #[schemars(description = "Path to .luau file to delete")]
    pub file_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsSearchContentParams {
    #[schemars(description = "Directory to search")]
    pub path: String,
    #[schemars(description = "Regex pattern to match")]
    pub pattern: String,
    #[schemars(description = "File extension filter (e.g., 'luau') - REQUIRED")]
    pub extension: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsGetChangesParams {
    #[schemars(description = "Root path to scan for file modification times")]
    pub path: String,
}

// === STUDIO PARAMS ===
// These parameter structs define the JSON schema for Studio bridge MCP tools
// All Studio tools communicate with Roblox Studio via the HTTP plugin bridge

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetDataModelParams {
    #[schemars(description = "Maximum depth to traverse (default: 3)")]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetDataModelPaginatedParams {
    #[schemars(description = "Maximum depth to traverse (default: 3)")]
    pub max_depth: Option<usize>,
    #[schemars(description = "Starting path for pagination (e.g., 'game.Workspace')")]
    pub start_path: Option<String>,
    #[schemars(description = "Maximum instances to return (default: 500, max: 1000)")]
    pub limit: Option<usize>,
    #[schemars(description = "Cursor from previous response for continuation")]
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetScriptSourceParams {
    #[schemars(description = "Full path to script (e.g., 'game.ServerScriptService.Main')")]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioModifyScriptParams {
    #[schemars(description = "Full path to script")]
    pub path: String,
    #[schemars(description = "New script content")]
    pub new_source: String,
    #[schemars(description = "Record undo waypoint (default: true)")]
    pub record_undo: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioCreateInstanceParams {
    #[schemars(description = "Roblox class name (e.g., 'Part', 'Script')")]
    pub class_name: String,
    #[schemars(description = "Parent path (e.g., 'game.Workspace')")]
    pub parent: String,
    #[schemars(description = "Instance name")]
    pub name: String,
    #[schemars(description = "Properties to set as JSON object")]
    pub properties: Option<serde_json::Value>,
    #[schemars(description = "Record undo waypoint (default: true)")]
    pub record_undo: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioSetPropertyParams {
    #[schemars(description = "Instance path (e.g., 'game.Workspace.Part')")]
    pub path: String,
    #[schemars(description = "Property name (e.g., 'Name', 'Position', 'BrickColor')")]
    pub property: String,
    #[schemars(description = "Property value (type depends on property)")]
    pub value: serde_json::Value,
    #[schemars(description = "Record undo waypoint (default: true)")]
    pub record_undo: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioDeleteInstanceParams {
    #[schemars(description = "Instance path to delete")]
    pub path: String,
    #[schemars(description = "Record undo waypoint (default: true)")]
    pub record_undo: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioFindInstancesParams {
    #[schemars(description = "Class name to find (e.g., 'Part', 'Script', 'ModuleScript')")]
    pub class_name: String,
    #[schemars(description = "Root to search from (default: 'game')")]
    pub root: Option<String>,
}

// === CLOUD PARAMS ===
// These parameter structs define the JSON schema for Open Cloud MCP tools
// All Cloud tools require ROBLOX_OPEN_CLOUD_API_KEY environment variable

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudPublishPlaceParams {
    #[schemars(description = "Universe ID from Roblox Creator Dashboard")]
    pub universe_id: u64,
    #[schemars(description = "Place ID to publish to")]
    pub place_id: u64,
    #[schemars(description = "Path to .rbxl file")]
    pub rbxl_path: String,
}

// === EXTENDED CLOUD PARAMS ===
// Phase 4: Asset upload and DataStore access

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudUploadAssetParams {
    #[schemars(description = "Asset type: 'image', 'model', or 'audio'")]
    pub asset_type: String,
    #[schemars(description = "Path to asset file")]
    pub file_path: String,
    #[schemars(description = "Display name for the asset")]
    pub name: String,
    #[schemars(description = "Asset description")]
    pub description: String,
    #[schemars(description = "Creator user ID")]
    pub creator_id: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudDatastoreGetParams {
    #[schemars(description = "Universe ID containing the DataStore")]
    pub universe_id: u64,
    #[schemars(description = "Name of the DataStore")]
    pub datastore_name: String,
    #[schemars(description = "Entry key to retrieve")]
    pub key: String,
    #[schemars(description = "Scope (default: 'global')")]
    pub scope: Option<String>,
}

// === LINTING PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsLintScriptParams {
    #[schemars(description = "Path to .luau file to lint")]
    pub file_path: String,
    #[schemars(description = "Path to selene.toml configuration file (optional)")]
    pub config_path: Option<String>,
}

// === WATCHER PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsWatchChangesParams {
    #[schemars(description = "Maximum number of changes to return (default: 100)")]
    pub limit: Option<usize>,
}
