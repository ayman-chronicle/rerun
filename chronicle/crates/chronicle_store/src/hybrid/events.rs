//! `EventStore` implementation for the hybrid backend.
//!
//! Routes queries between Postgres (hot) and Parquet/DataFusion (cold)
//! based on the time range relative to the archive cutoff.

use async_trait::async_trait;
use datafusion::prelude::*;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::{EntityId, EntityType, EventId, OrgId};
use chronicle_core::query::{EventResult, OrderBy, StructuredQuery, TimelineQuery};

use crate::traits::EventStore;
use super::parquet_io::batches_to_event_results;
use super::HybridBackend;

#[async_trait]
impl EventStore for HybridBackend {
    /// Insert always goes to Postgres (the hot store).
    async fn insert_events(&self, events: &[Event]) -> Result<Vec<EventId>, StoreError> {
        self.pg.insert_events(events).await
    }

    /// Try Postgres first (hot), then fall back to Parquet (cold).
    async fn get_event(&self, org_id: &OrgId, id: &EventId) -> Result<Option<EventResult>, StoreError> {
        if let Some(result) = self.pg.get_event(org_id, id).await? {
            return Ok(Some(result));
        }
        self.get_event_from_parquet(org_id, id).await
    }

    /// Route based on time range: hot, cold, or fan-out.
    async fn query_structured(&self, query: &StructuredQuery) -> Result<Vec<EventResult>, StoreError> {
        let cutoff = self.archive_cutoff();

        match &query.time_range {
            Some(range) if range.min() >= cutoff => {
                self.pg.query_structured(query).await
            }
            Some(range) if range.max() < cutoff => {
                self.query_cold_structured(query).await
            }
            _ => {
                let (hot, cold) = tokio::join!(
                    self.pg.query_structured(query),
                    self.query_cold_structured(query),
                );
                merge_results(hot?, cold?, &query.order_by, query.limit)
            }
        }
    }

    /// Route timeline queries between hot and cold.
    async fn query_timeline(&self, query: &TimelineQuery) -> Result<Vec<EventResult>, StoreError> {
        let cutoff = self.archive_cutoff();

        match &query.time_range {
            Some(range) if range.min() >= cutoff => {
                self.pg.query_timeline(query).await
            }
            Some(range) if range.max() < cutoff => {
                self.query_cold_timeline(query).await
            }
            _ => {
                let (hot, cold) = tokio::join!(
                    self.pg.query_timeline(query),
                    self.query_cold_timeline(query),
                );
                merge_results(hot?, cold?, &OrderBy::EventTimeAsc, usize::MAX)
            }
        }
    }

    /// Run arbitrary SQL over both hot (Postgres) and cold (Parquet) data
    /// via DataFusion. Hot events are loaded into a MemTable, cold events
    /// come from Parquet files. Both are unified into an `events` view.
    async fn query_sql(&self, org_id: &OrgId, sql: &str) -> Result<Vec<EventResult>, StoreError> {
        self.execute_datafusion_sql(org_id, sql).await
    }

    async fn count(&self, query: &StructuredQuery) -> Result<u64, StoreError> {
        let cutoff = self.archive_cutoff();

        match &query.time_range {
            Some(range) if range.min() >= cutoff => {
                self.pg.count(query).await
            }
            Some(range) if range.max() < cutoff => {
                let results = self.query_cold_structured(query).await?;
                Ok(results.len() as u64)
            }
            _ => {
                let (hot, cold) = tokio::join!(
                    self.pg.count(query),
                    self.query_cold_structured(query),
                );
                Ok(hot? + cold?.len() as u64)
            }
        }
    }
}

impl HybridBackend {
    /// Query cold (Parquet) data for a single event by ID.
    async fn get_event_from_parquet(
        &self,
        org_id: &OrgId,
        event_id: &EventId,
    ) -> Result<Option<EventResult>, StoreError> {
        let ctx = SessionContext::new();
        let parquet_dir = self.parquet_dir.clone();

        if !parquet_dir.exists() {
            return Ok(None);
        }

        let table_name = "cold_events";
        self.register_all_parquet(&ctx, table_name).await?;

        let sql = format!(
            "SELECT * FROM {table_name} WHERE org_id = '{org}' AND event_id = '{eid}' LIMIT 1",
            org = org_id.as_str(),
            eid = event_id,
        );

        let df = ctx
            .sql(&sql)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;
        let batches = df
            .collect()
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        let results = batches_to_event_results(&batches);
        Ok(results.into_iter().next())
    }

    /// Query cold (Parquet) data with structured filters via DataFusion.
    pub async fn query_cold_structured(
        &self,
        query: &StructuredQuery,
    ) -> Result<Vec<EventResult>, StoreError> {
        let ctx = SessionContext::new();

        if !self.parquet_dir.exists() {
            return Ok(vec![]);
        }

        let table_name = "cold_events";
        self.register_all_parquet(&ctx, table_name).await?;

        let mut conditions = vec![format!("org_id = '{}'", query.org_id.as_str())];

        if let Some(ref s) = query.source {
            conditions.push(format!("source = '{}'", s.as_str()));
        }
        if let Some(ref t) = query.event_type {
            conditions.push(format!("event_type = '{}'", t.as_str()));
        }
        if let Some(ref range) = query.time_range {
            conditions.push(format!(
                "event_time >= '{}'",
                range.min().to_rfc3339()
            ));
            conditions.push(format!(
                "event_time <= '{}'",
                range.max().to_rfc3339()
            ));
        }

        if let Some((ref etype, ref eid)) = query.entity {
            let event_ids = self.pg_event_ids_for_entity(
                &query.org_id,
                etype,
                eid,
            ).await?;
            if event_ids.is_empty() {
                return Ok(vec![]);
            }
            let id_list: Vec<String> = event_ids.iter().map(|id| format!("'{id}'")).collect();
            conditions.push(format!("event_id IN ({})", id_list.join(",")));
        }

        let where_clause = conditions.join(" AND ");
        let order = match query.order_by {
            OrderBy::EventTimeAsc => "event_time ASC",
            OrderBy::EventTimeDesc => "event_time DESC",
            OrderBy::IngestionTimeAsc => "ingestion_time ASC",
            OrderBy::IngestionTimeDesc => "ingestion_time DESC",
        };

        let sql = format!(
            "SELECT * FROM {table_name} WHERE {where_clause} ORDER BY {order} LIMIT {}",
            query.limit
        );

        let df = ctx
            .sql(&sql)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;
        let batches = df
            .collect()
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(batches_to_event_results(&batches))
    }

    /// Query cold timeline data via DataFusion.
    async fn query_cold_timeline(
        &self,
        query: &TimelineQuery,
    ) -> Result<Vec<EventResult>, StoreError> {
        let event_ids = self
            .pg_event_ids_for_entity(
                &query.org_id,
                &query.entity_type,
                &query.entity_id,
            )
            .await?;

        if event_ids.is_empty() {
            return Ok(vec![]);
        }

        let ctx = SessionContext::new();
        if !self.parquet_dir.exists() {
            return Ok(vec![]);
        }

        let table_name = "cold_events";
        self.register_all_parquet(&ctx, table_name).await?;

        let id_list: Vec<String> = event_ids.iter().map(|id| format!("'{id}'")).collect();
        let mut conditions = vec![
            format!("org_id = '{}'", query.org_id.as_str()),
            format!("event_id IN ({})", id_list.join(",")),
        ];

        if let Some(ref range) = query.time_range {
            conditions.push(format!("event_time >= '{}'", range.min().to_rfc3339()));
            conditions.push(format!("event_time <= '{}'", range.max().to_rfc3339()));
        }

        let sql = format!(
            "SELECT * FROM {table_name} WHERE {} ORDER BY event_time ASC",
            conditions.join(" AND ")
        );

        let df = ctx
            .sql(&sql)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;
        let batches = df
            .collect()
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(batches_to_event_results(&batches))
    }

    /// Execute arbitrary SQL via DataFusion over a unified hot+cold events table.
    async fn execute_datafusion_sql(
        &self,
        org_id: &OrgId,
        sql: &str,
    ) -> Result<Vec<EventResult>, StoreError> {
        let ctx = SessionContext::new();
        let schema = std::sync::Arc::new(super::parquet_io::event_arrow_schema());
        let mut tables_registered = false;

        // Register cold Parquet data.
        let has_cold = self.parquet_dir.exists()
            && !collect_parquet_files(&self.parquet_dir).is_empty();

        if has_cold {
            self.register_all_parquet(&ctx, "cold_events").await?;
            tables_registered = true;
        }

        // Fetch hot events from Postgres, convert to Arrow, register as MemTable.
        let hot_events = self.fetch_hot_events_for_org(org_id).await?;
        let has_hot = !hot_events.is_empty();

        if has_hot {
            let batch = super::parquet_io::events_to_record_batch(&hot_events)?;
            let mem_table = datafusion::datasource::MemTable::try_new(
                batch.schema(),
                vec![vec![batch]],
            )
            .map_err(|e| StoreError::Internal(e.to_string()))?;
            ctx.register_table("hot_events", std::sync::Arc::new(mem_table))
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            tables_registered = true;
        }

        if !tables_registered {
            let empty = datafusion::arrow::array::RecordBatch::new_empty(schema);
            let mem_table = datafusion::datasource::MemTable::try_new(
                empty.schema(),
                vec![vec![empty]],
            )
            .map_err(|e| StoreError::Internal(e.to_string()))?;
            ctx.register_table("events", std::sync::Arc::new(mem_table))
                .map_err(|e| StoreError::Internal(e.to_string()))?;
        } else {
            // Create a unified "events" view via UNION ALL.
            let view_sql = match (has_hot, has_cold) {
                (true, true) => {
                    "CREATE VIEW events AS \
                     SELECT * FROM hot_events UNION ALL SELECT * FROM cold_events"
                }
                (true, false) => "CREATE VIEW events AS SELECT * FROM hot_events",
                (false, true) => "CREATE VIEW events AS SELECT * FROM cold_events",
                (false, false) => unreachable!(),
            };
            ctx.sql(view_sql)
                .await
                .map_err(|e| StoreError::Query(e.to_string()))?;
        }

        let df = ctx
            .sql(sql)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;
        let batches = df
            .collect()
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(batches_to_event_results(&batches))
    }

    /// Fetch all current (hot) events for an org from Postgres as domain objects.
    async fn fetch_hot_events_for_org(
        &self,
        org_id: &OrgId,
    ) -> Result<Vec<chronicle_core::event::Event>, StoreError> {
        use crate::postgres::events::row_to_event;

        let rows = sqlx::query(
            "SELECT event_id, org_id, source, topic, event_type, event_time, \
             ingestion_time, payload, media_type, media_ref, media_blob, \
             media_size_bytes, raw_body \
             FROM events WHERE org_id = $1 ORDER BY event_time ASC",
        )
        .bind(org_id.as_str())
        .fetch_all(&self.pg.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(row_to_event).collect())
    }

    /// Look up event_ids that match an entity filter by querying Postgres entity_refs.
    async fn pg_event_ids_for_entity(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        entity_id: &EntityId,
    ) -> Result<Vec<EventId>, StoreError> {
        use crate::traits::EntityRefStore;
        self.pg
            .get_events_for_entity(org_id, entity_type, entity_id)
            .await
    }

    /// Register all Parquet files under `parquet_dir` as a single DataFusion table.
    ///
    /// Uses a `ListingTable` that recursively discovers `.parquet` files.
    async fn register_all_parquet(
        &self,
        ctx: &SessionContext,
        table_name: &str,
    ) -> Result<(), StoreError> {
        let parquet_files = collect_parquet_files(&self.parquet_dir);

        if parquet_files.is_empty() {
            let schema = super::parquet_io::event_arrow_schema();
            let empty =
                datafusion::arrow::array::RecordBatch::new_empty(std::sync::Arc::new(schema));
            let mem_table =
                datafusion::datasource::MemTable::try_new(empty.schema(), vec![vec![empty]])
                    .map_err(|e| StoreError::Internal(e.to_string()))?;
            ctx.register_table(table_name, std::sync::Arc::new(mem_table))
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            return Ok(());
        }

        if parquet_files.len() == 1 {
            ctx.register_parquet(
                table_name,
                &parquet_files[0],
                ParquetReadOptions::default(),
            )
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;
            return Ok(());
        }

        // Multiple files: read each into a RecordBatch and combine into a MemTable.
        // This is simpler and more reliable than ListingTable for nested directories.
        let schema = std::sync::Arc::new(super::parquet_io::event_arrow_schema());
        let mut all_batches = Vec::new();

        for path in &parquet_files {
            let tmp_ctx = SessionContext::new();
            let tmp_name = "tmp_pq";
            tmp_ctx
                .register_parquet(tmp_name, path, ParquetReadOptions::default())
                .await
                .map_err(|e| StoreError::Query(e.to_string()))?;

            let df = tmp_ctx
                .sql(&format!("SELECT * FROM {tmp_name}"))
                .await
                .map_err(|e| StoreError::Query(e.to_string()))?;
            let batches = df
                .collect()
                .await
                .map_err(|e| StoreError::Query(e.to_string()))?;

            all_batches.extend(batches);
        }

        if all_batches.is_empty() {
            let empty = datafusion::arrow::array::RecordBatch::new_empty(schema);
            all_batches.push(empty);
        }

        let combined_schema = all_batches[0].schema();
        let mem_table =
            datafusion::datasource::MemTable::try_new(combined_schema, vec![all_batches])
                .map_err(|e| StoreError::Internal(e.to_string()))?;
        ctx.register_table(table_name, std::sync::Arc::new(mem_table))
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        Ok(())
    }
}

/// Collect all `.parquet` file paths under a directory recursively.
fn collect_parquet_files(dir: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_parquet_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "parquet") {
                if let Some(s) = path.to_str() {
                    files.push(s.to_string());
                }
            }
        }
    }
    files
}

/// Merge results from hot and cold sources, dedup, sort, and limit.
fn merge_results(
    hot: Vec<EventResult>,
    cold: Vec<EventResult>,
    order_by: &OrderBy,
    limit: usize,
) -> Result<Vec<EventResult>, StoreError> {
    let mut merged = hot;
    merged.extend(cold);

    // Dedup by event_id (events may exist in both hot and cold briefly).
    merged.sort_by(|a, b| a.event.event_id.cmp(&b.event.event_id));
    merged.dedup_by(|a, b| a.event.event_id == b.event.event_id);

    sort_results(&mut merged, order_by);
    merged.truncate(limit);
    Ok(merged)
}

fn sort_results(results: &mut [EventResult], order: &OrderBy) {
    match order {
        OrderBy::EventTimeAsc => results.sort_by_key(|r| r.event.event_time),
        OrderBy::EventTimeDesc => {
            results.sort_by(|a, b| b.event.event_time.cmp(&a.event.event_time));
        }
        OrderBy::IngestionTimeAsc => results.sort_by_key(|r| r.event.ingestion_time),
        OrderBy::IngestionTimeDesc => {
            results.sort_by(|a, b| b.event.ingestion_time.cmp(&a.event.ingestion_time));
        }
    }
}
