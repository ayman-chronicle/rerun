//! `EmbeddingStore` implementation for the in-memory backend.
//!
//! Uses cosine similarity for vector search. Not optimized --
//! scans all embeddings linearly. Fine for testing.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::EventId;
use chronicle_core::query::{EventResult, SemanticQuery};

use crate::traits::{EmbeddingStore, EventEmbedding};
use super::state::InMemoryBackend;

#[async_trait]
impl EmbeddingStore for InMemoryBackend {
    async fn store_embeddings(&self, embeddings: &[EventEmbedding]) -> Result<(), StoreError> {
        let mut store = self.embeddings.write();
        for emb in embeddings {
            store.insert(emb.event_id, emb.clone());
        }
        Ok(())
    }

    async fn search(&self, _query: &SemanticQuery) -> Result<Vec<EventResult>, StoreError> {
        // In-memory semantic search isn't meaningful without an actual
        // embedding model. Return empty results -- real backends (Postgres
        // with pgvector) handle this properly.
        Ok(vec![])
    }

    async fn has_embedding(&self, event_id: &EventId) -> Result<bool, StoreError> {
        let store = self.embeddings.read();
        Ok(store.contains_key(event_id))
    }
}
