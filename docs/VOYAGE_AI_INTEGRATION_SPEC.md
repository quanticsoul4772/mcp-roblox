# Voyage AI Integration Specification

## Overview

This specification defines the integration of Voyage AI embeddings into the Roblox Studio MCP server to enable semantic code search and intelligent assistance for Claude Code when working with Luau scripts.

**Primary User**: Claude Code (LLM assistant)
**Purpose**: Reduce token usage and improve code suggestions by retrieving semantically relevant code
**Storage Backend**: Neo4j (already installed)
**Embedding Model**: voyage-code-3

---

## 1. Architecture

### 1.1 System Context

```
┌─────────────────────────────────────────────────────────────────┐
│                      Claude Code (LLM)                          │
│                                                                 │
│  Current: Grep/Glob/Read → reads many files                    │
│  Future:  ai_search_codebase → retrieves relevant snippets     │
└──────────────────────────┬──────────────────────────────────────┘
                           │ MCP Protocol
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                    mcp-roblox Server                            │
├─────────────────────────────────────────────────────────────────┤
│  Existing Tools (40)     │  New AI Tools (4)                    │
│  ├── fs_*               │  ├── ai_search_codebase              │
│  ├── studio_*           │  ├── ai_find_related                 │
│  ├── cloud_*            │  ├── ai_get_context                  │
│  └── toolchain_*        │  └── ai_index_project                │
├─────────────────────────────────────────────────────────────────┤
│  src/ai/                                                        │
│  ├── mod.rs              # Feature-gated module                 │
│  ├── config.rs           # Neo4jConfig, VoyageConfig            │
│  ├── embedder.rs         # EmbeddingProvider trait + Voyage     │
│  ├── knowledge_graph.rs  # LuauKnowledgeGraph (Neo4j)           │
│  └── parser.rs           # Luau relationship extraction         │
└──────────────────────────┬──────────────────────────────────────┘
                           │
           ┌───────────────┴───────────────┐
           ▼                               ▼
┌─────────────────────┐         ┌─────────────────────┐
│    Voyage AI API    │         │       Neo4j         │
│  voyage-code-3      │         │  Vector indexes     │
│  1024 dimensions    │         │  Graph relationships│
└─────────────────────┘         └─────────────────────┘
```

### 1.2 Module Structure

```
src/
├── ai/
│   ├── mod.rs              # Module root, feature gate, re-exports
│   ├── config.rs           # Configuration structs
│   ├── embedder.rs         # EmbeddingProvider trait + VoyageEmbedder
│   ├── knowledge_graph.rs  # LuauKnowledgeGraph struct
│   ├── parser.rs           # Luau code analysis
│   └── mock.rs             # Mock implementations for testing
├── mcp/
│   ├── server.rs           # Add AI tools here
│   └── params.rs           # Add AI tool params here
└── ...
```

---

## 2. Configuration

### 2.1 Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `VOYAGE_API_KEY` | Yes (for AI) | - | Voyage AI API key |
| `VOYAGE_MODEL` | No | `voyage-code-3` | Embedding model |
| `VOYAGE_DIMENSIONS` | No | `1024` | Embedding dimensions |
| `NEO4J_URI` | Yes (for AI) | - | Neo4j connection URI |
| `NEO4J_USERNAME` | No | `neo4j` | Neo4j username |
| `NEO4J_PASSWORD` | Yes (for AI) | - | Neo4j password |
| `NEO4J_DATABASE` | No | `roblox` | Neo4j database name |

### 2.2 Cargo.toml Additions

```toml
[features]
default = []
ai = ["dep:neo4rs"]

[dependencies]
neo4rs = { version = "0.7", optional = true }
```

### 2.3 Config Structs

```rust
// src/ai/config.rs

/// Voyage AI configuration
#[derive(Clone)]
pub struct VoyageConfig {
    pub api_key: Secret<String>,
    pub model: String,
    pub dimensions: usize,
}

impl VoyageConfig {
    pub fn from_env() -> Result<Self, RobloxMcpError> {
        Ok(Self {
            api_key: Secret::new(
                std::env::var("VOYAGE_API_KEY")
                    .map_err(|_| RobloxMcpError::ConfigError("VOYAGE_API_KEY not set".into()))?
            ),
            model: std::env::var("VOYAGE_MODEL")
                .unwrap_or_else(|_| "voyage-code-3".to_string()),
            dimensions: std::env::var("VOYAGE_DIMENSIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1024),
        })
    }
}

/// Neo4j configuration
#[derive(Clone)]
pub struct Neo4jConfig {
    pub uri: String,
    pub username: String,
    pub password: Secret<String>,
    pub database: String,
}

impl Neo4jConfig {
    pub fn from_env() -> Result<Self, RobloxMcpError> {
        Ok(Self {
            uri: std::env::var("NEO4J_URI")
                .map_err(|_| RobloxMcpError::ConfigError("NEO4J_URI not set".into()))?,
            username: std::env::var("NEO4J_USERNAME")
                .unwrap_or_else(|_| "neo4j".to_string()),
            password: Secret::new(
                std::env::var("NEO4J_PASSWORD")
                    .map_err(|_| RobloxMcpError::ConfigError("NEO4J_PASSWORD not set".into()))?
            ),
            database: std::env::var("NEO4J_DATABASE")
                .unwrap_or_else(|_| "roblox".to_string()),
        })
    }
}
```

---

## 3. Core Traits and Types

### 3.1 EmbeddingProvider Trait

```rust
// src/ai/embedder.rs

use async_trait::async_trait;
use crate::error::RobloxMcpError;

/// Trait for embedding providers (enables testing with mocks)
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embedding vector for text
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RobloxMcpError>;

    /// Batch embed multiple texts (more efficient)
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RobloxMcpError>;

    /// Get the dimensionality of embeddings
    fn dimensions(&self) -> usize;
}
```

### 3.2 VoyageEmbedder Implementation

```rust
// src/ai/embedder.rs

use secrecy::{ExposeSecret, Secret};
use crate::http::HttpClient;

pub struct VoyageEmbedder<H: HttpClient> {
    http_client: Arc<H>,
    config: VoyageConfig,
}

impl<H: HttpClient> VoyageEmbedder<H> {
    const API_URL: &'static str = "https://api.voyageai.com/v1/embeddings";

    pub fn new(http_client: Arc<H>, config: VoyageConfig) -> Self {
        Self { http_client, config }
    }
}

#[async_trait]
impl<H: HttpClient> EmbeddingProvider for VoyageEmbedder<H> {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RobloxMcpError> {
        self.embed_batch(&[text]).await.map(|mut v| v.remove(0))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RobloxMcpError> {
        #[derive(serde::Serialize)]
        struct Request<'a> {
            input: Vec<&'a str>,
            model: &'a str,
            input_type: &'a str,
            output_dimension: usize,
        }

        #[derive(serde::Deserialize)]
        struct Response {
            data: Vec<EmbeddingData>,
        }

        #[derive(serde::Deserialize)]
        struct EmbeddingData {
            embedding: Vec<f32>,
        }

        let body = serde_json::json!({
            "input": texts,
            "model": self.config.model,
            "input_type": "document",
            "output_dimension": self.config.dimensions,
        });

        let headers = [
            ("Authorization", format!("Bearer {}", self.config.api_key.expose_secret()).as_str()),
            ("Content-Type", "application/json"),
        ];

        let response = self.http_client
            .post_json(Self::API_URL, &headers, body)
            .await?;

        if !response.is_success() {
            return Err(RobloxMcpError::CloudApiError(
                format!("Voyage API error: {}", response.status)
            ));
        }

        let parsed: Response = response.json()?;
        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }

    fn dimensions(&self) -> usize {
        self.config.dimensions
    }
}
```

### 3.3 LuauKnowledgeGraph Trait

```rust
// src/ai/knowledge_graph.rs

use async_trait::async_trait;

/// Script metadata stored in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptNode {
    pub path: String,
    pub name: String,
    pub script_type: ScriptType,  // Script, LocalScript, ModuleScript
    pub content_hash: String,      // For change detection
    pub line_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScriptType {
    Script,
    LocalScript,
    ModuleScript,
}

/// Search result from vector similarity
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub similarity_score: f64,
    pub snippet: String,  // Relevant code excerpt
}

/// Related script from graph traversal
#[derive(Debug, Clone)]
pub struct RelatedScript {
    pub path: String,
    pub relationship: String,  // "REQUIRES", "FIRES_REMOTE", etc.
    pub direction: String,     // "outgoing" or "incoming"
}

/// Trait for knowledge graph operations
#[async_trait]
pub trait KnowledgeGraph: Send + Sync {
    /// Index a script (store embedding + relationships)
    async fn index_script(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(), RobloxMcpError>;

    /// Remove a script from the index
    async fn remove_script(&self, path: &str) -> Result<(), RobloxMcpError>;

    /// Semantic search for scripts
    async fn search(
        &self,
        query: &str,
        limit: usize,
        min_similarity: f64,
    ) -> Result<Vec<SearchResult>, RobloxMcpError>;

    /// Find related scripts via graph traversal
    async fn find_related(
        &self,
        path: &str,
        max_depth: usize,
    ) -> Result<Vec<RelatedScript>, RobloxMcpError>;

    /// Get context snippets for a task description
    async fn get_context(
        &self,
        task_description: &str,
        token_budget: usize,
    ) -> Result<Vec<SearchResult>, RobloxMcpError>;

    /// Check if index is up to date
    async fn needs_reindex(&self, path: &str, content_hash: &str) -> Result<bool, RobloxMcpError>;

    /// Get indexing statistics
    async fn get_stats(&self) -> Result<IndexStats, RobloxMcpError>;
}

#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub total_scripts: usize,
    pub total_relationships: usize,
    pub last_indexed: Option<String>,
}
```

---

## 4. Neo4j Schema

### 4.1 Node Types

```cypher
// Script node - represents a Luau script file
(:Script {
    path: String,           // Unique identifier (file path)
    name: String,           // File name without extension
    script_type: String,    // "Script", "LocalScript", "ModuleScript"
    content_hash: String,   // SHA-256 of content for change detection
    line_count: Int,        // Number of lines
    embedding: [Float],     // voyage-code-3 embedding vector
    indexed_at: DateTime    // When this was last indexed
})

// Instance node - represents a Roblox instance referenced in code
(:Instance {
    path: String,           // e.g., "game.Workspace.SpawnLocation"
    class_name: String      // e.g., "SpawnLocation", "RemoteEvent"
})

// RemoteEvent/RemoteFunction nodes
(:Remote {
    path: String,
    remote_type: String     // "RemoteEvent", "RemoteFunction", "BindableEvent"
})
```

### 4.2 Relationship Types

```cypher
// Script requires another ModuleScript
(:Script)-[:REQUIRES {line: Int}]->(:Script)

// Script fires/invokes a remote
(:Script)-[:FIRES_REMOTE {line: Int, method: String}]->(:Remote)
// method: "FireServer", "FireClient", "InvokeServer", etc.

// Script connects to an event
(:Script)-[:CONNECTS_TO {line: Int, event: String}]->(:Instance)

// Script modifies/creates an instance
(:Script)-[:MODIFIES {line: Int, operation: String}]->(:Instance)
// operation: "create", "destroy", "set_property"
```

### 4.3 Indexes

```cypher
// Uniqueness constraints
CREATE CONSTRAINT script_path IF NOT EXISTS
FOR (s:Script) REQUIRE s.path IS UNIQUE;

CREATE CONSTRAINT instance_path IF NOT EXISTS
FOR (i:Instance) REQUIRE i.path IS UNIQUE;

CREATE CONSTRAINT remote_path IF NOT EXISTS
FOR (r:Remote) REQUIRE r.path IS UNIQUE;

// Vector index for semantic search
CREATE VECTOR INDEX script_embeddings IF NOT EXISTS
FOR (s:Script) ON (s.embedding)
OPTIONS {
    indexConfig: {
        `vector.dimensions`: 1024,
        `vector.similarity_function`: 'cosine'
    }
};

// Text index for name search fallback
CREATE TEXT INDEX script_names IF NOT EXISTS
FOR (s:Script) ON (s.name);
```

---

## 5. MCP Tool Specifications

### 5.1 ai_search_codebase

**Purpose**: Semantic search across Luau scripts
**Primary Value**: Replace many Grep/Read calls with single semantic query

```rust
// Parameter struct
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AiSearchCodebaseParams {
    /// Natural language query describing what to find
    /// Examples: "player damage calculation", "data saving code", "remote event handlers"
    pub query: String,

    /// Maximum number of results (default: 5)
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Minimum similarity score 0.0-1.0 (default: 0.5)
    #[serde(default = "default_min_similarity")]
    pub min_similarity: f64,
}

fn default_limit() -> usize { 5 }
fn default_min_similarity() -> f64 { 0.5 }

// Response
#[derive(Serialize)]
pub struct AiSearchCodebaseResult {
    pub results: Vec<CodeSearchResult>,
    pub query_embedding_time_ms: u64,
    pub search_time_ms: u64,
}

#[derive(Serialize)]
pub struct CodeSearchResult {
    pub path: String,
    pub name: String,
    pub similarity: f64,
    pub snippet: String,      // Most relevant ~10 lines
    pub line_range: (usize, usize),
}
```

**Tool Description**:
```
Search Luau scripts by semantic meaning. Returns scripts matching natural language queries
like "find player respawn logic" or "data persistence code". More accurate than keyword grep
for conceptual searches.
```

### 5.2 ai_find_related

**Purpose**: Find scripts related to a given script via graph relationships
**Primary Value**: When editing a script, automatically discover dependencies

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AiFindRelatedParams {
    /// Path to the script to find relationships for
    pub path: String,

    /// Maximum relationship depth to traverse (default: 2)
    #[serde(default = "default_depth")]
    pub max_depth: usize,

    /// Filter by relationship type (optional)
    /// Options: "REQUIRES", "FIRES_REMOTE", "CONNECTS_TO", "MODIFIES"
    pub relationship_type: Option<String>,
}

fn default_depth() -> usize { 2 }

#[derive(Serialize)]
pub struct AiFindRelatedResult {
    pub script: String,
    pub related: Vec<RelatedScriptResult>,
}

#[derive(Serialize)]
pub struct RelatedScriptResult {
    pub path: String,
    pub relationship: String,
    pub direction: String,  // "requires_this", "required_by", etc.
    pub depth: usize,
}
```

**Tool Description**:
```
Find scripts related to a given script through code relationships (requires, remote calls,
event connections). Useful for understanding dependencies before making changes.
```

### 5.3 ai_get_context

**Purpose**: Get relevant code context for a task description
**Primary Value**: RAG context retrieval for code generation

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AiGetContextParams {
    /// Description of the task or question
    pub task: String,

    /// Approximate token budget for context (default: 2000)
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,

    /// Prefer scripts related to this path (optional)
    pub focus_path: Option<String>,
}

fn default_token_budget() -> usize { 2000 }

#[derive(Serialize)]
pub struct AiGetContextResult {
    pub context_snippets: Vec<ContextSnippet>,
    pub total_tokens: usize,
    pub scripts_searched: usize,
}

#[derive(Serialize)]
pub struct ContextSnippet {
    pub path: String,
    pub relevance: f64,
    pub code: String,
    pub reason: String,  // Why this was included
}
```

**Tool Description**:
```
Get relevant code snippets for a task description. Returns the most useful context within
a token budget. Use when you need examples or patterns from the user's codebase.
```

### 5.4 ai_index_project

**Purpose**: Manually trigger project reindexing
**Primary Value**: Force refresh after major changes

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AiIndexProjectParams {
    /// Only reindex files that have changed (default: true)
    #[serde(default = "default_incremental")]
    pub incremental: bool,

    /// Force reindex even if content unchanged (default: false)
    #[serde(default)]
    pub force: bool,
}

fn default_incremental() -> bool { true }

#[derive(Serialize)]
pub struct AiIndexProjectResult {
    pub scripts_indexed: usize,
    pub scripts_skipped: usize,
    pub relationships_extracted: usize,
    pub duration_ms: u64,
    pub errors: Vec<String>,
}
```

**Tool Description**:
```
Index or reindex the project's Luau scripts for semantic search. Usually happens
automatically, but can be triggered manually after major changes.
```

---

## 6. Luau Parser Specification

### 6.1 Relationship Extraction Patterns

```rust
// src/ai/parser.rs

/// Patterns to extract from Luau code
pub struct LuauRelationships {
    pub requires: Vec<RequireRelation>,
    pub remote_calls: Vec<RemoteCallRelation>,
    pub event_connections: Vec<EventConnectionRelation>,
    pub instance_modifications: Vec<InstanceModification>,
}

pub struct RequireRelation {
    pub line: usize,
    pub module_path: String,  // e.g., "game.ReplicatedStorage.Modules.Combat"
}

pub struct RemoteCallRelation {
    pub line: usize,
    pub remote_path: String,
    pub method: String,  // "FireServer", "InvokeServer", etc.
}

pub struct EventConnectionRelation {
    pub line: usize,
    pub event_path: String,
    pub method: String,  // "Connect", "Once", etc.
}

pub struct InstanceModification {
    pub line: usize,
    pub instance_path: String,
    pub operation: String,  // "create", "destroy", "set_property"
}
```

### 6.2 Regex Patterns

```rust
lazy_static! {
    // require(game.ReplicatedStorage.Modules.Combat)
    // require(script.Parent.Utils)
    static ref REQUIRE_PATTERN: Regex = Regex::new(
        r#"require\s*\(\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*\)"#
    ).unwrap();

    // RemoteEvent:FireServer(), RemoteFunction:InvokeServer()
    static ref REMOTE_CALL_PATTERN: Regex = Regex::new(
        r#"([a-zA-Z_][a-zA-Z0-9_.]*)\s*:\s*(Fire(?:Server|Client|AllClients)|Invoke(?:Server|Client))\s*\("#
    ).unwrap();

    // event:Connect(function), event.Changed:Connect()
    static ref EVENT_CONNECT_PATTERN: Regex = Regex::new(
        r#"([a-zA-Z_][a-zA-Z0-9_.]*)\s*:\s*(Connect|Once|Wait)\s*\("#
    ).unwrap();

    // Instance.new("Part"), workspace.Part.CFrame =
    static ref INSTANCE_NEW_PATTERN: Regex = Regex::new(
        r#"Instance\.new\s*\(\s*["']([^"']+)["']\s*\)"#
    ).unwrap();
}
```

---

## 7. Integration with Existing Architecture

### 7.1 Server Initialization

```rust
// src/main.rs or src/startup.rs

#[cfg(feature = "ai")]
async fn initialize_ai_components() -> Option<Arc<dyn KnowledgeGraph>> {
    match (VoyageConfig::from_env(), Neo4jConfig::from_env()) {
        (Ok(voyage_config), Ok(neo4j_config)) => {
            let http_client = Arc::new(ReqwestHttpClient::new()?);
            let embedder = Arc::new(VoyageEmbedder::new(http_client, voyage_config));

            match LuauKnowledgeGraph::new(neo4j_config, embedder).await {
                Ok(kg) => {
                    tracing::info!("AI knowledge graph initialized");
                    Some(Arc::new(kg))
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize AI: {}", e);
                    None
                }
            }
        }
        _ => {
            tracing::debug!("AI features disabled (missing VOYAGE_API_KEY or NEO4J_* config)");
            None
        }
    }
}
```

### 7.2 Tool Registration

```rust
// src/mcp/server.rs

impl<B, L, F, R, W, M> RobloxMcpServer<B, L, F, R, W, M>
where
    B: StudioBridge + Clone,
    // ... other bounds
{
    #[cfg(feature = "ai")]
    #[tool(description = "Search Luau scripts by semantic meaning")]
    async fn ai_search_codebase(
        &self,
        Parameters(params): Parameters<AiSearchCodebaseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("ai_search_codebase");

        let Some(kg) = &self.knowledge_graph else {
            return Err(ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                "AI features not configured (set VOYAGE_API_KEY and NEO4J_* env vars)",
            ));
        };

        let result = kg.search(&params.query, params.limit, params.min_similarity).await;
        call.finish_with(result).await
    }

    // ... other AI tools
}
```

### 7.3 Auto-Indexing Hook

```rust
// Integration with file watcher

impl FileWatcher {
    #[cfg(feature = "ai")]
    pub fn with_knowledge_graph(mut self, kg: Arc<dyn KnowledgeGraph>) -> Self {
        self.on_change(move |event| {
            if event.path.extension() == Some("luau") {
                let kg = kg.clone();
                tokio::spawn(async move {
                    if let Err(e) = kg.index_script(&event.path, &event.content).await {
                        tracing::warn!("Failed to index {}: {}", event.path, e);
                    }
                });
            }
        });
        self
    }
}
```

---

## 8. Testing Strategy

### 8.1 Mock Implementations

```rust
// src/ai/mock.rs

pub struct MockEmbeddingProvider {
    pub dimension: usize,
    pub responses: Mutex<VecDeque<Result<Vec<f32>, RobloxMcpError>>>,
}

impl MockEmbeddingProvider {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            responses: Mutex::new(VecDeque::new()),
        }
    }

    pub fn queue_response(&self, response: Result<Vec<f32>, RobloxMcpError>) {
        self.responses.lock().unwrap().push_back(response);
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, RobloxMcpError> {
        self.responses.lock().unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(vec![0.0; self.dimension]))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RobloxMcpError> {
        let mut results = Vec::new();
        for _ in texts {
            results.push(self.embed("").await?);
        }
        Ok(results)
    }

    fn dimensions(&self) -> usize {
        self.dimension
    }
}
```

### 8.2 Integration Tests

```rust
// tests/ai_integration.rs

#[tokio::test]
#[ignore] // Requires Neo4j and Voyage API
async fn test_full_indexing_and_search() {
    let kg = create_test_knowledge_graph().await;

    // Index test scripts
    kg.index_script("src/Combat.luau", r#"
        local Damage = require(game.ReplicatedStorage.Modules.Damage)
        function calculateDamage(player, weapon)
            return Damage.calculate(weapon.BaseDamage * player.Stats.Multiplier)
        end
    "#).await.unwrap();

    // Semantic search
    let results = kg.search("damage calculation", 5, 0.5).await.unwrap();
    assert!(!results.is_empty());
    assert!(results[0].path.contains("Combat"));
}
```

---

## 9. Performance Considerations

### 9.1 Embedding Caching

- Cache embeddings in Neo4j (stored with script node)
- Content hash comparison to skip unchanged files
- Batch embedding requests (up to 128 texts per call)

### 9.2 Query Optimization

- Use Neo4j vector index for similarity search
- Limit graph traversal depth (default: 2)
- Return snippets, not full file contents

### 9.3 Resource Limits

```rust
pub const MAX_EMBEDDING_BATCH: usize = 128;
pub const MAX_SCRIPT_SIZE: usize = 100_000;  // 100KB
pub const MAX_SEARCH_RESULTS: usize = 20;
pub const MAX_CONTEXT_TOKENS: usize = 8000;
pub const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.5;
```

---

## 10. Error Handling

### 10.1 Graceful Degradation

- AI tools return helpful error if not configured
- File watcher continues if indexing fails
- Search falls back to name matching if embedding fails

### 10.2 Error Types

```rust
// Add to src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum RobloxMcpError {
    // ... existing variants

    #[error("AI configuration error: {0}")]
    AiConfigError(String),

    #[error("Embedding API error: {0}")]
    EmbeddingError(String),

    #[error("Knowledge graph error: {0}")]
    KnowledgeGraphError(String),
}
```

---

## 11. Implementation Phases

### Phase 1: Foundation (Week 1)
- [ ] Add `ai` feature flag to Cargo.toml
- [ ] Create `src/ai/mod.rs` with feature gate
- [ ] Implement `VoyageConfig` and `Neo4jConfig`
- [ ] Port `EmbeddingProvider` trait and `VoyageEmbedder`
- [ ] Add mock implementations

### Phase 2: Knowledge Graph (Week 2)
- [ ] Implement `LuauKnowledgeGraph` with Neo4j
- [ ] Create Neo4j schema and indexes
- [ ] Implement `index_script` and `search` methods
- [ ] Add basic Luau relationship parser

### Phase 3: MCP Tools (Week 3)
- [ ] Add `ai_search_codebase` tool
- [ ] Add `ai_find_related` tool
- [ ] Add `ai_get_context` tool
- [ ] Add `ai_index_project` tool
- [ ] Integrate with file watcher for auto-indexing

### Phase 4: Testing & Polish (Week 4)
- [ ] Unit tests for all components
- [ ] Integration tests with Neo4j
- [ ] Documentation updates
- [ ] Performance optimization

---

## 12. Success Metrics

| Metric | Target |
|--------|--------|
| Search latency (p50) | < 200ms |
| Indexing throughput | > 50 scripts/sec |
| Search relevance | > 80% precision@5 |
| Token reduction | 50% fewer Read calls for code discovery |

---

## Appendix A: Example Queries

```
// What Claude Code asks → What gets returned

"Find player damage calculation"
→ Scripts containing damage/health logic, sorted by relevance

"Where is data saved?"
→ Scripts with DataStore/ProfileService patterns

"Remote events for combat"
→ Scripts firing combat-related RemoteEvents

"What requires the Inventory module?"
→ Graph traversal: all scripts with REQUIRES→Inventory
```
