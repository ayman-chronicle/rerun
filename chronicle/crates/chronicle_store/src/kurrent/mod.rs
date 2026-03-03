//! KurrentDB storage backend.
//!
//! Events are appended to Kurrent streams for real-time push subscriptions
//! and global ordering. A Postgres sidecar stores entity refs, links,
//! embeddings, and schemas (Kurrent has no relational/vector capabilities).
//!
//! The dual-write strategy:
//! - `insert_events` writes to BOTH Kurrent streams AND Postgres
//! - All queries go through Postgres (the Kurrent Postgres Sink connector
//!   would eliminate the Postgres write in production)
//! - Subscriptions use Kurrent's native persistent subscriptions

mod embeddings;
mod entity_refs;
mod events;
mod links;
mod schemas;
pub mod subscriptions;

use chronicle_core::error::StoreError;
use kurrentdb::Client as KurrentClient;

use crate::postgres::PostgresBackend;

/// KurrentDB-backed storage with Postgres sidecar.
///
/// Kurrent provides: event append, global ordering, push subscriptions.
/// Postgres provides: entity refs, links, embeddings, schemas, analytics.
pub struct KurrentBackend {
    pub(crate) kurrent: KurrentClient,
    pub(crate) pg: PostgresBackend,
}

impl KurrentBackend {
    /// Connect to both KurrentDB and Postgres.
    pub async fn new(kurrent_url: &str, postgres_url: &str) -> Result<Self, StoreError> {
        let settings: kurrentdb::ClientSettings = kurrent_url
            .parse()
            .map_err(|e: kurrentdb::ClientSettingsParseError| {
                StoreError::Connection(e.to_string())
            })?;

        let kurrent =
            KurrentClient::new(settings).map_err(|e| StoreError::Connection(e.to_string()))?;

        let pg = PostgresBackend::new(postgres_url).await?;

        Ok(Self { kurrent, pg })
    }

    /// Run Postgres migrations (Kurrent needs no schema setup).
    pub async fn run_migrations(&self) -> Result<(), StoreError> {
        self.pg.run_migrations().await
    }

    /// Access the Postgres pool for administrative operations.
    pub fn pg_pool(&self) -> &sqlx::PgPool {
        self.pg.pg_pool()
    }

    /// Access the Kurrent client for direct operations.
    pub fn kurrent_client(&self) -> &KurrentClient {
        &self.kurrent
    }
}
