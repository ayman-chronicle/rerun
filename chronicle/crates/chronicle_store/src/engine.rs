//! The [`StorageEngine`] bundles all storage traits into one handle.
//!
//! All service crates receive a `StorageEngine` and access the backend
//! through it. This is the single point of configuration -- swapping
//! `PostgresBackend` for `HybridBackend` only changes how the engine
//! is constructed.

use std::sync::Arc;

use crate::traits::{
    EmbeddingStore, EntityRefStore, EventLinkStore, EventStore, SchemaRegistry,
};

/// Bundles all storage trait objects into one handle.
///
/// Passed to every service at startup. Services access storage
/// exclusively through these trait objects, never through a concrete
/// backend type.
///
/// # Example
///
/// ```ignore
/// // Phase 1: all traits implemented by PostgresBackend
/// let pg = Arc::new(PostgresBackend::new(db_url).await?);
/// let engine = StorageEngine {
///     events: pg.clone(),
///     entity_refs: pg.clone(),
///     links: pg.clone(),
///     embeddings: pg.clone(),
///     schemas: pg.clone(),
/// };
///
/// // Phase 2: swap to hybrid without changing any service code
/// let hybrid = Arc::new(HybridBackend::new(pg, parquet_dir).await?);
/// let engine = StorageEngine {
///     events: hybrid.clone(),
///     entity_refs: hybrid.clone(),  // still delegates to Postgres
///     links: hybrid.clone(),
///     embeddings: hybrid.clone(),
///     schemas: hybrid.clone(),
/// };
/// ```
#[derive(Clone)]
pub struct StorageEngine {
    pub events: Arc<dyn EventStore>,
    pub entity_refs: Arc<dyn EntityRefStore>,
    pub links: Arc<dyn EventLinkStore>,
    pub embeddings: Arc<dyn EmbeddingStore>,
    pub schemas: Arc<dyn SchemaRegistry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify `StorageEngine` is `Send + Sync + Clone` -- required for
    /// passing across async task boundaries and sharing between handlers.
    fn assert_send_sync_clone<T: Send + Sync + Clone>() {}

    #[test]
    fn storage_engine_is_send_sync_clone() {
        assert_send_sync_clone::<StorageEngine>();
    }
}
