//! `EntityRefStore` implementation for Postgres.

use async_trait::async_trait;
use sqlx::Row;

use chronicle_core::entity_ref::EntityRef;
use chronicle_core::error::StoreError;
use chronicle_core::ids::*;

use crate::traits::{EntityInfo, EntityRefStore, EntityTypeInfo};
use super::PostgresBackend;

#[async_trait]
impl EntityRefStore for PostgresBackend {
    async fn add_refs(&self, refs: &[EntityRef]) -> Result<(), StoreError> {
        for r in refs {
            sqlx::query(
                "INSERT INTO entity_refs (event_id, org_id, entity_type, entity_id, created_by)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT DO NOTHING"
            )
            .bind(r.event_id.to_string())
            .bind("") // org_id filled from event at query time
            .bind(r.entity_type.as_str())
            .bind(r.entity_id.as_str())
            .bind(&r.created_by)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    async fn get_refs_for_event(&self, event_id: &EventId) -> Result<Vec<EntityRef>, StoreError> {
        let rows = sqlx::query(
            "SELECT event_id, entity_type, entity_id, created_by, created_at
             FROM entity_refs WHERE event_id = $1"
        )
        .bind(event_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| EntityRef {
            event_id: r.get::<String, _>("event_id").parse().unwrap_or_else(|_| EventId::new()),
            entity_type: EntityType::new(r.get::<String, _>("entity_type").as_str()),
            entity_id: EntityId::new(r.get::<String, _>("entity_id")),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
        }).collect())
    }

    async fn get_events_for_entity(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        entity_id: &EntityId,
    ) -> Result<Vec<EventId>, StoreError> {
        let rows = sqlx::query(
            "SELECT event_id FROM entity_refs
             WHERE org_id = $1 AND entity_type = $2 AND entity_id = $3"
        )
        .bind(org_id.as_str())
        .bind(entity_type.as_str())
        .bind(entity_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().filter_map(|r| {
            r.get::<String, _>("event_id").parse().ok()
        }).collect())
    }

    async fn link_entity(
        &self,
        org_id: &OrgId,
        from_type: &EntityType,
        from_id: &EntityId,
        to_type: &EntityType,
        to_id: &EntityId,
        created_by: &str,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            "INSERT INTO entity_refs (event_id, org_id, entity_type, entity_id, created_by)
             SELECT event_id, org_id, $4, $5, $6
             FROM entity_refs
             WHERE org_id = $1 AND entity_type = $2 AND entity_id = $3
             ON CONFLICT DO NOTHING"
        )
        .bind(org_id.as_str())
        .bind(from_type.as_str())
        .bind(from_id.as_str())
        .bind(to_type.as_str())
        .bind(to_id.as_str())
        .bind(created_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Internal(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn list_entity_types(&self, org_id: &OrgId) -> Result<Vec<EntityTypeInfo>, StoreError> {
        let rows = sqlx::query(
            "SELECT entity_type, COUNT(DISTINCT entity_id) as entity_count
             FROM entity_refs WHERE org_id = $1
             GROUP BY entity_type ORDER BY entity_count DESC"
        )
        .bind(org_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| EntityTypeInfo {
            entity_type: EntityType::new(r.get::<String, _>("entity_type").as_str()),
            entity_count: r.get::<i64, _>("entity_count") as u64,
            first_seen: None,
            last_seen: None,
        }).collect())
    }

    async fn list_entities(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        limit: usize,
    ) -> Result<Vec<EntityInfo>, StoreError> {
        let rows = sqlx::query(
            "SELECT entity_id, COUNT(*) as event_count
             FROM entity_refs
             WHERE org_id = $1 AND entity_type = $2
             GROUP BY entity_id
             ORDER BY event_count DESC
             LIMIT $3"
        )
        .bind(org_id.as_str())
        .bind(entity_type.as_str())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Query(e.to_string()))?;

        Ok(rows.iter().map(|r| EntityInfo {
            entity_type: *entity_type,
            entity_id: EntityId::new(r.get::<String, _>("entity_id")),
            event_count: r.get::<i64, _>("event_count") as u64,
            first_seen: None,
            last_seen: None,
        }).collect())
    }
}
