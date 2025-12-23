//! Open Cloud integration for Roblox Studio MCP Server
//!
//! Provides CI/CD automation capabilities:
//! - Publish places to Roblox
//! - Upload assets (images, models, audio)
//! - Manage DataStores
//! - Manage OrderedDataStores (leaderboards)
//! - Publish messages via MessagingService
//! - Manage Universes (game metadata, server restart)
//!
//! # Architecture
//!
//! The [`CloudClient`] trait enables dependency injection for testing:
//! - Production: [`OpenCloudClient`] with real HTTP client
//! - Testing: [`mock::MockCloudClient`] with queued responses

mod assets;
mod client;
mod datastores;
mod messaging;
mod ordered_datastores;
mod traits;
mod universes;

#[cfg(test)]
pub mod mock;

// Re-export public API types
pub use assets::{AssetType, AssetUploadResult};
pub use client::{OpenCloudClient, PublishResult};
pub use datastores::DataStoreEntry;
#[allow(unused_imports)]
pub use messaging::MessagePublishResult;
pub use ordered_datastores::{
    OrderedDataStoreEntry, OrderedDataStoreList, OrderedDataStoreListParams,
};
pub use traits::CloudClient;
pub use universes::UniverseInfo;
