//! `EmbeddingStore` implementation for Postgres.
//!
//! Uses `REAL[]` columns for vector storage. For production vector search,
//! pgvector extension with HNSW index would replace the array-based approach.

use async_trait::async_trait;
use sqlx::Row;

use chronicle_core::error::StoreError;
use chronicle_core::ids::*;
use chronicle_core::query::{EventResult, SemanticQuery};

use crate::traits::{EmbeddingStore, EventEmbedding};
use super::PostgresBackend;

#[async_trait]
impl EmbeddingStore for PostgresBackend {
    async fn store_embeddings(&self, embeddings: &[EventEmbedding]) -> Result<(), StoreError> {
        for emb in embeddings {
            sqlx::query(
                "INSERT INTO event_embeddings (event_id, org_id, embedding, embedded_text, model_version)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (event_id) DO UPDATE SET embedding = $3, model_version = $5"
            )
            .bind(emb.event_id.to_string())
            .bind(emb.org_id.as_str())
            .bind(&emb.embedding)
            .bind(&emb.embedded_text)
            .bind(&emb.model_version)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    async fn search(&self, _query: &SemanticQuery) -> Result<Vec<EventResult>, StoreError> {
        // Full vector search requires pgvector extension.
        // This placeholder returns empty results.
        // Production: use <-> operator with HNSW index.
        Ok(vec![])
    }

    async fn has_embedding(&self, event_id: &EventId) -> Result<bool, StoreError> {
        let row = sqlx::query("SELECT 1 FROM event_embeddings WHERE event_id = $1")
            .bind(event_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;
        Ok(row.is_some())
    }
}
