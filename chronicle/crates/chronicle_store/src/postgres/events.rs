//! `EventStore` implementation for Postgres.

use async_trait::async_trait;
use sqlx::Row;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::*;
use chronicle_core::media::MediaAttachment;
use chronicle_core::query::{EventResult, StructuredQuery, TimelineQuery};

use crate::traits::EventStore;
use super::PostgresBackend;
use super::query_builder::{SelectBuilder, bind_params};

#[async_trait]
impl EventStore for PostgresBackend {
    async fn insert_events(&self, events: &[Event]) -> Result<Vec<EventId>, StoreError> {
        let mut ids = Vec::with_capacity(events.len());

        for event in events {
            let eid = event.event_id.to_string();
            let media_type = event.media.as_ref().map(|m| m.media_type.clone());
            let media_ref = event.media.as_ref().and_then(|m| m.external_ref.clone());
            let media_blob = event.media.as_ref().and_then(|m| m.inline_blob.clone());
            let media_size = event.media.as_ref().map(|m| m.size_bytes as i64);

            sqlx::query(
                "INSERT INTO events \
                 (event_id, org_id, source, topic, event_type, event_time, \
                  ingestion_time, payload, media_type, media_ref, media_blob, \
                  media_size_bytes, raw_body) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) \
                 ON CONFLICT (event_id) DO NOTHING"
            )
            .bind(&eid)
            .bind(event.org_id.as_str())
            .bind(event.source.as_str())
            .bind(event.topic.as_str())
            .bind(event.event_type.as_str())
            .bind(event.event_time)
            .bind(event.ingestion_time)
            .bind(&event.payload)
            .bind(&media_type)
            .bind(&media_ref)
            .bind(&media_blob)
            .bind(&media_size)
            .bind(event.raw_body.as_deref())
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

            for r in &event.materialize_entity_refs("ingestion") {
                sqlx::query(
                    "INSERT INTO entity_refs (event_id, org_id, entity_type, entity_id, created_by) \
                     VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING"
                )
                .bind(&eid)
                .bind(event.org_id.as_str())
                .bind(r.entity_type.as_str())
                .bind(r.entity_id.as_str())
                .bind(&r.created_by)
                .execute(&self.pool)
                .await
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            }

            ids.push(event.event_id);
        }

        Ok(ids)
    }

    async fn get_event(&self, org_id: &OrgId, id: &EventId) -> Result<Option<EventResult>, StoreError> {
        let (sql, params) = SelectBuilder::events()
            .where_org(org_id.as_str())
            .build();

        // For get_event, add the event_id filter directly
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

    async fn count(&self, query: &StructuredQuery) -> Result<u64, StoreError> {
        let results = self.query_structured(query).await?;
        Ok(results.len() as u64)
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
