//! `EventStore` implementation for Postgres.
//!
//! Uses multi-row INSERT with transactions for high write throughput.

use async_trait::async_trait;
use sqlx::{QueryBuilder, Row};

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::*;
use chronicle_core::media::MediaAttachment;
use chronicle_core::query::{EventResult, StructuredQuery, TimelineQuery};

use crate::traits::EventStore;
use super::PostgresBackend;
use super::query_builder::{SelectBuilder, bind_params};

/// Pre-computed row values for batch INSERT (avoids lifetime issues with QueryBuilder).
struct EventRow {
    event_id: String,
    org_id: String,
    source: String,
    topic: String,
    event_type: String,
    event_time: chrono::DateTime<chrono::Utc>,
    ingestion_time: chrono::DateTime<chrono::Utc>,
    payload: Option<serde_json::Value>,
    media_type: Option<String>,
    media_ref: Option<String>,
    media_blob: Option<Vec<u8>>,
    media_size: Option<i64>,
    raw_body: Option<String>,
}

impl EventRow {
    fn from_event(event: &Event) -> Self {
        Self {
            event_id: event.event_id.to_string(),
            org_id: event.org_id.as_str().to_string(),
            source: event.source.as_str().to_string(),
            topic: event.topic.as_str().to_string(),
            event_type: event.event_type.as_str().to_string(),
            event_time: event.event_time,
            ingestion_time: event.ingestion_time,
            payload: event.payload.clone(),
            media_type: event.media.as_ref().map(|m| m.media_type.clone()),
            media_ref: event.media.as_ref().and_then(|m| m.external_ref.clone()),
            media_blob: event.media.as_ref().and_then(|m| m.inline_blob.clone()),
            media_size: event.media.as_ref().map(|m| m.size_bytes as i64),
            raw_body: event.raw_body.clone(),
        }
    }
}

struct RefRow {
    event_id: String,
    org_id: String,
    entity_type: String,
    entity_id: String,
    created_by: String,
}

/// Max events per multi-row INSERT (13 columns × 500 = 6500 params, under 65535 limit).
const EVENT_BATCH_SIZE: usize = 500;
/// Max entity refs per multi-row INSERT (5 columns × 2000 = 10000 params).
const REF_BATCH_SIZE: usize = 2000;

#[async_trait]
impl EventStore for PostgresBackend {
    async fn insert_events(&self, events: &[Event]) -> Result<Vec<EventId>, StoreError> {
        if events.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<EventId> = events.iter().map(|e| e.event_id).collect();

        // Pre-compute row values.
        let event_rows: Vec<EventRow> = events.iter().map(EventRow::from_event).collect();
        let ref_rows: Vec<RefRow> = events
            .iter()
            .flat_map(|event| {
                event.materialize_entity_refs("ingestion").into_iter().map(|r| RefRow {
                    event_id: event.event_id.to_string(),
                    org_id: event.org_id.as_str().to_string(),
                    entity_type: r.entity_type.as_str().to_string(),
                    entity_id: r.entity_id.as_str().to_string(),
                    created_by: r.created_by.clone(),
                })
            })
            .collect();

        // Single transaction for the entire batch.
        let mut tx = self.pool.begin().await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        // Batch INSERT events.
        for chunk in event_rows.chunks(EVENT_BATCH_SIZE) {
            let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
                "INSERT INTO events \
                 (event_id, org_id, source, topic, event_type, event_time, \
                  ingestion_time, payload, media_type, media_ref, media_blob, \
                  media_size_bytes, raw_body) ",
            );

            qb.push_values(chunk, |mut b, row| {
                b.push_bind(&row.event_id)
                    .push_bind(&row.org_id)
                    .push_bind(&row.source)
                    .push_bind(&row.topic)
                    .push_bind(&row.event_type)
                    .push_bind(row.event_time)
                    .push_bind(row.ingestion_time)
                    .push_bind(&row.payload)
                    .push_bind(&row.media_type)
                    .push_bind(&row.media_ref)
                    .push_bind(&row.media_blob)
                    .push_bind(&row.media_size)
                    .push_bind(&row.raw_body);
            });
            qb.push(" ON CONFLICT (event_id) DO NOTHING");

            qb.build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StoreError::Internal(e.to_string()))?;
        }

        // Batch INSERT entity refs.
        for chunk in ref_rows.chunks(REF_BATCH_SIZE) {
            let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
                "INSERT INTO entity_refs \
                 (event_id, org_id, entity_type, entity_id, created_by) ",
            );

            qb.push_values(chunk, |mut b, row| {
                b.push_bind(&row.event_id)
                    .push_bind(&row.org_id)
                    .push_bind(&row.entity_type)
                    .push_bind(&row.entity_id)
                    .push_bind(&row.created_by);
            });
            qb.push(" ON CONFLICT DO NOTHING");

            qb.build()
                .execute(&mut *tx)
                .await
                .map_err(|e| StoreError::Internal(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        Ok(ids)
    }

    async fn get_event(&self, org_id: &OrgId, id: &EventId) -> Result<Option<EventResult>, StoreError> {
        let (sql, params) = SelectBuilder::events()
            .where_org(org_id.as_str())
            .build();

        let full_sql = format!("{sql} AND e.event_id = ${}", params.len() + 1);

        let mut q = sqlx::query(&full_sql);
        q = bind_params(q, &params);
        q = q.bind(id.to_string());

        let row = q.fetch_optional(&self.pool).await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(row.map(|r| EventResult {
            event: row_to_event(&r),
            entity_refs: vec![],
            search_distance: None,
        }))
    }

    async fn query_structured(&self, query: &StructuredQuery) -> Result<Vec<EventResult>, StoreError> {
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

        let rows = q.fetch_all(&self.pool).await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| EventResult {
            event: row_to_event(r),
            entity_refs: vec![],
            search_distance: None,
        }).collect())
    }

    async fn query_timeline(&self, query: &TimelineQuery) -> Result<Vec<EventResult>, StoreError> {
        let (sql, params) = SelectBuilder::events()
            .join_entity_refs()
            .where_org(query.org_id.as_str())
            .where_entity(Some(query.entity_type.as_str()), Some(query.entity_id.as_str()))
            .where_time_range(query.time_range.as_ref())
            .order_by(&chronicle_core::query::OrderBy::EventTimeAsc)
            .build();

        let mut q = sqlx::query(&sql);
        q = bind_params(q, &params);

        let rows = q.fetch_all(&self.pool).await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| EventResult {
            event: row_to_event(r),
            entity_refs: vec![],
            search_distance: None,
        }).collect())
    }

    async fn query_sql(&self, _org_id: &OrgId, sql: &str) -> Result<Vec<EventResult>, StoreError> {
        Err(StoreError::Query(format!("Raw SQL not yet supported: {sql}")))
    }

    /// Proper COUNT(*) query instead of loading all results into memory.
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

        let row = q.fetch_one(&self.pool).await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        let cnt: i64 = row.get("cnt");
        Ok(cnt as u64)
    }
}

/// Convert a sqlx Row into a domain Event. Used by events.rs and links.rs.
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
