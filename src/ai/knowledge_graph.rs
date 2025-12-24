//! Knowledge graph trait and Neo4j implementation for Luau code storage.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::RobloxMcpError;

use super::embedder::EmbeddingProvider;
use super::parser::LuauParser;

/// Type of Roblox script.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScriptType {
    Script,
    LocalScript,
    ModuleScript,
}

impl ScriptType {
    /// Infer script type from file path.
    pub fn from_path(path: &str) -> Self {
        let lower = path.to_lowercase();
        if lower.contains(".client.") || lower.contains("localscript") {
            ScriptType::LocalScript
        } else if lower.contains(".server.") || lower.contains("serverscript") {
            ScriptType::Script
        } else {
            ScriptType::ModuleScript
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ScriptType::Script => "Script",
            ScriptType::LocalScript => "LocalScript",
            ScriptType::ModuleScript => "ModuleScript",
        }
    }
}

/// Metadata for a script stored in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptNode {
    /// File path (unique identifier)
    pub path: String,
    /// File name without extension
    pub name: String,
    /// Type of script
    pub script_type: ScriptType,
    /// SHA-256 hash of content for change detection
    pub content_hash: String,
    /// Number of lines in the script
    pub line_count: usize,
}

/// Result from semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// File path
    pub path: String,
    /// Script name
    pub name: String,
    /// Similarity score (0.0 to 1.0)
    pub similarity_score: f64,
    /// Relevant code snippet
    pub snippet: String,
    /// Line range of the snippet
    pub line_range: (usize, usize),
}

/// Related script from graph traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedScript {
    /// File path
    pub path: String,
    /// Relationship type (REQUIRES, FIRES_REMOTE, etc.)
    pub relationship: String,
    /// Direction relative to source script
    pub direction: String,
    /// Depth in graph traversal
    pub depth: usize,
}

/// Statistics about the knowledge graph index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    /// Total number of indexed scripts
    pub total_scripts: usize,
    /// Total number of relationships
    pub total_relationships: usize,
    /// Timestamp of last indexing operation
    pub last_indexed: Option<String>,
}

/// Trait for knowledge graph operations.
#[async_trait]
pub trait KnowledgeGraph: Send + Sync {
    /// Index a script (store embedding + extract relationships).
    async fn index_script(&self, path: &str, content: &str) -> Result<(), RobloxMcpError>;

    /// Remove a script from the index.
    async fn remove_script(&self, path: &str) -> Result<(), RobloxMcpError>;

    /// Semantic search for scripts.
    async fn search(
        &self,
        query: &str,
        limit: usize,
        min_similarity: f64,
    ) -> Result<Vec<SearchResult>, RobloxMcpError>;

    /// Find scripts related via graph relationships.
    async fn find_related(
        &self,
        path: &str,
        max_depth: usize,
    ) -> Result<Vec<RelatedScript>, RobloxMcpError>;

    /// Get context snippets for a task description within a token budget.
    async fn get_context(
        &self,
        task: &str,
        token_budget: usize,
    ) -> Result<Vec<SearchResult>, RobloxMcpError>;

    /// Check if a script needs reindexing based on content hash.
    async fn needs_reindex(&self, path: &str, content_hash: &str) -> Result<bool, RobloxMcpError>;

    /// Get indexing statistics.
    async fn get_stats(&self) -> Result<IndexStats, RobloxMcpError>;
}

/// Neo4j-backed knowledge graph for Luau scripts.
pub struct LuauKnowledgeGraph<E: EmbeddingProvider> {
    graph: Arc<neo4rs::Graph>,
    embedder: Arc<E>,
    parser: LuauParser,
}

impl<E: EmbeddingProvider> LuauKnowledgeGraph<E> {
    /// Create a new knowledge graph connected to Neo4j.
    pub async fn new(
        config: super::config::Neo4jConfig,
        embedder: Arc<E>,
    ) -> Result<Self, RobloxMcpError> {
        let graph = neo4rs::Graph::new(&config.uri, &config.username, config.password())
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Neo4j connection failed: {}", e)))?;

        // Initialize schema
        Self::init_schema(&graph).await?;

        Ok(Self {
            graph: Arc::new(graph),
            embedder,
            parser: LuauParser::new(),
        })
    }

    /// Initialize Neo4j schema (constraints and indexes).
    async fn init_schema(graph: &neo4rs::Graph) -> Result<(), RobloxMcpError> {
        // Create constraints
        let constraints = [
            "CREATE CONSTRAINT script_path IF NOT EXISTS FOR (s:Script) REQUIRE s.path IS UNIQUE",
            "CREATE CONSTRAINT instance_path IF NOT EXISTS FOR (i:Instance) REQUIRE i.path IS UNIQUE",
            "CREATE CONSTRAINT remote_path IF NOT EXISTS FOR (r:Remote) REQUIRE r.path IS UNIQUE",
        ];

        for constraint in constraints {
            graph
                .run(neo4rs::query(constraint))
                .await
                .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to create constraint: {}", e)))?;
        }

        // Create vector index for embeddings
        // Note: Vector index creation syntax may vary by Neo4j version
        let vector_index = r#"
            CREATE VECTOR INDEX script_embeddings IF NOT EXISTS
            FOR (s:Script) ON (s.embedding)
            OPTIONS {
                indexConfig: {
                    `vector.dimensions`: 1024,
                    `vector.similarity_function`: 'cosine'
                }
            }
        "#;

        // Ignore errors for vector index as it may already exist or syntax may differ
        let _ = graph.run(neo4rs::query(vector_index)).await;

        Ok(())
    }

    /// Compute content hash for change detection.
    fn content_hash(content: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Extract the most relevant snippet from content.
    fn extract_snippet(content: &str, max_lines: usize) -> (String, usize, usize) {
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        if total <= max_lines {
            return (content.to_string(), 1, total);
        }

        // Find the "most interesting" section (functions, not just comments)
        let mut best_start = 0;
        let mut best_score = 0;

        for i in 0..total.saturating_sub(max_lines) {
            let mut score = 0;
            for line in lines.iter().skip(i).take(max_lines) {
                if line.contains("function") {
                    score += 3;
                }
                if line.contains("local") {
                    score += 1;
                }
                if line.contains("return") {
                    score += 2;
                }
                if line.trim().starts_with("--") {
                    score -= 1;
                }
            }
            if score > best_score {
                best_score = score;
                best_start = i;
            }
        }

        let snippet: String = lines[best_start..best_start + max_lines].join("\n");
        (snippet, best_start + 1, best_start + max_lines)
    }
}

#[async_trait]
impl<E: EmbeddingProvider + 'static> KnowledgeGraph for LuauKnowledgeGraph<E> {
    async fn index_script(&self, path: &str, content: &str) -> Result<(), RobloxMcpError> {
        let content_hash = Self::content_hash(content);

        // Check if already indexed with same content
        if !self.needs_reindex(path, &content_hash).await? {
            return Ok(());
        }

        // Generate embedding
        let embedding = self.embedder.embed(content).await?;

        // Parse relationships
        let relationships = self.parser.parse(content);

        // Extract metadata
        let name = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let script_type = ScriptType::from_path(path);
        let line_count = content.lines().count();

        // Upsert script node
        let upsert_query = neo4rs::query(
            r#"
            MERGE (s:Script {path: $path})
            SET s.name = $name,
                s.script_type = $script_type,
                s.content_hash = $content_hash,
                s.line_count = $line_count,
                s.embedding = $embedding,
                s.indexed_at = datetime()
            "#,
        )
        .param("path", path)
        .param("name", name)
        .param("script_type", script_type.as_str())
        .param("content_hash", content_hash)
        .param("line_count", line_count as i64)
        .param("embedding", embedding);

        self.graph
            .run(upsert_query)
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to upsert script: {}", e)))?;

        // Create relationships
        for req in &relationships.requires {
            let query = neo4rs::query(
                r#"
                MATCH (s:Script {path: $source})
                MERGE (t:Script {path: $target})
                MERGE (s)-[r:REQUIRES {line: $line}]->(t)
                "#,
            )
            .param("source", path)
            .param("target", req.module_path.clone())
            .param("line", req.line as i64);

            let _ = self.graph.run(query).await;
        }

        for remote in &relationships.remote_calls {
            let query = neo4rs::query(
                r#"
                MATCH (s:Script {path: $source})
                MERGE (r:Remote {path: $remote_path})
                SET r.remote_type = 'RemoteEvent'
                MERGE (s)-[rel:FIRES_REMOTE {line: $line, method: $method}]->(r)
                "#,
            )
            .param("source", path)
            .param("remote_path", remote.remote_path.clone())
            .param("line", remote.line as i64)
            .param("method", remote.method.clone());

            let _ = self.graph.run(query).await;
        }

        Ok(())
    }

    async fn remove_script(&self, path: &str) -> Result<(), RobloxMcpError> {
        let query = neo4rs::query(
            r#"
            MATCH (s:Script {path: $path})
            DETACH DELETE s
            "#,
        )
        .param("path", path);

        self.graph
            .run(query)
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to remove script: {}", e)))?;

        Ok(())
    }

    async fn search(
        &self,
        query_text: &str,
        limit: usize,
        min_similarity: f64,
    ) -> Result<Vec<SearchResult>, RobloxMcpError> {
        // Generate query embedding
        let query_embedding = self.embedder.embed(query_text).await?;

        // Vector similarity search
        let query = neo4rs::query(
            r#"
            CALL db.index.vector.queryNodes('script_embeddings', $limit, $embedding)
            YIELD node, score
            WHERE score >= $min_similarity
            RETURN node.path AS path, node.name AS name, score
            ORDER BY score DESC
            LIMIT $limit
            "#,
        )
        .param("embedding", query_embedding)
        .param("limit", limit as i64)
        .param("min_similarity", min_similarity);

        let mut result = self
            .graph
            .execute(query)
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Search failed: {}", e)))?;

        let mut results = Vec::new();
        while let Some(row) = result
            .next()
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to read row: {}", e)))?
        {
            let path: String = row.get("path").unwrap_or_default();
            let name: String = row.get("name").unwrap_or_default();
            let score: f64 = row.get("score").unwrap_or(0.0);

            results.push(SearchResult {
                path,
                name,
                similarity_score: score,
                snippet: String::new(), // Would need to read file content
                line_range: (1, 1),
            });
        }

        Ok(results)
    }

    async fn find_related(
        &self,
        path: &str,
        max_depth: usize,
    ) -> Result<Vec<RelatedScript>, RobloxMcpError> {
        let query = neo4rs::query(
            r#"
            MATCH (source:Script {path: $path})
            CALL apoc.path.subgraphNodes(source, {
                maxLevel: $max_depth,
                relationshipFilter: 'REQUIRES|FIRES_REMOTE|CONNECTS_TO|MODIFIES'
            }) YIELD node
            WHERE node <> source AND node:Script
            WITH node,
                 [(source)-[r]->(node) | type(r)] AS outRels,
                 [(node)-[r]->(source) | type(r)] AS inRels
            RETURN node.path AS path,
                   CASE
                       WHEN size(outRels) > 0 THEN outRels[0]
                       ELSE inRels[0]
                   END AS relationship,
                   CASE
                       WHEN size(outRels) > 0 THEN 'outgoing'
                       ELSE 'incoming'
                   END AS direction
            "#,
        )
        .param("path", path)
        .param("max_depth", max_depth as i64);

        let mut result = self
            .graph
            .execute(query)
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Find related failed: {}", e)))?;

        let mut related = Vec::new();
        while let Some(row) = result
            .next()
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to read row: {}", e)))?
        {
            let path: String = row.get("path").unwrap_or_default();
            let relationship: String = row.get("relationship").unwrap_or_default();
            let direction: String = row.get("direction").unwrap_or_default();

            related.push(RelatedScript {
                path,
                relationship,
                direction,
                depth: 1, // Simplified for now
            });
        }

        Ok(related)
    }

    async fn get_context(
        &self,
        task: &str,
        token_budget: usize,
    ) -> Result<Vec<SearchResult>, RobloxMcpError> {
        // Estimate: ~4 chars per token, so divide by 4
        let estimated_snippets = token_budget / 500; // ~500 tokens per snippet
        let limit = estimated_snippets.clamp(3, 10);

        self.search(task, limit, 0.4).await
    }

    async fn needs_reindex(&self, path: &str, content_hash: &str) -> Result<bool, RobloxMcpError> {
        let query = neo4rs::query(
            r#"
            MATCH (s:Script {path: $path})
            RETURN s.content_hash AS hash
            "#,
        )
        .param("path", path);

        let mut result = self
            .graph
            .execute(query)
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to check hash: {}", e)))?;

        if let Some(row) = result
            .next()
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to read row: {}", e)))?
        {
            let stored_hash: String = row.get("hash").unwrap_or_default();
            return Ok(stored_hash != content_hash);
        }

        // Script not indexed yet
        Ok(true)
    }

    async fn get_stats(&self) -> Result<IndexStats, RobloxMcpError> {
        let query = neo4rs::query(
            r#"
            MATCH (s:Script)
            WITH count(s) AS scripts
            MATCH ()-[r]->()
            RETURN scripts, count(r) AS relationships
            "#,
        );

        let mut result = self
            .graph
            .execute(query)
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to get stats: {}", e)))?;

        if let Some(row) = result
            .next()
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Failed to read row: {}", e)))?
        {
            let scripts: i64 = row.get("scripts").unwrap_or(0);
            let relationships: i64 = row.get("relationships").unwrap_or(0);

            return Ok(IndexStats {
                total_scripts: scripts as usize,
                total_relationships: relationships as usize,
                last_indexed: None,
            });
        }

        Ok(IndexStats::default())
    }
}
