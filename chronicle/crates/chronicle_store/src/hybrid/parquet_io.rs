//! Parquet I/O utilities for the hybrid backend.
//!
//! Re-exports the shared Arrow schema and conversion functions from
//! [`crate::arrow_export`] (DRY -- one source of truth for the schema).
//! Adds Parquet-specific path helpers.

use std::path::Path;

// Re-export the shared Arrow functions. All consumers (DataFrames,
// Parquet, Rerun bridge) go through the same code path.
pub use crate::arrow_export::{
    batches_to_event_results, event_arrow_schema, events_to_record_batch,
};

/// Build the Parquet directory path for a given (org, source, event_type).
pub fn parquet_dir_for_group(
    base: &Path,
    org_id: &str,
    source: &str,
    event_type: &str,
) -> std::path::PathBuf {
    base.join(org_id).join(source).join(event_type)
}
