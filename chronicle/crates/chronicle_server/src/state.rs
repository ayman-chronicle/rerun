//! Server state shared across all route handlers.

use std::sync::Arc;

use chronicle_embed::EmbedService;
use chronicle_ingest::{Batcher, BatchConfig};
use chronicle_link::LinkService;
use chronicle_query::QueryService;
use chronicle_store::StorageEngine;

/// Shared state for all axum handlers. Passed via `State<ServerState>`.
///
/// Each service is constructed once at startup and shared across all
/// requests via `Arc`.
#[derive(Clone)]
pub struct ServerState {
    pub query: QueryService,
    pub link: LinkService,
    pub engine: StorageEngine,
}

impl ServerState {
    /// Create server state from a storage engine.
    ///
    /// Services are constructed from the engine. The batcher and
    /// embed service are created separately since they need additional
    /// config (batch thresholds, embedding model).
    pub fn new(engine: StorageEngine) -> Self {
        Self {
            query: QueryService::new(engine.clone()),
            link: LinkService::new(engine.clone()),
            engine,
        }
    }
}
