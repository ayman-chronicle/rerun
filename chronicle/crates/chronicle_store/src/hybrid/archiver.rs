//! Background archiver: moves cold events from Postgres to Parquet.
//!
//! The archiver reads events older than [`HybridBackend::archive_cutoff`],
//! groups them by `(org_id, source, event_type)`, writes each group to
//! a Parquet file, and then deletes the archived events from Postgres.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use datafusion::parquet::arrow::ArrowWriter;
use datafusion::parquet::file::properties::WriterProperties;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::EventId;

use super::parquet_io::{events_to_record_batch, parquet_dir_for_group};
use super::HybridBackend;
use crate::postgres::events::row_to_event;

/// Report from a single archive run.
#[derive(Debug, Default)]
pub struct ArchiveReport {
    pub events_archived: u64,
    pub parquet_files_written: u64,
}

/// Grouping key for archive batches.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct ArchiveGroup {
    org_id: String,
    source: String,
    event_type: String,
}

impl HybridBackend {
    /// Archive events older than the cutoff from Postgres to Parquet.
    ///
    /// 1. Reads archivable events grouped by (org_id, source, event_type).
    /// 2. Writes each group as a Parquet file.
    /// 3. Deletes the archived events from Postgres.
    ///
    /// Entity refs, links, and embeddings remain in Postgres --
    /// they reference event_ids that now live in Parquet.
    pub async fn archive_cold_events(&self) -> Result<ArchiveReport, StoreError> {
        let cutoff = self.archive_cutoff();
        let mut report = ArchiveReport::default();

        let events = self.fetch_archivable_events(cutoff).await?;
        if events.is_empty() {
            return Ok(report);
        }

        let groups = group_events(events);

        for (group, events) in &groups {
            let dir = parquet_dir_for_group(
                &self.parquet_dir,
                &group.org_id,
                &group.source,
                &group.event_type,
            );
            std::fs::create_dir_all(&dir)
                .map_err(|e| StoreError::Internal(format!("mkdir {}: {e}", dir.display())))?;

            let filename = format!("batch_{}.parquet", EventId::new());
            let path = dir.join(filename);

            write_events_parquet(&path, events)?;
            report.parquet_files_written += 1;
        }

        let event_ids: Vec<String> = groups
            .values()
            .flat_map(|evts| evts.iter().map(|e| e.event_id.to_string()))
            .collect();

        self.delete_archived_events(&event_ids).await?;
        report.events_archived = event_ids.len() as u64;

        tracing::info!(
            archived = report.events_archived,
            files = report.parquet_files_written,
            "archive run complete"
        );

        Ok(report)
    }

    async fn fetch_archivable_events(&self, cutoff: DateTime<Utc>) -> Result<Vec<Event>, StoreError> {
        let rows = sqlx::query(
            "SELECT event_id, org_id, source, topic, event_type, event_time, \
             ingestion_time, payload, media_type, media_ref, media_blob, \
             media_size_bytes, raw_body \
             FROM events \
             WHERE event_time < $1 \
             ORDER BY org_id, source, event_type, event_time ASC",
        )
        .bind(cutoff)
        .fetch_all(&self.pg.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(row_to_event).collect())
    }

    async fn delete_archived_events(&self, event_ids: &[String]) -> Result<(), StoreError> {
        if event_ids.is_empty() {
            return Ok(());
        }

        // Delete in batches to avoid oversized IN clauses.
        for chunk in event_ids.chunks(500) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("${i}")).collect();
            let sql = format!(
                "DELETE FROM events WHERE event_id IN ({})",
                placeholders.join(",")
            );

            let mut query = sqlx::query(&sql);
            for id in chunk {
                query = query.bind(id);
            }
            query
                .execute(&self.pg.pool)
                .await
                .map_err(|e| StoreError::Internal(e.to_string()))?;
        }

        Ok(())
    }
}

fn group_events(events: Vec<Event>) -> HashMap<ArchiveGroup, Vec<Event>> {
    let mut groups: HashMap<ArchiveGroup, Vec<Event>> = HashMap::new();
    for event in events {
        let key = ArchiveGroup {
            org_id: event.org_id.as_str().to_string(),
            source: event.source.as_str().to_string(),
            event_type: event.event_type.as_str().to_string(),
        };
        groups.entry(key).or_default().push(event);
    }
    groups
}

fn write_events_parquet(path: &PathBuf, events: &[Event]) -> Result<(), StoreError> {
    let batch = events_to_record_batch(events)?;
    let file = std::fs::File::create(path)
        .map_err(|e| StoreError::Internal(format!("create {}: {e}", path.display())))?;

    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))
        .map_err(|e| StoreError::Internal(e.to_string()))?;

    writer
        .write(&batch)
        .map_err(|e| StoreError::Internal(e.to_string()))?;
    writer
        .close()
        .map_err(|e| StoreError::Internal(e.to_string()))?;

    Ok(())
}
