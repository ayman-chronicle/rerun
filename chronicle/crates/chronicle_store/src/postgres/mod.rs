//! PostgreSQL storage backend.
//!
//! Implements all storage traits against Postgres using sqlx.
//! This is the Phase 1 production backend.

pub(crate) mod events;
mod entity_refs;
mod links;
mod embeddings;
pub mod query_builder;
mod schemas;

use chronicle_core::error::StoreError;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Postgres-backed storage. Implements all storage traits.
///
/// Create via [`PostgresBackend::new`] with a database URL, then
/// call [`PostgresBackend::run_migrations`] to set up the schema.
#[derive(Clone)]
pub struct PostgresBackend {
    pub(crate) pool: PgPool,
}

impl PostgresBackend {
    /// Connect to Postgres and create a tuned connection pool.
    pub async fn new(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .min_connections(2)
            .connect(database_url)
            .await
            .map_err(|e| StoreError::Connection(e.to_string()))?;
        Ok(Self { pool })
    }

    /// Create from an existing pool (useful for testing).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Access the connection pool (for administrative operations in tests).
    pub fn pg_pool(&self) -> &PgPool {
        &self.pool
    }

    /// Run database migrations to create/update the schema.
    pub async fn run_migrations(&self) -> Result<(), StoreError> {
        sqlx::raw_sql(include_str!("../../migrations/001_initial.sql"))
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Migration(e.to_string()))?;
        Ok(())
    }
}
