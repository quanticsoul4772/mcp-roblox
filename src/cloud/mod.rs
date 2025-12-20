//! Open Cloud integration for Roblox Studio MCP Server
//!
//! Provides CI/CD automation capabilities:
//! - Publish places to Roblox
//! - Upload assets (images, models, audio)
//! - Manage DataStores

mod assets;
mod client;
mod datastores;

pub use assets::{AssetType, AssetUploadResult};
pub use client::OpenCloudClient;
pub use datastores::DataStoreEntry;
