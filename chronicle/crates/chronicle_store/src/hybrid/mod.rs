//! Hybrid storage backend: hot data in Postgres, cold data in Parquet.
//!
//! The [`HybridBackend`] wraps a [`PostgresBackend`] for hot/recent events
//! and archives cold events to Parquet files queried via DataFusion.
//!
//! Entity refs, links, embeddings, and schemas always stay in Postgres.
//! Only the events table is split between hot (Postgres) and cold (Parquet).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐
//! │  Ingestion   │  → always writes to Postgres (hot)
//! └──────┬───────┘
//!        v
//! ┌─────────────┐    ┌────────────────┐
//! │  Postgres    │    │  Parquet/       │
//! │  (hot events)│    │  DataFusion     │
//! │  + metadata  │    │  (cold events)  │
//! └──────┬───────┘    └────────┬───────┘
//!        │                     │
//!        └──────────┬──────────┘
//!                   v
//!            Query Router
//!         (hot/cold/fan-out)
//! ```

pub mod archiver;
mod embeddings;
mod entity_refs;
mod events;
mod links;
pub mod parquet_io;
mod schemas;

use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};

use chronicle_core::error::StoreError;

use crate::postgres::PostgresBackend;

/// Hybrid storage backend: Postgres for hot data, Parquet for cold.
///
/// Insert always goes to Postgres. The archiver periodically moves
/// old events to Parquet. Queries route between hot and cold based
/// on the time range, fanning out and merging when necessary.
pub struct HybridBackend {
    pub(crate) pg: PostgresBackend,
    pub(crate) parquet_dir: PathBuf,
    pub(crate) archive_after: Duration,
}

impl HybridBackend {
    /// Access the underlying Postgres pool for administrative operations.
    pub fn pg_pool(&self) -> &sqlx::PgPool {
        &self.pg.pool
    }
}

impl HybridBackend {
    /// Create a new hybrid backend.
    ///
    /// - `database_url`: Postgres connection string for the hot store
    /// - `parquet_dir`: local directory (or S3 prefix) for cold Parquet files
    /// - `archive_after`: events older than this are eligible for archival
    pub async fn new(
        database_url: &str,
        parquet_dir: PathBuf,
        archive_after: Duration,
    ) -> Result<Self, StoreError> {
        let pg = PostgresBackend::new(database_url).await?;
        Ok(Self::from_postgres(pg, parquet_dir, archive_after))
    }

    /// Wrap an existing Postgres backend with hybrid capabilities.
    pub fn from_postgres(pg: PostgresBackend, parquet_dir: PathBuf, archive_after: Duration) -> Self {
        Self {
            pg,
            parquet_dir,
            archive_after,
        }
    }

    /// Run both Phase 1 (create tables) and Phase 2 (drop FKs) migrations.
    pub async fn run_migrations(&self) -> Result<(), StoreError> {
        self.pg.run_migrations().await?;

        sqlx::raw_sql(include_str!("../../migrations/002_drop_fk_for_archival.sql"))
            .execute(&self.pg.pool)
            .await
            .map_err(|e| StoreError::Migration(e.to_string()))?;

        Ok(())
    }

    /// The timestamp before which events are considered "cold".
    pub fn archive_cutoff(&self) -> DateTime<Utc> {
        Utc::now() - self.archive_after
    }
}
