//! Embedding pipeline for Chronicle.
//!
//! Extracts text content from events, generates vector embeddings
//! via a pluggable model, and stores them for semantic search.
//!
//! # Architecture
//!
//! ```text
//! Event → text_extractor → EmbeddingModel::embed → EmbeddingStore::store
//! ```
//!
//! The [`EmbeddingModel`] trait abstracts over the actual model
//! (OpenAI, local model, mock for tests). Implement it to plug in
//! any embedding provider.

pub mod model;
pub mod text_extractor;

use std::sync::Arc;

use chronicle_core::error::ChronicleError;
use chronicle_core::event::Event;
use chronicle_core::ids::OrgId;
use chronicle_store::traits::{EmbeddingStore, EventEmbedding};
use chronicle_store::StorageEngine;
use model::EmbeddingModel;

/// Processes events and generates embeddings.
///
/// Call [`EmbedService::embed_events`] to extract text from events,
/// generate embeddings via the configured model, and store them.
pub struct EmbedService {
    engine: StorageEngine,
    model: Arc<dyn EmbeddingModel>,
}

impl EmbedService {
    pub fn new(engine: StorageEngine, model: Arc<dyn EmbeddingModel>) -> Self {
        Self { engine, model }
    }

    /// Extract text from events, embed them, and store the vectors.
    ///
    /// Skips events that already have embeddings or have no
    /// extractable text content.
    pub async fn embed_events(&self, events: &[Event]) -> Result<usize, ChronicleError> {
        let mut to_embed: Vec<(chronicle_core::ids::EventId, OrgId, String)> = Vec::new();

        for event in events {
            let already_embedded = self
                .engine
                .embeddings
                .has_embedding(&event.org_id, &event.event_id)
                .await?;
            if already_embedded {
                continue;
            }

            if let Some(text) = text_extractor::extract_text(event) {
                to_embed.push((event.event_id, event.org_id, text));
            }
        }

        if to_embed.is_empty() {
            return Ok(0);
        }

        let texts: Vec<&str> = to_embed.iter().map(|(_, _, t)| t.as_str()).collect();
        let vectors = self.model.embed_batch(&texts).await.map_err(|e| {
            ChronicleError::External(format!("Embedding model error: {e}"))
        })?;

        let model_version = self.model.model_name().to_string();
        let embeddings: Vec<EventEmbedding> = to_embed
            .iter()
            .zip(vectors.iter())
            .map(|((event_id, org_id, text), vector)| EventEmbedding {
                event_id: *event_id,
                org_id: *org_id,
                embedding: vector.clone(),
                embedded_text: text.clone(),
                model_version: model_version.clone(),
            })
            .collect();

        let count = embeddings.len();
        self.engine.embeddings.store_embeddings(&embeddings).await?;

        tracing::debug!(count, "Stored event embeddings");
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_store::memory::InMemoryBackend;
    use chronicle_test_fixtures::factories;
    use model::MockEmbeddingModel;

    fn make_service() -> (EmbedService, Arc<InMemoryBackend>) {
        let backend = Arc::new(InMemoryBackend::new());
        let engine = StorageEngine {
            events: backend.clone(),
            entity_refs: backend.clone(),
            links: backend.clone(),
            embeddings: backend.clone(),
            schemas: backend.clone(),
            subscriptions: Some(backend.clone()),
        };
        let model = Arc::new(MockEmbeddingModel::new(4));
        (EmbedService::new(engine, model), backend)
    }

    #[tokio::test]
    async fn embeds_events_with_payload() {
        let (svc, backend) = make_service();

        let events = vec![
            factories::stripe_payment("org_1", "c1", 1000),
            factories::support_ticket("org_1", "c1", "Billing issue"),
        ];

        let count = svc.embed_events(&events).await.unwrap();
        assert_eq!(count, 2, "Both events have payload text");

        assert_eq!(backend.embedding_count(), 2);
    }

    #[tokio::test]
    async fn skips_already_embedded() {
        let (svc, _backend) = make_service();

        let events = vec![factories::stripe_payment("org_1", "c1", 1000)];

        let first = svc.embed_events(&events).await.unwrap();
        assert_eq!(first, 1);

        let second = svc.embed_events(&events).await.unwrap();
        assert_eq!(second, 0, "Should skip already-embedded events");
    }
}
