//! PostgreSQL storage backend.
//!
//! Implements all storage traits against Postgres using sqlx.
//! This is the Phase 1 production backend.
//!
//! Write path is optimized for throughput: events go into a single
//! transactional UNNEST with deferred WAL, while entity_refs are
//! populated asynchronously. Call [`PostgresBackend::flush_pending_refs`]
//! in tests to wait for async ref writes to complete.

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
///
/// Write path: events are inserted in a single transactional UNNEST with
/// deferred WAL sync. Entity refs are written synchronously for small
/// batches (< 2000 events) and asynchronously for large batches.
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
        // Admin mode: disable RLS filtering during migrations.
        sqlx::raw_sql("SET LOCAL app.current_org_id = ''")
            .execute(&self.pool)
            .await
            .ok();

        sqlx::raw_sql(include_str!("../../migrations/001_initial.sql"))
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Migration(e.to_string()))?;
        sqlx::raw_sql(include_str!(
            "../../migrations/002_entity_refs_jsonb_index.sql"
        ))
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Migration(e.to_string()))?;
        sqlx::raw_sql(include_str!(
            "../../migrations/003_row_level_security.sql"
        ))
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Migration(e.to_string()))?;
        Ok(())
    }

    /// Set the tenant context for the current connection.
    ///
    /// Must be called before any tenant-scoped operation when RLS is
    /// enabled. The Postgres RLS policies check `app.current_org_id`.
    pub async fn set_tenant(&self, org_id: &str) -> Result<(), StoreError> {
        sqlx::query(&format!("SET LOCAL app.current_org_id = '{}'", org_id.replace('\'', "''")))
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }
}
