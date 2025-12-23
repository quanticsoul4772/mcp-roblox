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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetOutputParams {
    #[schemars(description = "Maximum number of log entries to return (default: 100)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetPropertiesParams {
    #[schemars(description = "Instance path (e.g., 'game.Workspace.Part')")]
    pub path: String,
    #[schemars(
        description = "List of property names to read. If omitted, returns common properties for the class."
    )]
    pub properties: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetBoundsParams {
    #[schemars(
        description = "Instance path to a BasePart or Model (e.g., 'game.Workspace.MyPart')"
    )]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioInsertR15RigParams {
    #[schemars(description = "Parent path for the rig (e.g., 'game.ReplicatedStorage.NPCModels')")]
    pub parent: String,
    #[schemars(description = "Name for the created rig model")]
    pub name: String,
    #[schemars(description = "Record undo waypoint (default: true)")]
    pub record_undo: Option<bool>,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudDatastoreSetParams {
    #[schemars(description = "Universe ID containing the DataStore")]
    pub universe_id: u64,
    #[schemars(description = "Name of the DataStore")]
    pub datastore_name: String,
    #[schemars(description = "Entry key to set")]
    pub key: String,
    #[schemars(description = "JSON value to store")]
    pub value: serde_json::Value,
    #[schemars(description = "Scope (default: 'global')")]
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudMessagingPublishParams {
    #[schemars(description = "Universe ID to publish the message to")]
    pub universe_id: u64,
    #[schemars(description = "Topic name to publish to (servers subscribe to this topic)")]
    pub topic: String,
    #[schemars(description = "Message content (JSON value, will be stringified for delivery)")]
    pub message: serde_json::Value,
}

// === PHASE 1: ORDERED DATASTORE AND UNIVERSE PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudOrderedDatastoreListParams {
    #[schemars(description = "Universe ID containing the OrderedDataStore")]
    pub universe_id: u64,
    #[schemars(description = "Name of the OrderedDataStore")]
    pub datastore_name: String,
    #[schemars(description = "Scope within the datastore (default: 'global')")]
    pub scope: Option<String>,
    #[schemars(description = "Maximum entries to return per page (1-100, default: 100)")]
    pub max_page_size: Option<u32>,
    #[schemars(description = "Page token from previous response for pagination")]
    pub page_token: Option<String>,
    #[schemars(description = "Sort order: 'desc' (default) or 'asc'")]
    pub order_by: Option<String>,
    #[schemars(description = "Filter expression for value ranges (e.g., 'value > 100')")]
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudOrderedDatastoreSetParams {
    #[schemars(description = "Universe ID containing the OrderedDataStore")]
    pub universe_id: u64,
    #[schemars(description = "Name of the OrderedDataStore")]
    pub datastore_name: String,
    #[schemars(description = "Scope within the datastore (default: 'global')")]
    pub scope: Option<String>,
    #[schemars(description = "Entry ID (key) for the entry")]
    pub entry_id: String,
    #[schemars(description = "Numerical value to store (integer)")]
    pub value: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudOrderedDatastoreIncrementParams {
    #[schemars(description = "Universe ID containing the OrderedDataStore")]
    pub universe_id: u64,
    #[schemars(description = "Name of the OrderedDataStore")]
    pub datastore_name: String,
    #[schemars(description = "Scope within the datastore (default: 'global')")]
    pub scope: Option<String>,
    #[schemars(description = "Entry ID (key) for the entry")]
    pub entry_id: String,
    #[schemars(description = "Amount to increment the value by (can be negative)")]
    pub increment: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudOrderedDatastoreDeleteParams {
    #[schemars(description = "Universe ID containing the OrderedDataStore")]
    pub universe_id: u64,
    #[schemars(description = "Name of the OrderedDataStore")]
    pub datastore_name: String,
    #[schemars(description = "Scope within the datastore (default: 'global')")]
    pub scope: Option<String>,
    #[schemars(description = "Entry ID (key) to delete")]
    pub entry_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudGetUniverseParams {
    #[schemars(description = "Universe ID to get information for")]
    pub universe_id: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudRestartServersParams {
    #[schemars(description = "Universe ID to restart servers for")]
    pub universe_id: u64,
}

// === LINTING PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsLintScriptParams {
    #[schemars(description = "Path to .luau file to lint")]
    pub file_path: String,
    #[schemars(description = "Path to selene.toml configuration file (optional)")]
    pub config_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StyluaFormatParams {
    #[schemars(description = "Path to .luau file to format")]
    pub file_path: String,
    #[schemars(description = "Path to stylua.toml configuration file (optional)")]
    pub config_path: Option<String>,
    #[schemars(
        description = "Check only mode - don't modify file, just report if changes needed (default: false)"
    )]
    pub check_only: Option<bool>,
}

// === ROJO PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RojoBuildParams {
    #[schemars(description = "Path to project directory or *.project.json file")]
    pub project_path: String,
    #[schemars(description = "Output path for the built .rbxl/.rbxlx file")]
    pub output_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RojoSourcemapParams {
    #[schemars(description = "Path to project directory or *.project.json file")]
    pub project_path: String,
    #[schemars(description = "Output path for the sourcemap JSON file (optional)")]
    pub output_path: Option<String>,
}

// === WALLY PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WallyInstallParams {
    #[schemars(description = "Path to project directory containing wally.toml")]
    pub project_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WallyUpdateParams {
    #[schemars(description = "Path to project directory containing wally.toml")]
    pub project_path: String,
}

// === MOONWAVE PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MoonwaveBuildParams {
    #[schemars(description = "Path to project directory containing moonwave.toml")]
    pub project_path: String,
    #[schemars(description = "Output directory for generated documentation (optional)")]
    pub output_dir: Option<String>,
}

// === WATCHER PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsWatchChangesParams {
    #[schemars(description = "Maximum number of changes to return (default: 100)")]
    pub limit: Option<usize>,
}

// === AI PARAMS ===
// These parameter structs define the JSON schema for AI-powered code search tools
// Requires the 'ai' feature flag and configured Neo4j + Voyage AI credentials
// Fields are used via JSON deserialization in MCP tools

#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code)] // Fields used via JSON deserialization
pub struct AiSearchCodebaseParams {
    #[schemars(description = "Natural language query describing what you're looking for")]
    pub query: String,
    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub limit: Option<usize>,
    #[schemars(description = "Minimum similarity score 0.0-1.0 (default: 0.5)")]
    pub min_similarity: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code)] // Fields used via JSON deserialization
pub struct AiFindRelatedParams {
    #[schemars(description = "File path to find related scripts for")]
    pub path: String,
    #[schemars(description = "Maximum graph traversal depth (default: 2)")]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code)] // Fields used via JSON deserialization
pub struct AiGetContextParams {
    #[schemars(description = "Task description to find relevant context for")]
    pub task: String,
    #[schemars(description = "Maximum tokens worth of context to return (default: 4000)")]
    pub token_budget: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code)] // Fields used via JSON deserialization
pub struct AiIndexProjectParams {
    #[schemars(description = "Root directory to index (defaults to project root)")]
    pub path: Option<String>,
    #[schemars(description = "Force reindex even if content unchanged (default: false)")]
    pub force: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // === FILESYSTEM PARAMS TESTS ===

    #[test]
    fn test_fs_get_tree_params_deserialize() {
        let json = r#"{"path": "/project", "max_depth": 3}"#;
        let params: FsGetTreeParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "/project");
        assert_eq!(params.max_depth, Some(3));
    }

    #[test]
    fn test_fs_get_tree_params_without_max_depth() {
        let json = r#"{"path": "/project"}"#;
        let params: FsGetTreeParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "/project");
        assert!(params.max_depth.is_none());
    }

    #[test]
    fn test_fs_read_script_params_deserialize() {
        let json = r#"{"file_path": "src/main.luau"}"#;
        let params: FsReadScriptParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.file_path, "src/main.luau");
    }

    #[test]
    fn test_fs_write_script_params_full() {
        let json = r#"{"file_path": "src/test.luau", "content": "print('hello')", "create_directories": true}"#;
        let params: FsWriteScriptParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.file_path, "src/test.luau");
        assert_eq!(params.content, "print('hello')");
        assert_eq!(params.create_directories, Some(true));
    }

    #[test]
    fn test_fs_write_script_params_minimal() {
        let json = r#"{"file_path": "test.luau", "content": ""}"#;
        let params: FsWriteScriptParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.file_path, "test.luau");
        assert_eq!(params.content, "");
        assert!(params.create_directories.is_none());
    }

    #[test]
    fn test_fs_delete_script_params_deserialize() {
        let json = r#"{"file_path": "src/old.luau"}"#;
        let params: FsDeleteScriptParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.file_path, "src/old.luau");
    }

    #[test]
    fn test_fs_search_content_params_deserialize() {
        let json = r#"{"path": "src", "pattern": "function\\s+\\w+", "extension": "luau"}"#;
        let params: FsSearchContentParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "src");
        assert_eq!(params.pattern, "function\\s+\\w+");
        assert_eq!(params.extension, "luau");
    }

    #[test]
    fn test_fs_get_changes_params_deserialize() {
        let json = r#"{"path": "/project/src"}"#;
        let params: FsGetChangesParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "/project/src");
    }

    // === STUDIO PARAMS TESTS ===

    #[test]
    fn test_studio_get_datamodel_params_full() {
        let json = r#"{"max_depth": 5}"#;
        let params: StudioGetDataModelParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.max_depth, Some(5));
    }

    #[test]
    fn test_studio_get_datamodel_params_empty() {
        let json = r#"{}"#;
        let params: StudioGetDataModelParams = serde_json::from_str(json).unwrap();
        assert!(params.max_depth.is_none());
    }

    #[test]
    fn test_studio_get_datamodel_paginated_params_full() {
        let json =
            r#"{"max_depth": 3, "start_path": "game.Workspace", "limit": 500, "cursor": "abc123"}"#;
        let params: StudioGetDataModelPaginatedParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.max_depth, Some(3));
        assert_eq!(params.start_path, Some("game.Workspace".to_string()));
        assert_eq!(params.limit, Some(500));
        assert_eq!(params.cursor, Some("abc123".to_string()));
    }

    #[test]
    fn test_studio_get_datamodel_paginated_params_minimal() {
        let json = r#"{}"#;
        let params: StudioGetDataModelPaginatedParams = serde_json::from_str(json).unwrap();
        assert!(params.max_depth.is_none());
        assert!(params.start_path.is_none());
        assert!(params.limit.is_none());
        assert!(params.cursor.is_none());
    }

    #[test]
    fn test_studio_get_script_source_params_deserialize() {
        let json = r#"{"path": "game.ServerScriptService.Main"}"#;
        let params: StudioGetScriptSourceParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "game.ServerScriptService.Main");
    }

    #[test]
    fn test_studio_modify_script_params_full() {
        let json =
            r#"{"path": "game.Scripts.Test", "new_source": "print('hi')", "record_undo": false}"#;
        let params: StudioModifyScriptParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "game.Scripts.Test");
        assert_eq!(params.new_source, "print('hi')");
        assert_eq!(params.record_undo, Some(false));
    }

    #[test]
    fn test_studio_modify_script_params_minimal() {
        let json = r#"{"path": "game.Test", "new_source": ""}"#;
        let params: StudioModifyScriptParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "game.Test");
        assert_eq!(params.new_source, "");
        assert!(params.record_undo.is_none());
    }

    #[test]
    fn test_studio_create_instance_params_full() {
        let json = r#"{"class_name": "Part", "parent": "game.Workspace", "name": "MyPart", "properties": {"Size": [4, 1, 2]}, "record_undo": true}"#;
        let params: StudioCreateInstanceParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.class_name, "Part");
        assert_eq!(params.parent, "game.Workspace");
        assert_eq!(params.name, "MyPart");
        assert!(params.properties.is_some());
        assert_eq!(params.record_undo, Some(true));
    }

    #[test]
    fn test_studio_create_instance_params_minimal() {
        let json = r#"{"class_name": "Script", "parent": "game.ServerScriptService", "name": "NewScript"}"#;
        let params: StudioCreateInstanceParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.class_name, "Script");
        assert_eq!(params.parent, "game.ServerScriptService");
        assert_eq!(params.name, "NewScript");
        assert!(params.properties.is_none());
        assert!(params.record_undo.is_none());
    }

    #[test]
    fn test_studio_set_property_params_full() {
        let json = r#"{"path": "game.Workspace.Part", "property": "Name", "value": "NewName", "record_undo": true}"#;
        let params: StudioSetPropertyParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "game.Workspace.Part");
        assert_eq!(params.property, "Name");
        assert_eq!(params.value, "NewName");
        assert_eq!(params.record_undo, Some(true));
    }

    #[test]
    fn test_studio_set_property_params_with_complex_value() {
        let json =
            r#"{"path": "game.Workspace.Part", "property": "Position", "value": [0, 10, 0]}"#;
        let params: StudioSetPropertyParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "game.Workspace.Part");
        assert_eq!(params.property, "Position");
        assert!(params.value.is_array());
        assert!(params.record_undo.is_none());
    }

    #[test]
    fn test_studio_delete_instance_params_full() {
        let json = r#"{"path": "game.Workspace.OldPart", "record_undo": true}"#;
        let params: StudioDeleteInstanceParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "game.Workspace.OldPart");
        assert_eq!(params.record_undo, Some(true));
    }

    #[test]
    fn test_studio_delete_instance_params_minimal() {
        let json = r#"{"path": "game.Workspace.Part"}"#;
        let params: StudioDeleteInstanceParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "game.Workspace.Part");
        assert!(params.record_undo.is_none());
    }

    #[test]
    fn test_studio_find_instances_params_full() {
        let json = r#"{"class_name": "Script", "root": "game.ServerScriptService"}"#;
        let params: StudioFindInstancesParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.class_name, "Script");
        assert_eq!(params.root, Some("game.ServerScriptService".to_string()));
    }

    #[test]
    fn test_studio_find_instances_params_minimal() {
        let json = r#"{"class_name": "Part"}"#;
        let params: StudioFindInstancesParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.class_name, "Part");
        assert!(params.root.is_none());
    }

    #[test]
    fn test_studio_insert_r15_rig_params_full() {
        let json = r#"{"parent": "game.ReplicatedStorage.NPCModels", "name": "TestRig", "record_undo": true}"#;
        let params: StudioInsertR15RigParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.parent, "game.ReplicatedStorage.NPCModels");
        assert_eq!(params.name, "TestRig");
        assert_eq!(params.record_undo, Some(true));
    }

    #[test]
    fn test_studio_insert_r15_rig_params_minimal() {
        let json = r#"{"parent": "game.Workspace", "name": "NPC"}"#;
        let params: StudioInsertR15RigParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.parent, "game.Workspace");
        assert_eq!(params.name, "NPC");
        assert!(params.record_undo.is_none());
    }

    // === CLOUD PARAMS TESTS ===

    #[test]
    fn test_cloud_publish_place_params_deserialize() {
        let json = r#"{"universe_id": 123456789, "place_id": 987654321, "rbxl_path": "/path/to/game.rbxl"}"#;
        let params: CloudPublishPlaceParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.universe_id, 123456789);
        assert_eq!(params.place_id, 987654321);
        assert_eq!(params.rbxl_path, "/path/to/game.rbxl");
    }

    #[test]
    fn test_cloud_upload_asset_params_deserialize() {
        let json = r#"{"asset_type": "image", "file_path": "/path/to/image.png", "name": "MyImage", "description": "An image asset", "creator_id": 12345}"#;
        let params: CloudUploadAssetParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.asset_type, "image");
        assert_eq!(params.file_path, "/path/to/image.png");
        assert_eq!(params.name, "MyImage");
        assert_eq!(params.description, "An image asset");
        assert_eq!(params.creator_id, 12345);
    }

    #[test]
    fn test_cloud_datastore_get_params_full() {
        let json = r#"{"universe_id": 123, "datastore_name": "PlayerData", "key": "player_123", "scope": "global"}"#;
        let params: CloudDatastoreGetParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.universe_id, 123);
        assert_eq!(params.datastore_name, "PlayerData");
        assert_eq!(params.key, "player_123");
        assert_eq!(params.scope, Some("global".to_string()));
    }

    #[test]
    fn test_cloud_datastore_get_params_minimal() {
        let json = r#"{"universe_id": 456, "datastore_name": "Stats", "key": "user_789"}"#;
        let params: CloudDatastoreGetParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.universe_id, 456);
        assert_eq!(params.datastore_name, "Stats");
        assert_eq!(params.key, "user_789");
        assert!(params.scope.is_none());
    }

    // === LINTING PARAMS TESTS ===

    #[test]
    fn test_fs_lint_script_params_full() {
        let json = r#"{"file_path": "src/main.luau", "config_path": ".selene.toml"}"#;
        let params: FsLintScriptParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.file_path, "src/main.luau");
        assert_eq!(params.config_path, Some(".selene.toml".to_string()));
    }

    #[test]
    fn test_fs_lint_script_params_minimal() {
        let json = r#"{"file_path": "test.luau"}"#;
        let params: FsLintScriptParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.file_path, "test.luau");
        assert!(params.config_path.is_none());
    }

    #[test]
    fn test_stylua_format_params_full() {
        let json =
            r#"{"file_path": "src/main.luau", "config_path": "stylua.toml", "check_only": true}"#;
        let params: StyluaFormatParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.file_path, "src/main.luau");
        assert_eq!(params.config_path, Some("stylua.toml".to_string()));
        assert_eq!(params.check_only, Some(true));
    }

    #[test]
    fn test_stylua_format_params_minimal() {
        let json = r#"{"file_path": "test.luau"}"#;
        let params: StyluaFormatParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.file_path, "test.luau");
        assert!(params.config_path.is_none());
        assert!(params.check_only.is_none());
    }

    // === ROJO PARAMS TESTS ===

    #[test]
    fn test_rojo_build_params() {
        let json = r#"{"project_path": "default.project.json", "output_path": "build/game.rbxl"}"#;
        let params: RojoBuildParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project_path, "default.project.json");
        assert_eq!(params.output_path, "build/game.rbxl");
    }

    #[test]
    fn test_rojo_sourcemap_params_full() {
        let json = r#"{"project_path": "default.project.json", "output_path": "sourcemap.json"}"#;
        let params: RojoSourcemapParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project_path, "default.project.json");
        assert_eq!(params.output_path, Some("sourcemap.json".to_string()));
    }

    #[test]
    fn test_rojo_sourcemap_params_minimal() {
        let json = r#"{"project_path": "default.project.json"}"#;
        let params: RojoSourcemapParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project_path, "default.project.json");
        assert!(params.output_path.is_none());
    }

    // === WALLY PARAMS TESTS ===

    #[test]
    fn test_wally_install_params_deserialize() {
        let json = r#"{"project_path": "/my/project"}"#;
        let params: WallyInstallParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project_path, "/my/project");
    }

    #[test]
    fn test_wally_update_params_deserialize() {
        let json = r#"{"project_path": "/my/project"}"#;
        let params: WallyUpdateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project_path, "/my/project");
    }

    // === MOONWAVE PARAMS TESTS ===

    #[test]
    fn test_moonwave_build_params_full() {
        let json = r#"{"project_path": "/my/project", "output_dir": "/docs/output"}"#;
        let params: MoonwaveBuildParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project_path, "/my/project");
        assert_eq!(params.output_dir, Some("/docs/output".to_string()));
    }

    #[test]
    fn test_moonwave_build_params_minimal() {
        let json = r#"{"project_path": "/my/project"}"#;
        let params: MoonwaveBuildParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project_path, "/my/project");
        assert!(params.output_dir.is_none());
    }

    // === WATCHER PARAMS TESTS ===

    #[test]
    fn test_fs_watch_changes_params_with_limit() {
        let json = r#"{"limit": 50}"#;
        let params: FsWatchChangesParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.limit, Some(50));
    }

    #[test]
    fn test_fs_watch_changes_params_empty() {
        let json = r#"{}"#;
        let params: FsWatchChangesParams = serde_json::from_str(json).unwrap();
        assert!(params.limit.is_none());
    }

    // === DEBUG TRAIT TESTS ===

    #[test]
    fn test_fs_get_tree_params_debug() {
        let params = FsGetTreeParams {
            path: "/test".to_string(),
            max_depth: Some(3),
        };
        let debug = format!("{:?}", params);
        assert!(debug.contains("FsGetTreeParams"));
        assert!(debug.contains("/test"));
    }

    #[test]
    fn test_studio_create_instance_params_debug() {
        let params = StudioCreateInstanceParams {
            class_name: "Part".to_string(),
            parent: "game.Workspace".to_string(),
            name: "TestPart".to_string(),
            properties: None,
            record_undo: None,
        };
        let debug = format!("{:?}", params);
        assert!(debug.contains("StudioCreateInstanceParams"));
        assert!(debug.contains("Part"));
    }

    #[test]
    fn test_cloud_publish_place_params_debug() {
        let params = CloudPublishPlaceParams {
            universe_id: 123,
            place_id: 456,
            rbxl_path: "/path/to/file.rbxl".to_string(),
        };
        let debug = format!("{:?}", params);
        assert!(debug.contains("CloudPublishPlaceParams"));
        assert!(debug.contains("123"));
    }

    // === AI PARAMS TESTS ===

    #[test]
    fn test_ai_search_codebase_params_full() {
        let json = r#"{"query": "damage calculation", "limit": 5, "min_similarity": 0.7}"#;
        let params: AiSearchCodebaseParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "damage calculation");
        assert_eq!(params.limit, Some(5));
        assert_eq!(params.min_similarity, Some(0.7));
    }

    #[test]
    fn test_ai_search_codebase_params_minimal() {
        let json = r#"{"query": "player data"}"#;
        let params: AiSearchCodebaseParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "player data");
        assert!(params.limit.is_none());
        assert!(params.min_similarity.is_none());
    }

    #[test]
    fn test_ai_find_related_params_full() {
        let json = r#"{"path": "src/combat/damage.luau", "max_depth": 3}"#;
        let params: AiFindRelatedParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "src/combat/damage.luau");
        assert_eq!(params.max_depth, Some(3));
    }

    #[test]
    fn test_ai_find_related_params_minimal() {
        let json = r#"{"path": "src/main.luau"}"#;
        let params: AiFindRelatedParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, "src/main.luau");
        assert!(params.max_depth.is_none());
    }

    #[test]
    fn test_ai_get_context_params_full() {
        let json = r#"{"task": "fix the combat system", "token_budget": 8000}"#;
        let params: AiGetContextParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.task, "fix the combat system");
        assert_eq!(params.token_budget, Some(8000));
    }

    #[test]
    fn test_ai_get_context_params_minimal() {
        let json = r#"{"task": "add player respawn"}"#;
        let params: AiGetContextParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.task, "add player respawn");
        assert!(params.token_budget.is_none());
    }

    #[test]
    fn test_ai_index_project_params_full() {
        let json = r#"{"path": "src/server", "force": true}"#;
        let params: AiIndexProjectParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.path, Some("src/server".to_string()));
        assert_eq!(params.force, Some(true));
    }

    #[test]
    fn test_ai_index_project_params_empty() {
        let json = r#"{}"#;
        let params: AiIndexProjectParams = serde_json::from_str(json).unwrap();
        assert!(params.path.is_none());
        assert!(params.force.is_none());
    }
}
