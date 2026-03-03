//! `EntityRefStore` for Kurrent -- delegates to Postgres sidecar.

use async_trait::async_trait;

use chronicle_core::entity_ref::EntityRef;
use chronicle_core::error::StoreError;
use chronicle_core::ids::{EntityId, EntityType, EventId, OrgId};

use crate::traits::{EntityInfo, EntityRefStore, EntityTypeInfo};
use super::KurrentBackend;

#[async_trait]
impl EntityRefStore for KurrentBackend {
    async fn add_refs(&self, refs: &[EntityRef]) -> Result<(), StoreError> {
        self.pg.add_refs(refs).await
    }

    async fn get_refs_for_event(&self, event_id: &EventId) -> Result<Vec<EntityRef>, StoreError> {
        self.pg.get_refs_for_event(event_id).await
    }

    async fn get_events_for_entity(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        entity_id: &EntityId,
    ) -> Result<Vec<EventId>, StoreError> {
        self.pg
            .get_events_for_entity(org_id, entity_type, entity_id)
            .await
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
        self.pg
            .link_entity(org_id, from_type, from_id, to_type, to_id, created_by)
            .await
    }

    async fn list_entity_types(&self, org_id: &OrgId) -> Result<Vec<EntityTypeInfo>, StoreError> {
        self.pg.list_entity_types(org_id).await
    }

    async fn list_entities(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        limit: usize,
    ) -> Result<Vec<EntityInfo>, StoreError> {
        self.pg.list_entities(org_id, entity_type, limit).await
    }
}
