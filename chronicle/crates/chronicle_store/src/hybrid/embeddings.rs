//! `EmbeddingStore` for the hybrid backend -- delegates entirely to Postgres.
//!
//! Embeddings and pgvector indices always live in Postgres.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventId, OrgId};
use chronicle_core::query::{EventResult, SemanticQuery};

use crate::traits::{EmbeddingStore, EventEmbedding};
use super::HybridBackend;

#[async_trait]
impl EmbeddingStore for HybridBackend {
    async fn store_embeddings(&self, embeddings: &[EventEmbedding]) -> Result<(), StoreError> {
        self.pg.store_embeddings(embeddings).await
    }

    async fn search(&self, query: &SemanticQuery) -> Result<Vec<EventResult>, StoreError> {
        self.pg.search(query).await
    }

    async fn has_embedding(&self, org_id: &OrgId, event_id: &EventId) -> Result<bool, StoreError> {
        self.pg.has_embedding(org_id, event_id).await
    }
}
