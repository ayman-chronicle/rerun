//! `EventLinkStore` implementation for Postgres.

use async_trait::async_trait;
use sqlx::Row;

use chronicle_core::error::StoreError;
use chronicle_core::ids::*;
use chronicle_core::link::{EventLink, LinkDirection};
use chronicle_core::query::{EventResult, GraphQuery};

use crate::traits::EventLinkStore;
use super::PostgresBackend;

#[async_trait]
impl EventLinkStore for PostgresBackend {
    async fn create_link(&self, link: &EventLink) -> Result<LinkId, StoreError> {
        link.validate().map_err(|e| StoreError::Query(e.to_string()))?;

        sqlx::query(
            "INSERT INTO event_links (link_id, org_id, source_event_id, target_event_id, link_type, confidence, reasoning, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT DO NOTHING"
        )
        .bind(link.link_id.to_string())
        .bind("") // org_id derived from events
        .bind(link.source_event_id.to_string())
        .bind(link.target_event_id.to_string())
        .bind(&link.link_type)
        .bind(link.confidence.value())
        .bind(link.reasoning.as_deref())
        .bind(&link.created_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Internal(e.to_string()))?;

        Ok(link.link_id)
    }

    async fn get_links_for_event(&self, event_id: &EventId) -> Result<Vec<EventLink>, StoreError> {
        let rows = sqlx::query(
            "SELECT link_id, source_event_id, target_event_id, link_type, confidence, reasoning, created_by, created_at
             FROM event_links
             WHERE source_event_id = $1 OR target_event_id = $1"
        )
        .bind(event_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        rows.iter().map(row_to_link).collect()
    }

    async fn traverse(&self, query: &GraphQuery) -> Result<Vec<EventResult>, StoreError> {
        // Use recursive CTE for graph traversal.
        // Direction determines which side of the link to follow.
        let (link_condition, next_column) = match query.direction {
            LinkDirection::Outgoing => ("source_event_id = chain.event_id", "target_event_id"),
            LinkDirection::Incoming => ("target_event_id = chain.event_id", "source_event_id"),
            LinkDirection::Both => ("source_event_id = chain.event_id OR target_event_id = chain.event_id", "CASE WHEN source_event_id = chain.event_id THEN target_event_id ELSE source_event_id END"),
        };

        let sql = format!(
            "WITH RECURSIVE chain AS (
                SELECT $1::text AS event_id, 0 AS depth
                UNION ALL
                SELECT {next_column} AS event_id, chain.depth + 1
                FROM event_links el
                JOIN chain ON {link_condition}
                WHERE chain.depth < $2
                  AND el.confidence >= $3
            )
            SELECT DISTINCT e.event_id, e.org_id, e.source, e.topic, e.event_type,
                   e.event_time, e.ingestion_time, e.payload, e.media_type, e.media_ref,
                   e.media_blob, e.media_size_bytes, e.raw_body
            FROM chain
            JOIN events e ON chain.event_id = e.event_id
            WHERE e.org_id = $4
            ORDER BY e.event_time ASC"
        );

        let rows = sqlx::query(&sql)
            .bind(query.start_event_id.to_string())
            .bind(query.max_depth as i32)
            .bind(query.min_confidence)
            .bind(query.org_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| EventResult {
            event: super::events::row_to_event(r),
            entity_refs: vec![],
            search_distance: None,
        }).collect())
    }
}

fn row_to_link(row: &sqlx::postgres::PgRow) -> Result<EventLink, StoreError> {
    Ok(EventLink {
        link_id: row.get::<String, _>("link_id").parse().map_err(|_| StoreError::Internal("bad link_id".to_string()))?,
        source_event_id: row.get::<String, _>("source_event_id").parse().map_err(|_| StoreError::Internal("bad source_event_id".to_string()))?,
        target_event_id: row.get::<String, _>("target_event_id").parse().map_err(|_| StoreError::Internal("bad target_event_id".to_string()))?,
        link_type: row.get("link_type"),
        confidence: Confidence::new(row.get("confidence")).map_err(|e| StoreError::Internal(e.to_string()))?,
        reasoning: row.get("reasoning"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
    })
}
