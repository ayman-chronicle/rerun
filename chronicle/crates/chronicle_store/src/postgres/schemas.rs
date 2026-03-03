//! `SchemaRegistry` implementation for Postgres.

use async_trait::async_trait;
use sqlx::Row;

use chronicle_core::error::StoreError;
use chronicle_core::ids::*;

use crate::traits::{SchemaRegistry, SourceInfo, SourceSchema};
use super::PostgresBackend;

#[async_trait]
impl SchemaRegistry for PostgresBackend {
    async fn register_schema(&self, schema: &SourceSchema) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO source_schemas (org_id, source, event_type, version, field_names, field_types, sample_event)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (org_id, source, event_type, version) DO UPDATE
             SET field_names = $5, field_types = $6, sample_event = $7"
        )
        .bind(schema.org_id.as_str())
        .bind(schema.source.as_str())
        .bind(schema.event_type.as_str())
        .bind(schema.version as i32)
        .bind(&schema.field_names)
        .bind(&schema.field_types)
        .bind(&schema.sample_event)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Internal(e.to_string()))?;

        Ok(())
    }

    async fn get_schema(
        &self,
        org_id: &OrgId,
        source: &Source,
        event_type: &EventType,
    ) -> Result<Option<SourceSchema>, StoreError> {
        let row = sqlx::query(
            "SELECT org_id, source, event_type, version, field_names, field_types, sample_event
             FROM source_schemas
             WHERE org_id = $1 AND source = $2 AND event_type = $3
             ORDER BY version DESC LIMIT 1"
        )
        .bind(org_id.as_str())
        .bind(source.as_str())
        .bind(event_type.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(row.map(|r| SourceSchema {
            org_id: OrgId::new(r.get::<String, _>("org_id").as_str()),
            source: Source::new(r.get::<String, _>("source").as_str()),
            event_type: EventType::new(r.get::<String, _>("event_type").as_str()),
            version: r.get::<i32, _>("version") as u32,
            field_names: r.get("field_names"),
            field_types: r.get("field_types"),
            sample_event: r.get("sample_event"),
        }))
    }

    async fn describe_sources(&self, org_id: &OrgId) -> Result<Vec<SourceInfo>, StoreError> {
        let rows = sqlx::query(
            "SELECT s.source,
                    ARRAY_AGG(DISTINCT s.event_type) as event_types,
                    COALESCE(e.event_count, 0) as event_count
             FROM source_schemas s
             LEFT JOIN (
                 SELECT source, COUNT(*) as event_count
                 FROM events WHERE org_id = $1
                 GROUP BY source
             ) e ON s.source = e.source
             WHERE s.org_id = $1
             GROUP BY s.source, e.event_count"
        )
        .bind(org_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| {
            let event_types: Vec<String> = r.get("event_types");
            SourceInfo {
                source: Source::new(r.get::<String, _>("source").as_str()),
                event_types: event_types.iter().map(|t| EventType::new(t.as_str())).collect(),
                event_count: r.get::<i64, _>("event_count") as u64,
                first_seen: None,
                last_seen: None,
            }
        }).collect())
    }
}
