//! Studio tool implementations.
//!
//! Provides 14 tools for Roblox Studio integration via the HTTP plugin bridge:
//! - `studio_health_check` - Check plugin connectivity
//! - `studio_get_selection` - Get selected instances
//! - `studio_get_datamodel` - Explore DataModel hierarchy
//! - `studio_get_datamodel_paginated` - Paginated DataModel traversal
//! - `studio_get_script_source` - Read script source
//! - `studio_get_properties` - Read instance properties
//! - `studio_get_bounds` - Get Part/Model bounding box
//! - `studio_modify_script` - Modify script with undo
//! - `studio_create_instance` - Create new instance
//! - `studio_insert_r15_rig` - Insert R15 humanoid rig
//! - `studio_set_property` - Set instance property
//! - `studio_delete_instance` - Delete instance with undo
//! - `studio_find_instances` - Find instances by class
//! - `studio_get_output` - Get output window logs

use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData;
use serde_json::json;

use crate::bridge::StudioBridge;
use crate::mcp::params::{
    StudioCreateInstanceParams, StudioDeleteInstanceParams, StudioFindInstancesParams,
    StudioGetBoundsParams, StudioGetDataModelPaginatedParams, StudioGetDataModelParams,
    StudioGetOutputParams, StudioGetPropertiesParams, StudioGetScriptSourceParams,
    StudioInsertR15RigParams, StudioModifyScriptParams, StudioSetPropertyParams,
};
use crate::mcp::server::RobloxMcpServer;
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
    // studio_health_check - Check if Studio plugin is connected
    // =========================================================================

    pub(crate) async fn studio_health_check_impl(&self) -> Result<CallToolResult, ErrorData> {
        let connected = self.bridge.is_connected().await;

        self.metrics.record_connection_status(connected);
        let connection_stats = self.metrics.connection_snapshot();

        let result = json!({
            "connected": connected,
            "message": if connected {
                "Studio plugin is connected and responsive"
            } else {
                "Studio plugin is not connected or heartbeat timed out"
            },
            "stats": connection_stats
        });

        let call_result = CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]);

        Ok(CallToolResult {
            is_error: Some(!connected),
            ..call_result
        })
    }

    // =========================================================================
    // studio_get_selection - Get currently selected instances
    // =========================================================================

    pub(crate) async fn studio_get_selection_impl(&self) -> Result<CallToolResult, ErrorData> {
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

    // =========================================================================
    // studio_get_datamodel - Explore the live Studio DataModel hierarchy
    // =========================================================================

    pub(crate) async fn studio_get_datamodel_impl(
        &self,
        params: StudioGetDataModelParams,
    ) -> Result<CallToolResult, ErrorData> {
        // max_depth default: 3 levels
        let result = self
            .bridge
            .execute_command(
                "getDataModel",
                json!({ "maxDepth": params.max_depth.unwrap_or(3) }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // studio_get_datamodel_paginated - Paginated DataModel traversal
    // =========================================================================

    pub(crate) async fn studio_get_datamodel_paginated_impl(
        &self,
        params: StudioGetDataModelPaginatedParams,
    ) -> Result<CallToolResult, ErrorData> {
        // Defaults: max_depth=3, limit=500 (capped at 1000), start_path="game"
        let max_depth = params.max_depth.unwrap_or(3);
        let limit = params.limit.unwrap_or(500).min(1000);
        let start_path = params.start_path.unwrap_or_else(|| "game".to_string());

        let result = self
            .bridge
            .execute_command(
                "getDataModelPaginated",
                json!({
                    "startPath": start_path,
                    "maxDepth": max_depth,
                    "limit": limit,
                    "cursor": params.cursor,
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // studio_get_script_source - Read script source from Studio
    // =========================================================================

    pub(crate) async fn studio_get_script_source_impl(
        &self,
        params: StudioGetScriptSourceParams,
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

    // =========================================================================
    // studio_get_properties - Read properties from any instance
    // =========================================================================

    pub(crate) async fn studio_get_properties_impl(
        &self,
        params: StudioGetPropertiesParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "getProperties",
                json!({
                    "path": params.path,
                    "properties": params.properties
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // studio_get_bounds - Get bounding box of a BasePart or Model
    // =========================================================================

    pub(crate) async fn studio_get_bounds_impl(
        &self,
        params: StudioGetBoundsParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command("getBounds", json!({ "path": params.path }))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // studio_modify_script - Modify script source with undo support
    // =========================================================================

    pub(crate) async fn studio_modify_script_impl(
        &self,
        params: StudioModifyScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        // record_undo default: true
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

    // =========================================================================
    // studio_create_instance - Create a new instance in Studio
    // =========================================================================

    pub(crate) async fn studio_create_instance_impl(
        &self,
        params: StudioCreateInstanceParams,
    ) -> Result<CallToolResult, ErrorData> {
        // record_undo default: true
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

    // =========================================================================
    // studio_insert_r15_rig - Insert a complete R15 humanoid rig
    // =========================================================================

    pub(crate) async fn studio_insert_r15_rig_impl(
        &self,
        params: StudioInsertR15RigParams,
    ) -> Result<CallToolResult, ErrorData> {
        // record_undo default: true
        let result = self
            .bridge
            .execute_command(
                "insertR15Rig",
                json!({
                    "parent": params.parent,
                    "name": params.name,
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

    // =========================================================================
    // studio_set_property - Set a property on an instance
    // =========================================================================

    pub(crate) async fn studio_set_property_impl(
        &self,
        params: StudioSetPropertyParams,
    ) -> Result<CallToolResult, ErrorData> {
        // record_undo default: true
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

    // =========================================================================
    // studio_delete_instance - Delete an instance from Studio
    // =========================================================================

    pub(crate) async fn studio_delete_instance_impl(
        &self,
        params: StudioDeleteInstanceParams,
    ) -> Result<CallToolResult, ErrorData> {
        // record_undo default: true
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

    // =========================================================================
    // studio_find_instances - Find all instances of a specific class
    // =========================================================================

    pub(crate) async fn studio_find_instances_impl(
        &self,
        params: StudioFindInstancesParams,
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
            serde_json::to_string(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // studio_get_output - Get recent Output window logs
    // =========================================================================

    pub(crate) async fn studio_get_output_impl(
        &self,
        params: StudioGetOutputParams,
    ) -> Result<CallToolResult, ErrorData> {
        // limit default: 100 entries
        let limit = params.limit.unwrap_or(100);

        let result = self
            .bridge
            .execute_command("getOutput", json!({ "limit": limit }))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }
}
