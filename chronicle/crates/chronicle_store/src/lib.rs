//! Storage trait abstractions for Chronicle.
//!
//! This crate defines the traits that every storage backend must implement.
//! All service crates (`chronicle_ingest`, `chronicle_query`, `chronicle_link`,
//! `chronicle_embed`, `chronicle_server`) depend on these traits -- never on
//! a concrete backend.
//!
//! # Backend Implementations
//!
//! - **Phase 1**: `PostgresBackend` -- everything in Postgres + pgvector
//! - **Phase 2**: `HybridBackend` -- hot in Postgres, cold in Parquet via DataFusion
//! - **Phase 3**: `KurrentBackend` -- Kurrent for ingestion + subscriptions, Postgres sidecar
//!
//! All backends must pass the same trait test suite in `chronicle_test_fixtures`.
//!
//! # Architecture
//!
//! ```text
//! chronicle_server / chronicle_sdk
//!         |
//!         v
//!    StorageEngine (bundles all trait objects)
//!         |
//!    +----+----+----+----+
//!    |    |    |    |    |
//!    v    v    v    v    v
//! Events Refs Links Embed Schema  <-- traits
//!    |    |    |    |    |
//!    v    v    v    v    v
//! PostgresBackend / HybridBackend / KurrentBackend  <-- impls
//! ```

pub mod engine;
#[cfg(feature = "hybrid")]
pub mod hybrid;
pub mod memory;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod traits;

pub use engine::StorageEngine;
pub use traits::*;
