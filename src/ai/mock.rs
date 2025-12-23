//! Mock implementations for testing AI features.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::RobloxMcpError;

use super::embedder::EmbeddingProvider;
use super::knowledge_graph::{IndexStats, KnowledgeGraph, RelatedScript, SearchResult};

/// Mock embedding provider for testing.
pub struct MockEmbeddingProvider {
    dimension: usize,
    responses: Mutex<VecDeque<Result<Vec<f32>, RobloxMcpError>>>,
}

impl MockEmbeddingProvider {
    /// Create a new mock with the given embedding dimension.
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            responses: Mutex::new(VecDeque::new()),
        }
    }

    /// Queue a response to be returned on the next embed call.
    pub fn queue_response(&self, response: Result<Vec<f32>, RobloxMcpError>) {
        self.responses.lock().unwrap().push_back(response);
    }

    /// Queue multiple responses for batch operations.
    pub fn queue_batch_response(&self, responses: Vec<Result<Vec<f32>, RobloxMcpError>>) {
        let mut queue = self.responses.lock().unwrap();
        for response in responses {
            queue.push_back(response);
        }
    }

    /// Generate a deterministic embedding for testing (based on text hash).
    pub fn deterministic_embedding(text: &str, dimension: usize) -> Vec<f32> {
        let mut embedding = vec![0.0f32; dimension];
        for (i, byte) in text.bytes().enumerate() {
            embedding[i % dimension] += (byte as f32) / 255.0;
        }
        // Normalize
        let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if magnitude > 0.0 {
            for x in &mut embedding {
                *x /= magnitude;
            }
        }
        embedding
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RobloxMcpError> {
        let queued = self.responses.lock().unwrap().pop_front();
        match queued {
            Some(response) => response,
            None => Ok(Self::deterministic_embedding(text, self.dimension)),
        }
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RobloxMcpError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    fn dimensions(&self) -> usize {
        self.dimension
    }
}

/// Mock knowledge graph for testing.
pub struct MockKnowledgeGraph {
    indexed_scripts: Mutex<Vec<(String, String)>>, // (path, content)
    search_results: Mutex<VecDeque<Vec<SearchResult>>>,
    related_results: Mutex<VecDeque<Vec<RelatedScript>>>,
}

impl MockKnowledgeGraph {
    /// Create a new mock knowledge graph.
    pub fn new() -> Self {
        Self {
            indexed_scripts: Mutex::new(Vec::new()),
            search_results: Mutex::new(VecDeque::new()),
            related_results: Mutex::new(VecDeque::new()),
        }
    }

    /// Queue search results to be returned.
    pub fn queue_search_results(&self, results: Vec<SearchResult>) {
        self.search_results.lock().unwrap().push_back(results);
    }

    /// Queue related script results to be returned.
    pub fn queue_related_results(&self, results: Vec<RelatedScript>) {
        self.related_results.lock().unwrap().push_back(results);
    }

    /// Get all indexed scripts (for verification in tests).
    pub fn get_indexed_scripts(&self) -> Vec<(String, String)> {
        self.indexed_scripts.lock().unwrap().clone()
    }
}

impl Default for MockKnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl KnowledgeGraph for MockKnowledgeGraph {
    async fn index_script(&self, path: &str, content: &str) -> Result<(), RobloxMcpError> {
        self.indexed_scripts
            .lock()
            .unwrap()
            .push((path.to_string(), content.to_string()));
        Ok(())
    }

    async fn remove_script(&self, path: &str) -> Result<(), RobloxMcpError> {
        self.indexed_scripts
            .lock()
            .unwrap()
            .retain(|(p, _)| p != path);
        Ok(())
    }

    async fn search(
        &self,
        _query: &str,
        limit: usize,
        _min_similarity: f64,
    ) -> Result<Vec<SearchResult>, RobloxMcpError> {
        let queued = self.search_results.lock().unwrap().pop_front();
        match queued {
            Some(mut results) => {
                results.truncate(limit);
                Ok(results)
            }
            None => Ok(Vec::new()),
        }
    }

    async fn find_related(
        &self,
        _path: &str,
        _max_depth: usize,
    ) -> Result<Vec<RelatedScript>, RobloxMcpError> {
        let queued = self.related_results.lock().unwrap().pop_front();
        Ok(queued.unwrap_or_default())
    }

    async fn get_context(
        &self,
        task: &str,
        token_budget: usize,
    ) -> Result<Vec<SearchResult>, RobloxMcpError> {
        let limit = (token_budget / 500).clamp(3, 10);
        self.search(task, limit, 0.4).await
    }

    async fn needs_reindex(&self, path: &str, _content_hash: &str) -> Result<bool, RobloxMcpError> {
        let indexed = self.indexed_scripts.lock().unwrap();
        Ok(!indexed.iter().any(|(p, _)| p == path))
    }

    async fn get_stats(&self) -> Result<IndexStats, RobloxMcpError> {
        let scripts = self.indexed_scripts.lock().unwrap().len();
        Ok(IndexStats {
            total_scripts: scripts,
            total_relationships: 0,
            last_indexed: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_embedding_provider_deterministic() {
        let provider = MockEmbeddingProvider::new(128);

        let emb1 = provider.embed("test code").await.unwrap();
        let emb2 = provider.embed("test code").await.unwrap();

        assert_eq!(emb1, emb2);
        assert_eq!(emb1.len(), 128);
    }

    #[tokio::test]
    async fn test_mock_embedding_provider_queued() {
        let provider = MockEmbeddingProvider::new(3);
        provider.queue_response(Ok(vec![1.0, 2.0, 3.0]));

        let result = provider.embed("anything").await.unwrap();
        assert_eq!(result, vec![1.0, 2.0, 3.0]);
    }

    #[tokio::test]
    async fn test_mock_knowledge_graph_index() {
        let kg = MockKnowledgeGraph::new();

        kg.index_script("test.luau", "local x = 1").await.unwrap();
        kg.index_script("test2.luau", "local y = 2").await.unwrap();

        let indexed = kg.get_indexed_scripts();
        assert_eq!(indexed.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_knowledge_graph_search() {
        let kg = MockKnowledgeGraph::new();
        kg.queue_search_results(vec![
            SearchResult {
                path: "combat.luau".to_string(),
                name: "combat".to_string(),
                similarity_score: 0.9,
                snippet: "damage calculation".to_string(),
                line_range: (1, 10),
            },
        ]);

        let results = kg.search("damage", 5, 0.5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "combat.luau");
    }
}
