//! Open Cloud integration for Roblox Studio MCP Server
//!
//! Provides CI/CD automation capabilities:
//! - Publish places to Roblox
//! - Upload assets (images, models, audio)
//! - Manage DataStores

mod assets;
mod client;
mod datastores;

// Re-export public API types (may be used by external consumers)
#[allow(unused_imports)]
pub use assets::{AssetType, AssetUploadResult};
pub use client::OpenCloudClient;
#[allow(unused_imports)]
pub use datastores::DataStoreEntry;
