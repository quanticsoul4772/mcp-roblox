//! Domain-specific tool implementations for the MCP server.
//!
//! This module organizes tool implementations by domain to improve maintainability.
//! The `#[tool]` macro declarations remain in `server.rs`, but the actual implementation
//! logic (`*_impl` methods) are split into domain-specific modules.
//!
//! # Modules
//!
//! - `filesystem` - File operations: read, write, delete, search, lint, watch
//! - `studio` - Roblox Studio integration via plugin bridge
//! - `cloud` - Roblox Open Cloud API operations
//! - `toolchain` - External tools: StyLua, Rojo, Wally, Moonwave
//!
//! Note: AI tools and metrics remain inline in `server.rs` due to cfg-gating complexity.

pub mod cloud;
pub mod filesystem;
pub mod studio;
pub mod toolchain;
