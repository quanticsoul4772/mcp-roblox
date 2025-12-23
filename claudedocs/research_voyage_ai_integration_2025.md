# Voyage AI Integration Research Report

**Date**: 2025-12-23
**Subject**: Enhancing Roblox Studio MCP Server with Voyage AI Embeddings
**Confidence**: High (based on official documentation and architectural analysis)

---

## Executive Summary

Voyage AI offers state-of-the-art embedding models that could significantly enhance the Roblox Studio MCP Server with semantic code search, intelligent script recommendations, and context-aware assistance. The **voyage-code-3** model is specifically optimized for code retrieval and outperforms competitors by 13-17% on code-related benchmarks. Integration is feasible through the existing HTTP client infrastructure.

---

## Voyage AI Capabilities Analysis

### Key Models for This Use Case

| Model | Purpose | Context | Dimensions | Best For |
|-------|---------|---------|------------|----------|
| **voyage-code-3** | Code retrieval | 32K tokens | 256-2048 | Luau script search, code similarity |
| **voyage-3-large** | General purpose | 120K tokens | 256-2048 | Documentation, mixed content |
| **voyage-multimodal-3** | Text + images | - | - | UI screenshots, asset analysis |

### voyage-code-3 Performance (Primary Recommendation)

- **Outperforms OpenAI-v3-large** by 13.80% average on code retrieval
- **Outperforms CodeSage-large** by 16.81%
- Supports quantization (int8, binary) for 4x-32x storage reduction
- Matryoshka embeddings: use 2048, 1024, 512, or 256 dimensions from same vector

### API Details

- **Endpoint**: `POST https://api.voyageai.com/v1/embeddings`
- **Pricing**: First 200M tokens FREE, then $0.06-$0.18 per million tokens
- **Rate limits**: 1000 inputs per request, 120K total tokens for code-3

---

## Current MCP Server Architecture Analysis

### Existing Integration Points

The server already has robust infrastructure for external API integration:

```
src/http/mod.rs          → HttpClient trait abstraction
src/http/reqwest_client.rs → Production HTTP client with pooling
src/cloud/client.rs      → API key management (secrecy crate)
src/cloud/traits.rs      → Trait-based dependency injection
```

**Key Advantage**: The `HttpClient` trait pattern means Voyage AI can be integrated as a new client following the same testable, mockable architecture.

### Existing Tool Categories

| Category | Count | Opportunity |
|----------|-------|-------------|
| Filesystem | 8 | Index scripts for semantic search |
| Studio | 14 | Search DataModel by semantic meaning |
| Cloud | 11 | - |
| Toolchain | 6 | - |
| **New: AI** | TBD | Embedding-powered tools |

---

## Recommended Enhancements

### 1. Semantic Script Search (`ai_search_scripts`)

**Purpose**: Natural language search across Luau scripts
**Example**: "Find scripts that handle player respawning" → Returns relevant scripts ranked by semantic similarity

**Implementation**:
```rust
// New tool: ai_search_scripts
async fn ai_search_scripts(
    &self,
    query: String,
    limit: usize,
) -> Result<Vec<ScriptSearchResult>, Error>
```

**Flow**:
1. On first use or file change, embed all `.luau` files with voyage-code-3
2. Store embeddings in local vector store (Qdrant or in-memory)
3. Embed query, perform similarity search
4. Return ranked script paths with relevance scores

**Use Cases**:
- "Find all combat damage calculations"
- "Show me data persistence code"
- "Where is remote event validation?"

---

### 2. Code Similarity Analysis (`ai_find_similar`)

**Purpose**: Find scripts similar to a given script or code snippet
**Example**: Given a module, find other modules with similar patterns

**Implementation**:
```rust
async fn ai_find_similar(
    &self,
    path: String,      // Script path OR
    code: String,      // Raw code snippet
    limit: usize,
) -> Result<Vec<SimilarityResult>, Error>
```

**Use Cases**:
- Detect duplicate/redundant code
- Find examples of similar patterns
- Identify refactoring opportunities

---

### 3. Contextual Documentation Lookup (`ai_lookup_docs`)

**Purpose**: Semantic search over Roblox API documentation
**Example**: "How do I make a part face the player smoothly?" → Returns CFrame.lookAt, TweenService docs

**Implementation**:
- Pre-embed Roblox Creator Hub documentation
- Store in bundled or cloud vector database
- Query with voyage-3-large for general text

---

### 4. Smart Code Completion Context (`ai_get_context`)

**Purpose**: Retrieve relevant code context for LLM code completion
**Example**: When editing a script, find the most relevant existing code to provide as context

**Implementation**:
```rust
async fn ai_get_context(
    &self,
    current_file: String,
    cursor_position: usize,
    context_tokens: usize,  // Budget for context
) -> Result<CodeContext, Error>
```

**Use Cases**:
- RAG for code completion
- Smarter autocomplete suggestions
- Project-aware coding assistance

---

### 5. Instance Search by Description (`ai_search_instances`)

**Purpose**: Search Studio DataModel by natural language description
**Example**: "Find all spawn points in the game" → Returns instances matching semantic intent

**Implementation**:
- Embed instance names + class types + property summaries
- Allow semantic search across live Studio DataModel

---

## Technical Architecture

### New Module Structure

```
src/
├── ai/
│   ├── mod.rs              # AI module root
│   ├── voyage_client.rs    # Voyage AI API client
│   ├── embeddings.rs       # Embedding operations
│   ├── vector_store.rs     # Local vector storage (Qdrant or in-memory)
│   └── indexer.rs          # Script/instance indexing
```

### Voyage Client Implementation

```rust
// src/ai/voyage_client.rs
pub struct VoyageClient {
    http_client: Arc<dyn HttpClient>,
    api_key: Secret<String>,
    model: String,  // "voyage-code-3"
}

impl VoyageClient {
    pub async fn embed(
        &self,
        texts: Vec<String>,
        input_type: InputType,  // Query or Document
        dimensions: u16,        // 256, 512, 1024, 2048
    ) -> Result<Vec<Vec<f32>>, Error> {
        // POST to https://api.voyageai.com/v1/embeddings
    }
}
```

### Vector Storage Options

| Option | Pros | Cons |
|--------|------|------|
| **In-memory (usearch)** | Fast, no deps, embedded | Lost on restart |
| **Qdrant** | Production-ready, Rust native | External process |
| **SQLite + vectors** | Persistent, simple | Slower for large datasets |

**Recommendation**: Start with in-memory for MVP, add Qdrant option for production.

### New Dependencies

```toml
# Cargo.toml additions
qdrant-client = { version = "1.9", optional = true }  # Vector DB client
usearch = "2.0"                                        # In-memory ANN
```

---

## Implementation Roadmap

### Phase 1: Foundation (2-3 weeks effort)

1. Add `VoyageClient` with embedding API support
2. Implement in-memory vector storage
3. Create `ai_search_scripts` tool
4. Add `VOYAGE_API_KEY` environment variable

### Phase 2: Enhanced Search (1-2 weeks)

5. Add `ai_find_similar` tool
6. Implement incremental indexing (file watcher integration)
7. Add embedding cache with persistence

### Phase 3: Advanced Features (2-3 weeks)

8. Add `ai_get_context` for RAG support
9. Implement `ai_search_instances` for DataModel
10. Add Qdrant integration option
11. Pre-bundle Roblox API documentation embeddings

---

## Configuration

### New Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `VOYAGE_API_KEY` | For AI tools | - | Voyage AI API key |
| `VOYAGE_MODEL` | No | `voyage-code-3` | Embedding model |
| `AI_VECTOR_STORE` | No | `memory` | `memory` or `qdrant` |
| `QDRANT_URL` | If qdrant | `http://localhost:6334` | Qdrant server URL |

### MCP Client Configuration

```json
{
  "mcpServers": {
    "roblox-studio": {
      "command": "path/to/roblox-studio-mcp",
      "env": {
        "ROBLOX_OPEN_CLOUD_API_KEY": "...",
        "VOYAGE_API_KEY": "your-voyage-key"
      }
    }
  }
}
```

---

## Cost Analysis

### Free Tier Coverage

- **200M tokens free** for voyage-code-3
- Average Luau script: ~500 tokens
- 200M / 500 = **400,000 scripts can be embedded for free**

### Ongoing Costs (After Free Tier)

| Operation | Tokens | Cost |
|-----------|--------|------|
| Index 1000 scripts (~500 tokens each) | 500K | $0.03 |
| 100 search queries (~50 tokens each) | 5K | $0.0003 |
| Full project reindex | ~1M | $0.06 |

**Conclusion**: Costs are negligible for typical usage.

---

## Alternatives Considered

| Alternative | Pros | Cons |
|-------------|------|------|
| **OpenAI Embeddings** | Popular, good docs | 2-3x more expensive, less code-optimized |
| **Cohere Embeddings** | Good multilingual | Not code-specialized |
| **Local Models (Ollama)** | Free, private | Requires GPU, slower |
| **CodeBERT/CodeT5** | Open source | Complex setup, less performant |

**Recommendation**: Voyage AI voyage-code-3 offers the best code retrieval performance at competitive pricing.

---

## Security Considerations

1. **API Key Protection**: Use `secrecy::Secret<String>` (existing pattern)
2. **No Code Leakage**: Embeddings are one-way, code cannot be reconstructed
3. **Optional Feature**: AI tools only activate with `VOYAGE_API_KEY` set
4. **Local Storage**: Embeddings stored locally, no cloud persistence required

---

## Conclusion

Voyage AI integration would add significant value to the Roblox Studio MCP Server:

- **Semantic search** across Luau codebases
- **Code similarity** detection for pattern matching
- **RAG support** for context-aware LLM assistance
- **Minimal cost** with generous free tier
- **Clean architecture** leveraging existing HTTP/trait infrastructure

The `voyage-code-3` model's 13-17% performance advantage over alternatives, combined with the server's existing injectable architecture, makes this an excellent enhancement opportunity.

---

## Sources

- [Voyage AI Documentation](https://docs.voyageai.com/docs/embeddings)
- [voyage-code-3 Blog Post](https://blog.voyageai.com/2024/12/04/voyage-code-3/)
- [voyage-3-large Announcement](https://blog.voyageai.com/2025/01/07/voyage-3-large/)
- [Qdrant Rust Client](https://docs.rs/qdrant-client)
- [Qdrant & Rust Tutorial](https://redandgreen.co.uk/qdrant-rust/)
- [Semantic Caching with Qdrant](https://www.shuttle.dev/blog/2024/05/30/semantic-caching-qdrant-rust)
- [Code Search with Vector Embeddings](https://huggingface.co/learn/cookbook/en/code_search)
- [Building Semantic Search](https://blog.maximeheckel.com/posts/building-magical-ai-powered-semantic-search/)
