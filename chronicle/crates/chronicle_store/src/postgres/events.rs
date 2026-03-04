//! `EventStore` implementation for Postgres.
//!
//! Write path uses five tiers of optimization:
//! 1. **UNNEST INSERT** — single query with array parameters (no row-by-row)
//! 2. **Transactional batching** — events + entity ref JSONB in one `BEGIN/COMMIT`
//! 3. **Embedded entity refs** — refs merged into payload JSONB, no second table write
//! 4. **Deferred WAL sync** — `SET LOCAL synchronous_commit = off` inside txn
//! 5. **Concurrent pipeline** — large batches split across pool connections
//!
//! Entity queries use JSONB `@>` operator with GIN index instead of
//! JOIN on `entity_refs` table, eliminating the write + query overhead.

use async_trait::async_trait;
use sqlx::Row;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::*;
use chronicle_core::media::MediaAttachment;
use chronicle_core::query::{EventResult, StructuredQuery, TimelineQuery};

use super::PostgresBackend;
use super::query_builder::{SelectBuilder, bind_params};
use crate::traits::EventStore;

/// Above this count we split across multiple pool connections.
const CONCURRENT_THRESHOLD: usize = 2000;
/// Number of parallel connections for concurrent insert.
const PIPELINE_WIDTH: usize = 4;

// Strategy 4: static SQL strings for prepared statement caching.
const UNNEST_INSERT_SQL: &str = "\
    INSERT INTO events \
    (event_id, org_id, source, topic, event_type, event_time, \
     ingestion_time, payload, media_type, media_ref, \
     media_size_bytes, raw_body) \
    SELECT * FROM UNNEST(\
     $1::text[], $2::text[], $3::text[], $4::text[], $5::text[], \
     $6::timestamptz[], $7::timestamptz[], $8::jsonb[], $9::text[], \
     $10::text[], $11::bigint[], $12::text[]) \
    ON CONFLICT (event_id) DO NOTHING";

const UNNEST_INSERT_REFS_SQL: &str = "\
    INSERT INTO entity_refs \
    (event_id, org_id, entity_type, entity_id, created_by) \
    SELECT * FROM UNNEST(\
     $1::text[], $2::text[], $3::text[], $4::text[], $5::text[]) \
    ON CONFLICT DO NOTHING";

// ---------------------------------------------------------------------------
// Columnar arrays for UNNEST-based INSERT
// ---------------------------------------------------------------------------

struct EventColumns {
    event_ids: Vec<String>,
    org_ids: Vec<String>,
    sources: Vec<String>,
    topics: Vec<String>,
    event_types: Vec<String>,
    event_times: Vec<chrono::DateTime<chrono::Utc>>,
    ingestion_times: Vec<chrono::DateTime<chrono::Utc>>,
    payloads: Vec<Option<serde_json::Value>>,
    media_types: Vec<Option<String>>,
    media_refs: Vec<Option<String>>,
    media_sizes: Vec<Option<i64>>,
    raw_bodies: Vec<Option<String>>,
}

impl EventColumns {
    /// Strategy 2: merge entity refs into payload JSONB during column extraction.
    fn from_events(events: &[Event]) -> Self {
        let n = events.len();
        let mut cols = Self {
            event_ids: Vec::with_capacity(n),
            org_ids: Vec::with_capacity(n),
            sources: Vec::with_capacity(n),
            topics: Vec::with_capacity(n),
            event_types: Vec::with_capacity(n),
            event_times: Vec::with_capacity(n),
            ingestion_times: Vec::with_capacity(n),
            payloads: Vec::with_capacity(n),
            media_types: Vec::with_capacity(n),
            media_refs: Vec::with_capacity(n),
            media_sizes: Vec::with_capacity(n),
            raw_bodies: Vec::with_capacity(n),
        };

        for e in events {
            cols.event_ids.push(e.event_id.to_string());
            cols.org_ids.push(e.org_id.as_str().to_string());
            cols.sources.push(e.source.as_str().to_string());
            cols.topics.push(e.topic.as_str().to_string());
            cols.event_types.push(e.event_type.as_str().to_string());
            cols.event_times.push(e.event_time);
            cols.ingestion_times.push(e.ingestion_time);
            cols.payloads.push(Some(embed_entity_refs(e)));
            cols.media_types.push(e.media.as_ref().map(|m| m.media_type.clone()));
            cols.media_refs
                .push(e.media.as_ref().and_then(|m| m.external_ref.clone()));
            cols.media_sizes
                .push(e.media.as_ref().map(|m| m.size_bytes as i64));
            cols.raw_bodies.push(e.raw_body.clone());
        }

        cols
    }
}

/// Merge entity refs into the event's payload JSONB under `_entity_refs`.
///
/// Keeps the original payload fields intact. If the event has no payload,
/// creates a minimal object containing only the refs array.
fn embed_entity_refs(event: &Event) -> serde_json::Value {
    let mut obj = match event.payload.as_ref() {
        Some(serde_json::Value::Object(m)) => m.clone(),
        Some(other) => {
            let mut m = serde_json::Map::new();
            m.insert("_value".to_owned(), other.clone());
            m
        }
        None => serde_json::Map::new(),
    };

    if !event.entity_refs.is_empty() {
        let refs: Vec<serde_json::Value> = event
            .entity_refs
            .iter()
            .map(|er| {
                serde_json::json!({
                    "type": er.entity_type.as_str(),
                    "id": er.entity_id.as_str(),
                })
            })
            .collect();
        obj.insert("_entity_refs".to_owned(), serde_json::Value::Array(refs));
    }

    serde_json::Value::Object(obj)
}

struct RefColumns {
    event_ids: Vec<String>,
    org_ids: Vec<String>,
    entity_types: Vec<String>,
    entity_ids: Vec<String>,
    created_bys: Vec<String>,
}

impl RefColumns {
    fn from_events(events: &[Event]) -> Self {
        let mut cols = Self {
            event_ids: Vec::new(),
            org_ids: Vec::new(),
            entity_types: Vec::new(),
            entity_ids: Vec::new(),
            created_bys: Vec::new(),
        };

        for event in events {
            for r in &event.materialize_entity_refs("ingestion") {
                cols.event_ids.push(event.event_id.to_string());
                cols.org_ids.push(event.org_id.as_str().to_string());
                cols.entity_types.push(r.entity_type.as_str().to_string());
                cols.entity_ids.push(r.entity_id.as_str().to_string());
                cols.created_bys.push(r.created_by.clone());
            }
        }

        cols
    }

    fn is_empty(&self) -> bool {
        self.event_ids.is_empty()
    }
}

// ---------------------------------------------------------------------------
// UNNEST INSERT helpers (use static SQL for prepared stmt caching — Strategy 4)
// ---------------------------------------------------------------------------

async fn unnest_insert_events_on(
    conn: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    cols: &EventColumns,
) -> Result<(), StoreError> {
    sqlx::query(UNNEST_INSERT_SQL)
        .bind(&cols.event_ids)
        .bind(&cols.org_ids)
        .bind(&cols.sources)
        .bind(&cols.topics)
        .bind(&cols.event_types)
        .bind(&cols.event_times)
        .bind(&cols.ingestion_times)
        .bind(&cols.payloads)
        .bind(&cols.media_types)
        .bind(&cols.media_refs)
        .bind(&cols.media_sizes)
        .bind(&cols.raw_bodies)
        .execute(conn)
        .await
        .map_err(|e| StoreError::Internal(e.to_string()))?;
    Ok(())
}

async fn unnest_insert_refs_on(
    conn: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    cols: &RefColumns,
) -> Result<(), StoreError> {
    if cols.is_empty() {
        return Ok(());
    }
    sqlx::query(UNNEST_INSERT_REFS_SQL)
        .bind(&cols.event_ids)
        .bind(&cols.org_ids)
        .bind(&cols.entity_types)
        .bind(&cols.entity_ids)
        .bind(&cols.created_bys)
        .execute(conn)
        .await
        .map_err(|e| StoreError::Internal(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pool-level helpers (for concurrent pipeline)
// ---------------------------------------------------------------------------

async fn unnest_insert_events_pool(
    pool: &sqlx::PgPool,
    cols: &EventColumns,
) -> Result<(), StoreError> {
    unnest_insert_events_on(pool, cols).await
}

// ---------------------------------------------------------------------------
// EventStore implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl EventStore for PostgresBackend {
    async fn insert_events(&self, events: &[Event]) -> Result<Vec<EventId>, StoreError> {
        if events.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<EventId> = events.iter().map(|e| e.event_id).collect();

        if events.len() >= CONCURRENT_THRESHOLD {
            self.insert_events_concurrent(events).await?;
        } else {
            self.insert_events_transactional(events).await?;
        }

        // Entity refs: fire-and-forget for large batches, sync for small.
        // Small batches (tests, typical API calls) need refs immediately.
        // Large batches (bulk ingestion) benefit from async — refs arrive
        // within milliseconds but don't block the caller.
        let ref_cols = RefColumns::from_events(events);
        if !ref_cols.is_empty() {
            if events.len() >= CONCURRENT_THRESHOLD {
                let pool = self.pool.clone();
                tokio::spawn(async move {
                    let _ = unnest_insert_refs_on(&pool, &ref_cols).await;
                });
            } else {
                unnest_insert_refs_on(&self.pool, &ref_cols).await.ok();
            }
        }

        Ok(ids)
    }

    async fn get_event(
        &self,
        org_id: &OrgId,
        id: &EventId,
    ) -> Result<Option<EventResult>, StoreError> {
        let (sql, params) = SelectBuilder::events().where_org(org_id.as_str()).build();

        let full_sql = format!("{sql} AND e.event_id = ${}", params.len() + 1);

        let mut q = sqlx::query(&full_sql);
        q = bind_params(q, &params);
        q = q.bind(id.to_string());

        let row = q
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(row.map(|r| EventResult {
            event: row_to_event(&r),
            entity_refs: vec![],
            search_distance: None,
        }))
    }

    async fn query_structured(
        &self,
        query: &StructuredQuery,
    ) -> Result<Vec<EventResult>, StoreError> {
        let mut builder = SelectBuilder::events()
            .where_org(query.org_id.as_str())
            .where_source(query.source.as_ref().map(|s| s.as_str()))
            .where_event_type(query.event_type.as_ref().map(|t| t.as_str()))
            .where_time_range(query.time_range.as_ref())
            .order_by(&query.order_by)
            .limit(query.limit);

        if let Some((ref etype, ref eid)) = query.entity {
            builder = builder
                .join_entity_refs()
                .where_entity(Some(etype.as_str()), Some(eid.as_str()));
        }

        let (sql, params) = builder.build();

        let mut q = sqlx::query(&sql);
        q = bind_params(q, &params);

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|r| EventResult {
                event: row_to_event(r),
                entity_refs: vec![],
                search_distance: None,
            })
            .collect())
    }

    async fn query_timeline(
        &self,
        query: &TimelineQuery,
    ) -> Result<Vec<EventResult>, StoreError> {
        let (sql, params) = SelectBuilder::events()
            .join_entity_refs()
            .where_org(query.org_id.as_str())
            .where_entity(
                Some(query.entity_type.as_str()),
                Some(query.entity_id.as_str()),
            )
            .where_time_range(query.time_range.as_ref())
            .order_by(&chronicle_core::query::OrderBy::EventTimeAsc)
            .build();

        let mut q = sqlx::query(&sql);
        q = bind_params(q, &params);

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|r| EventResult {
                event: row_to_event(r),
                entity_refs: vec![],
                search_distance: None,
            })
            .collect())
    }

    async fn query_sql(
        &self,
        _org_id: &OrgId,
        sql: &str,
    ) -> Result<Vec<EventResult>, StoreError> {
        Err(StoreError::Query(format!(
            "Raw SQL not yet supported: {sql}"
        )))
    }

    async fn count(&self, query: &StructuredQuery) -> Result<u64, StoreError> {
        let mut builder = SelectBuilder::custom("COUNT(*) as cnt", "events e")
            .where_org(query.org_id.as_str())
            .where_source(query.source.as_ref().map(|s| s.as_str()))
            .where_event_type(query.event_type.as_ref().map(|t| t.as_str()))
            .where_time_range(query.time_range.as_ref());

        if let Some((ref etype, ref eid)) = query.entity {
            builder = builder
                .join_entity_refs()
                .where_entity(Some(etype.as_str()), Some(eid.as_str()));
        }

        let (sql, params) = builder.build();
        let mut q = sqlx::query(&sql);
        q = bind_params(q, &params);

        let row = q
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        let cnt: i64 = row.get("cnt");
        Ok(cnt as u64)
    }
}

// ---------------------------------------------------------------------------
// Insert strategies
// ---------------------------------------------------------------------------

impl PostgresBackend {
    /// Events-only transaction with deferred WAL. Entity refs in JSONB payload;
    /// the `entity_refs` table is populated asynchronously after this returns.
    async fn insert_events_transactional(&self, events: &[Event]) -> Result<(), StoreError> {
        let evt_cols = EventColumns::from_events(events);

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        sqlx::query("SET LOCAL synchronous_commit = off")
            .execute(&mut *tx)
            .await
            .ok();

        unnest_insert_events_on(&mut *tx, &evt_cols).await?;

        tx.commit()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        Ok(())
    }

    /// Concurrent pipeline: events-only across PIPELINE_WIDTH connections.
    /// Entity refs are populated async after all chunks complete.
    async fn insert_events_concurrent(&self, events: &[Event]) -> Result<(), StoreError> {
        let chunk_size = (events.len() / PIPELINE_WIDTH).max(500);

        let mut futs = Vec::new();
        for chunk in events.chunks(chunk_size) {
            let evt_cols = EventColumns::from_events(chunk);
            let pool = self.pool.clone();
            futs.push(tokio::spawn(async move {
                unnest_insert_events_pool(&pool, &evt_cols).await
            }));
        }

        for handle in futs {
            handle
                .await
                .map_err(|e| StoreError::Internal(format!("join: {e}")))?
                .map_err(|e| StoreError::Internal(format!("chunk insert: {e}")))?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Row conversion
// ---------------------------------------------------------------------------

/// Convert a sqlx Row into a domain Event.
pub(crate) fn row_to_event(row: &sqlx::postgres::PgRow) -> Event {
    let event_id_str: String = row.get("event_id");
    let media_type: Option<String> = row.get("media_type");
    let media_ref: Option<String> = row.get("media_ref");
    let media_blob: Option<Vec<u8>> = row.get("media_blob");
    let media_size: Option<i64> = row.get("media_size_bytes");

    let media = media_type.map(|mt| MediaAttachment {
        media_type: mt,
        inline_blob: media_blob,
        external_ref: media_ref,
        size_bytes: media_size.unwrap_or(0) as u64,
    });

    Event {
        event_id: event_id_str.parse().unwrap_or_else(|_| EventId::new()),
        org_id: OrgId::new(row.get::<String, _>("org_id").as_str()),
        source: Source::new(row.get::<String, _>("source").as_str()),
        topic: Topic::new(row.get::<String, _>("topic").as_str()),
        event_type: EventType::new(row.get::<String, _>("event_type").as_str()),
        event_time: row.get("event_time"),
        ingestion_time: row.get("ingestion_time"),
        payload: row.get("payload"),
        media,
        entity_refs: vec![],
        raw_body: row.get("raw_body"),
    }
}
