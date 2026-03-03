//! Embedding model trait and implementations.
//!
//! The [`EmbeddingModel`] trait abstracts over embedding providers.
//! Implement it for OpenAI, Cohere, local models, etc.

use async_trait::async_trait;

/// Trait for generating vector embeddings from text.
///
/// Implementations handle batching, retries, and rate limiting
/// internally. The caller provides text and receives vectors.
#[async_trait]
pub trait EmbeddingModel: Send + Sync + 'static {
    /// A human-readable model identifier (e.g., "text-embedding-3-small").
    fn model_name(&self) -> &str;

    /// The dimensionality of output vectors.
    fn dimensions(&self) -> usize;

    /// Embed a batch of texts. Returns one vector per input text.
    ///
    /// The output vectors must have exactly [`Self::dimensions`] elements.
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}

/// Error from the embedding model.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("model API error: {0}")]
    ApiError(String),

    #[error("rate limited, retry after {retry_after_secs} seconds")]
    RateLimited { retry_after_secs: u64 },

    #[error("input too long: {length} chars (max {max_length})")]
    InputTooLong { length: usize, max_length: usize },
}

/// Mock embedding model for testing.
///
/// Returns deterministic vectors (hash of input text) with configurable
/// dimensionality. Not semantically meaningful -- just for testing the
/// pipeline.
pub struct MockEmbeddingModel {
    dimensions: usize,
}

impl MockEmbeddingModel {
    pub fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }
}

#[async_trait]
impl EmbeddingModel for MockEmbeddingModel {
    fn model_name(&self) -> &str {
        "mock-embedding-model"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts
            .iter()
            .map(|text| {
                // Deterministic pseudo-embedding from text hash
                let hash = simple_hash(text);
                (0..self.dimensions)
                    .map(|i| ((hash.wrapping_add(i as u64) % 1000) as f32) / 1000.0)
                    .collect()
            })
            .collect())
    }
}

/// Simple non-cryptographic hash for deterministic mock embeddings.
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(byte));
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_model_produces_correct_dimensions() {
        let model = MockEmbeddingModel::new(128);
        let vectors = model.embed_batch(&["hello", "world"]).await.unwrap();
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].len(), 128);
        assert_eq!(vectors[1].len(), 128);
    }

    #[tokio::test]
    async fn mock_model_is_deterministic() {
        let model = MockEmbeddingModel::new(4);
        let v1 = model.embed_batch(&["test"]).await.unwrap();
        let v2 = model.embed_batch(&["test"]).await.unwrap();
        assert_eq!(v1, v2, "Same input should produce same output");
    }

    #[tokio::test]
    async fn mock_model_different_inputs_differ() {
        let model = MockEmbeddingModel::new(4);
        let vectors = model.embed_batch(&["hello", "world"]).await.unwrap();
        assert_ne!(vectors[0], vectors[1], "Different inputs should differ");
    }
}
