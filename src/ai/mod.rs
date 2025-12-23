//! AI-powered code search and analysis for Roblox Studio.
//!
//! This module provides semantic search capabilities using Voyage AI embeddings
//! and Neo4j for knowledge graph storage. It enables Claude Code to efficiently
//! find relevant Luau scripts without reading entire files.
//!
//! # Feature Flag
//!
//! This module is only compiled when the `ai` feature is enabled:
//! ```bash
//! cargo build --features ai
//! ```
//!
//! # Required Environment Variables
//!
//! - `VOYAGE_API_KEY`: Voyage AI API key for generating embeddings
//! - `NEO4J_URI`: Neo4j connection URI (e.g., `neo4j+s://xxx.databases.neo4j.io`)
//! - `NEO4J_PASSWORD`: Neo4j password
//!
//! # Optional Environment Variables
//!
//! - `VOYAGE_MODEL`: Embedding model (default: `voyage-code-3`)
//! - `VOYAGE_DIMENSIONS`: Embedding dimensions (default: `1024`)
//! - `NEO4J_USERNAME`: Neo4j username (default: `neo4j`)
//! - `NEO4J_DATABASE`: Neo4j database name (default: `neo4j`)

mod config;
mod embedder;
mod knowledge_graph;
mod mock;
mod parser;

pub use config::{Neo4jConfig, VoyageConfig};
pub use embedder::{EmbeddingProvider, VoyageEmbedder};
pub use knowledge_graph::{
    IndexStats, KnowledgeGraph, LuauKnowledgeGraph, RelatedScript, ScriptNode, ScriptType,
    SearchResult,
};
pub use mock::{MockEmbeddingProvider, MockKnowledgeGraph};
pub use parser::{LuauParser, LuauRelationships};
