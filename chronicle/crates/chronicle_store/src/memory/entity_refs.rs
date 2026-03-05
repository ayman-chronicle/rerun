//! `EntityRefStore` implementation for the in-memory backend.

use async_trait::async_trait;

use chronicle_core::entity_ref::EntityRef;
use chronicle_core::error::StoreError;
use chronicle_core::ids::{EntityId, EntityType, EventId, OrgId};

use crate::traits::{EntityInfo, EntityRefStore, EntityTypeInfo};
use super::state::InMemoryBackend;

#[async_trait]
impl EntityRefStore for InMemoryBackend {
    async fn add_refs(&self, org_id: &OrgId, refs: &[EntityRef]) -> Result<(), StoreError> {
        let mut store = self.entity_refs.write();
        for r in refs {
            let already_exists = store.iter().any(|(o, existing)| {
                *o == *org_id
                    && existing.event_id == r.event_id
                    && existing.entity_type == r.entity_type
                    && existing.entity_id == r.entity_id
            });
            if !already_exists {
                store.push((*org_id, r.clone()));
            }
        }
        Ok(())
    }

    async fn get_refs_for_event(&self, org_id: &OrgId, event_id: &EventId) -> Result<Vec<EntityRef>, StoreError> {
        let store = self.entity_refs.read();
        Ok(store
            .iter()
            .filter(|(o, r)| *o == *org_id && r.event_id == *event_id)
            .map(|(_, r)| r.clone())
            .collect())
    }

    async fn get_events_for_entity(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        entity_id: &EntityId,
    ) -> Result<Vec<EventId>, StoreError> {
        let store = self.entity_refs.read();
        let ids: Vec<EventId> = store
            .iter()
            .filter(|(o, r)| *o == *org_id && r.entity_type == *entity_type && r.entity_id == *entity_id)
            .map(|(_, r)| r.event_id)
            .collect();
        Ok(ids)
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
        let event_ids: Vec<EventId> = {
            let store = self.entity_refs.read();
            store
                .iter()
                .filter(|(o, r)| *o == *org_id && r.entity_type == *from_type && r.entity_id == *from_id)
                .map(|(_, r)| r.event_id)
                .collect()
        };

        let new_refs: Vec<EntityRef> = event_ids
            .iter()
            .map(|eid| EntityRef::new(*eid, *to_type, to_id.clone(), created_by))
            .collect();

        let count = new_refs.len() as u64;
        self.add_refs(org_id, &new_refs).await?;
        Ok(count)
    }

    async fn list_entity_types(&self, org_id: &OrgId) -> Result<Vec<EntityTypeInfo>, StoreError> {
        let store = self.entity_refs.read();
        let mut type_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

        for (o, r) in store.iter() {
            if *o == *org_id {
                *type_counts.entry(r.entity_type.as_str().to_string()).or_default() += 1;
            }
        }

        Ok(type_counts
            .into_iter()
            .map(|(t, count)| EntityTypeInfo {
                entity_type: EntityType::new(&t),
                entity_count: count,
                first_seen: None,
                last_seen: None,
            })
            .collect())
    }

    async fn list_entities(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        limit: usize,
    ) -> Result<Vec<EntityInfo>, StoreError> {
        let store = self.entity_refs.read();
        let mut entity_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

        for (o, r) in store.iter() {
            if *o == *org_id && r.entity_type == *entity_type {
                *entity_counts.entry(r.entity_id.as_str().to_string()).or_default() += 1;
            }
        }

        let mut entities: Vec<EntityInfo> = entity_counts
            .into_iter()
            .map(|(id, count)| EntityInfo {
                entity_type: *entity_type,
                entity_id: EntityId::new(id),
                event_count: count,
                first_seen: None,
                last_seen: None,
            })
            .collect();

        entities.sort_by(|a, b| b.event_count.cmp(&a.event_count));
        entities.truncate(limit);
        Ok(entities)
    }
}
