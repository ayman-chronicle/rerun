//! Event ingestion pipeline for Chronicle.
//!
//! This crate handles the flow from receiving raw events to persisting
//! them in the storage backend:
//!
//! 1. **Receive** events via SDK, REST, or webhook
//! 2. **Batch** them for efficient writes (micro-batcher)
//! 3. **Write** to the `StorageEngine`
//! 4. **Detect** schemas for the schema registry
//!
//! # Micro-Batcher
//!
//! The [`Batcher`] accumulates events and flushes when any threshold is hit:
//! - Row count exceeds `max_rows`
//! - Byte size exceeds `max_bytes`
//! - Time since last flush exceeds `flush_interval`
//!
//! This design is adapted from Rerun's `ChunkBatcher`.

pub mod batcher;
pub mod schema_detector;

pub use batcher::{Batcher, BatchConfig};
