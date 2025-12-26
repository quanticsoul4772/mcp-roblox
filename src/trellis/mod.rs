//! TRELLIS text-to-3D mesh generation via RunPod serverless.
//!
//! This module provides text-to-3D mesh generation using Microsoft's TRELLIS model
//! deployed on RunPod serverless infrastructure. Generated meshes are returned as
//! parsed GLB data that can be reconstructed in Roblox Studio using EditableMesh
//! and CreateMeshPartAsync.

mod client;
mod config;
mod glb_parser;

pub use client::TrellisClient;
pub use config::TrellisConfig;
pub use glb_parser::{Face, Normal, TexCoord, Vertex};
