//! Toolchain tool implementations.
//!
//! Provides 8 tools for external Luau development tool integration:
//! - `stylua_format` - Format Luau scripts using StyLua
//! - `rojo_build` - Build Roblox project using Rojo
//! - `rojo_sourcemap` - Generate Rojo sourcemap
//! - `wally_install` - Install Wally packages
//! - `wally_update` - Update Wally packages
//! - `moonwave_build` - Build Moonwave documentation
//! - `lune_run` - Run Luau scripts using Lune runtime
//! - `lune_eval` - Evaluate inline Luau code using Lune

use std::path::Path;
use std::time::Duration;

use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData;

use crate::bridge::StudioBridge;
use crate::mcp::params::{
    LuneEvalParams, LuneRunParams, MoonwaveBuildParams, RojoBuildParams, RojoSourcemapParams,
    StyluaFormatParams, WallyInstallParams, WallyUpdateParams,
};
use crate::mcp::server::RobloxMcpServer;
use crate::tools::filesystem::validate_path;
use crate::tools::formatting::Formatter;
use crate::tools::linting::Linter;
use crate::tools::lune::LuneRunner;
use crate::tools::moonwave::MoonwaveRunner;
use crate::tools::rojo::RojoRunner;
use crate::tools::wally::WallyRunner;

impl<B, L, F, R, W, M, LN> RobloxMcpServer<B, L, F, R, W, M, LN>
where
    B: StudioBridge + Clone + 'static,
    L: Linter + Clone + 'static,
    F: Formatter + Clone + 'static,
    R: RojoRunner + Clone + 'static,
    W: WallyRunner + Clone + 'static,
    M: MoonwaveRunner + Clone + 'static,
    LN: LuneRunner + Clone + 'static,
{
    // =========================================================================
    // stylua_format - Format Luau scripts using StyLua
    // =========================================================================

    pub(crate) async fn stylua_format_impl(
        &self,
        params: StyluaFormatParams,
    ) -> Result<CallToolResult, ErrorData> {
        let file_path = Path::new(&params.file_path);

        // Validate path is within project root
        let validated_path = validate_path(file_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate .luau extension
        if validated_path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::invalid_params(
                "Only .luau files can be formatted",
                None,
            ));
        }

        // Parse config path
        let config_path = params.config_path.as_ref().map(Path::new);

        // Default check_only to false
        let check_only = params.check_only.unwrap_or(false);

        // Run the formatter
        let result = self
            .formatter
            .format(&validated_path, config_path, check_only)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // rojo_build - Build Roblox project using Rojo
    // =========================================================================

    pub(crate) async fn rojo_build_impl(
        &self,
        params: RojoBuildParams,
    ) -> Result<CallToolResult, ErrorData> {
        let project_path = Path::new(&params.project_path);
        let output_path = Path::new(&params.output_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate output path is within project root
        let validated_output = validate_path(output_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Run rojo build
        let result = self
            .rojo
            .build(&validated_project, &validated_output)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // rojo_sourcemap - Generate Rojo sourcemap
    // =========================================================================

    pub(crate) async fn rojo_sourcemap_impl(
        &self,
        params: RojoSourcemapParams,
    ) -> Result<CallToolResult, ErrorData> {
        let project_path = Path::new(&params.project_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Parse optional output path
        let output_path = params
            .output_path
            .as_ref()
            .map(|p| Path::new(p).to_path_buf());

        // Run rojo sourcemap
        let result = self
            .rojo
            .sourcemap(&validated_project, output_path.as_deref())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Use compact JSON for large sourcemap output
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // wally_install - Install Wally packages
    // =========================================================================

    pub(crate) async fn wally_install_impl(
        &self,
        params: WallyInstallParams,
    ) -> Result<CallToolResult, ErrorData> {
        let project_path = Path::new(&params.project_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Run wally install
        let result = self
            .wally
            .install(&validated_project)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // wally_update - Update Wally packages
    // =========================================================================

    pub(crate) async fn wally_update_impl(
        &self,
        params: WallyUpdateParams,
    ) -> Result<CallToolResult, ErrorData> {
        let project_path = Path::new(&params.project_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Run wally update
        let result = self
            .wally
            .update(&validated_project)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // moonwave_build - Build Moonwave documentation
    // =========================================================================

    pub(crate) async fn moonwave_build_impl(
        &self,
        params: MoonwaveBuildParams,
    ) -> Result<CallToolResult, ErrorData> {
        let project_path = Path::new(&params.project_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Parse optional output directory
        let output_dir = params.output_dir.as_ref().map(Path::new);

        // Run moonwave build
        let result = self
            .moonwave
            .build(&validated_project, output_dir)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // lune_run - Run a Luau script using Lune runtime
    // =========================================================================

    pub(crate) async fn lune_run_impl(
        &self,
        params: LuneRunParams,
    ) -> Result<CallToolResult, ErrorData> {
        let script_path = Path::new(&params.script_path);

        // Validate path is within project root
        let validated_path = validate_path(script_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate .luau extension
        if validated_path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::invalid_params(
                "Only .luau files can be executed",
                None,
            ));
        }

        // Parse args (default to empty)
        let args = params.args.unwrap_or_default();

        // Parse timeout (default 30 seconds)
        let timeout = params.timeout.map(Duration::from_secs);

        // Run the script via Lune
        let result = self
            .lune
            .run(&validated_path, &args, timeout)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // =========================================================================
    // lune_eval - Evaluate inline Luau code using Lune runtime
    // =========================================================================

    pub(crate) async fn lune_eval_impl(
        &self,
        params: LuneEvalParams,
    ) -> Result<CallToolResult, ErrorData> {
        // Parse timeout (default 10 seconds for eval)
        let timeout = params.timeout.map(Duration::from_secs);

        // Evaluate the code via Lune
        let result = self
            .lune
            .eval(&params.code, timeout)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }
}
