//! Cloud tool implementations.
//!
//! Provides 11 tools for Roblox Open Cloud API access:
//! - `cloud_publish_place` - Publish place file to Roblox
//! - `cloud_upload_asset` - Upload asset (image, model, audio)
//! - `cloud_datastore_get` - Get DataStore value
//! - `cloud_datastore_set` - Set DataStore value
//! - `cloud_messaging_publish` - Publish to MessagingService
//! - `cloud_ordered_datastore_list` - List OrderedDataStore entries
//! - `cloud_ordered_datastore_set` - Set OrderedDataStore entry
//! - `cloud_ordered_datastore_increment` - Increment OrderedDataStore value
//! - `cloud_ordered_datastore_delete` - Delete OrderedDataStore entry
//! - `cloud_get_universe` - Get universe metadata
//! - `cloud_restart_servers` - Restart all game servers

use std::path::PathBuf;

use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData;
use serde_json::json;

use crate::bridge::StudioBridge;
use crate::cloud::OrderedDataStoreListParams;
use crate::cloud::AssetType;
use crate::mcp::params::{
    CloudDatastoreGetParams, CloudDatastoreSetParams, CloudGetUniverseParams,
    CloudMessagingPublishParams, CloudOrderedDatastoreDeleteParams,
    CloudOrderedDatastoreIncrementParams, CloudOrderedDatastoreListParams,
    CloudOrderedDatastoreSetParams, CloudPublishPlaceParams, CloudRestartServersParams,
    CloudUploadAssetParams,
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
    // cloud_publish_place - Publish place file to Roblox
    // =========================================================================

    pub(crate) async fn cloud_publish_place_impl(
        &self,
        params: CloudPublishPlaceParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        let path = PathBuf::from(&params.rbxl_path);
        let result = client
            .publish_place(params.universe_id, params.place_id, &path)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "version_number": result.version_number,
                "universe_id": params.universe_id,
                "place_id": params.place_id
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_upload_asset - Upload asset to Roblox
    // =========================================================================

    pub(crate) async fn cloud_upload_asset_impl(
        &self,
        params: CloudUploadAssetParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        // Parse asset type
        let asset_type = AssetType::from_str(&params.asset_type)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let path = PathBuf::from(&params.file_path);
        let result = client
            .upload_asset(
                asset_type,
                &path,
                &params.name,
                &params.description,
                params.creator_id,
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "operation_path": result.path,
                "done": result.done
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_datastore_get - Get DataStore value
    // =========================================================================

    pub(crate) async fn cloud_datastore_get_impl(
        &self,
        params: CloudDatastoreGetParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        let result = client
            .datastore_get(
                params.universe_id,
                &params.datastore_name,
                &params.key,
                params.scope.as_deref(),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "value": result.value,
                "version": result.version,
                "created_time": result.created_time,
                "updated_time": result.updated_time
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_datastore_set - Set DataStore value
    // =========================================================================

    pub(crate) async fn cloud_datastore_set_impl(
        &self,
        params: CloudDatastoreSetParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        let result = client
            .datastore_set(
                params.universe_id,
                &params.datastore_name,
                &params.key,
                params.value.clone(),
                params.scope.as_deref(),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "value": result.value,
                "version": result.version,
                "created_time": result.created_time,
                "updated_time": result.updated_time
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_messaging_publish - Publish to MessagingService
    // =========================================================================

    pub(crate) async fn cloud_messaging_publish_impl(
        &self,
        params: CloudMessagingPublishParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        client
            .messaging_publish(params.universe_id, &params.topic, params.message.clone())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "topic": params.topic,
                "universe_id": params.universe_id,
                "message_preview": if params.message.to_string().len() > 100 {
                    format!("{}...", &params.message.to_string()[..100])
                } else {
                    params.message.to_string()
                }
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_ordered_datastore_list - List OrderedDataStore entries
    // =========================================================================

    pub(crate) async fn cloud_ordered_datastore_list_impl(
        &self,
        params: CloudOrderedDatastoreListParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        let result = client
            .ordered_datastore_list(OrderedDataStoreListParams {
                universe_id: params.universe_id,
                datastore_name: &params.datastore_name,
                scope: params.scope.as_deref(),
                max_page_size: params.max_page_size,
                page_token: params.page_token.as_deref(),
                order_by: params.order_by.as_deref(),
                filter: params.filter.as_deref(),
            })
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Use compact JSON for leaderboard entries (potentially many entries)
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&json!({
                "entries": result.entries,
                "next_page_token": result.next_page_token
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_ordered_datastore_set - Set OrderedDataStore entry
    // =========================================================================

    pub(crate) async fn cloud_ordered_datastore_set_impl(
        &self,
        params: CloudOrderedDatastoreSetParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        let result = client
            .ordered_datastore_set(
                params.universe_id,
                &params.datastore_name,
                params.scope.as_deref(),
                &params.entry_id,
                params.value,
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "path": result.path,
                "id": result.id,
                "value": result.value
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_ordered_datastore_increment - Increment OrderedDataStore value
    // =========================================================================

    pub(crate) async fn cloud_ordered_datastore_increment_impl(
        &self,
        params: CloudOrderedDatastoreIncrementParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        let result = client
            .ordered_datastore_increment(
                params.universe_id,
                &params.datastore_name,
                params.scope.as_deref(),
                &params.entry_id,
                params.increment,
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "path": result.path,
                "id": result.id,
                "value": result.value
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_ordered_datastore_delete - Delete OrderedDataStore entry
    // =========================================================================

    pub(crate) async fn cloud_ordered_datastore_delete_impl(
        &self,
        params: CloudOrderedDatastoreDeleteParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        client
            .ordered_datastore_delete(
                params.universe_id,
                &params.datastore_name,
                params.scope.as_deref(),
                &params.entry_id,
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "deleted_entry_id": params.entry_id
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_get_universe - Get universe metadata
    // =========================================================================

    pub(crate) async fn cloud_get_universe_impl(
        &self,
        params: CloudGetUniverseParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        let info = client
            .get_universe(params.universe_id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "path": info.path,
                "display_name": info.display_name,
                "description": info.description,
                "create_time": info.create_time,
                "update_time": info.update_time,
                "visibility": info.visibility,
                "user": info.user,
                "group": info.group,
                "voice_chat_enabled": info.voice_chat_enabled,
                "age_rating": info.age_rating,
                "platforms": {
                    "desktop": info.desktop_enabled,
                    "mobile": info.mobile_enabled,
                    "tablet": info.tablet_enabled,
                    "vr": info.vr_enabled,
                    "console": info.console_enabled
                }
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // cloud_restart_servers - Restart all game servers
    // =========================================================================

    pub(crate) async fn cloud_restart_servers_impl(
        &self,
        params: CloudRestartServersParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud()?;

        client
            .restart_universe_servers(params.universe_id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "universe_id": params.universe_id,
                "message": "Server restart initiated. All servers will gracefully restart."
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }
}
