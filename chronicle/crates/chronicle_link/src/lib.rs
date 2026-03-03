//! Entity extraction, JIT linking, and link management.
//!
//! This crate provides:
//!
//! - [`LinkService`]: Unified API for creating links and entity refs
//! - [`extract_entities`]: Sync entity extraction from event payloads
//!   using configurable patterns
//!
//! # JIT Entity Linking
//!
//! When an AI agent discovers that a session belongs to a customer,
//! it calls `link_service.link_entity(...)` which propagates the
//! entity ref to all events that reference the source entity.

pub mod extractor;

use std::sync::Arc;

use chronicle_core::entity_ref::EntityRef;
use chronicle_core::error::ChronicleError;
use chronicle_core::ids::{EntityId, EntityType, EventId, LinkId, OrgId};
use chronicle_core::link::EventLink;
use chronicle_core::query::{EventResult, GraphQuery};
use chronicle_store::StorageEngine;

/// Unified link and entity ref management service.
#[derive(Clone)]
pub struct LinkService {
    engine: StorageEngine,
}

impl LinkService {
    pub fn new(engine: StorageEngine) -> Self {
        Self { engine }
    }

    /// Create a causal/relational link between two events.
    ///
    /// Validates no self-links before persisting.
    pub async fn create_link(&self, link: &EventLink) -> Result<LinkId, ChronicleError> {
        Ok(self.engine.links.create_link(link).await?)
    }

    /// Add an entity ref to an existing event.
    ///
    /// This is the JIT linking primitive -- call it when an AI agent
    /// discovers that an event is associated with an entity that wasn't
    /// known at ingestion time.
    pub async fn add_entity_ref(
        &self,
        event_id: EventId,
        entity_type: impl Into<EntityType>,
        entity_id: impl Into<EntityId>,
        created_by: &str,
    ) -> Result<(), ChronicleError> {
        let entity_ref = EntityRef::new(event_id, entity_type, entity_id, created_by);
        Ok(self.engine.entity_refs.add_refs(&[entity_ref]).await?)
    }

    /// Propagate entity refs: for every event that references `(from_type, from_id)`,
    /// add a new ref to `(to_type, to_id)`.
    ///
    /// This is the high-level JIT linking operation. Example: link all
    /// events from session "sess_X" to customer "cust_NEW".
    pub async fn link_entity(
        &self,
        org_id: &OrgId,
        from_type: impl Into<EntityType>,
        from_id: impl Into<EntityId>,
        to_type: impl Into<EntityType>,
        to_id: impl Into<EntityId>,
        created_by: &str,
    ) -> Result<u64, ChronicleError> {
        let from_type = from_type.into();
        let from_id = from_id.into();
        let to_type = to_type.into();
        let to_id = to_id.into();

        let count = self
            .engine
            .entity_refs
            .link_entity(org_id, &from_type, &from_id, &to_type, &to_id, created_by)
            .await?;

        tracing::info!(
            %org_id, %from_type, %from_id, %to_type, %to_id,
            linked_count = count,
            "Entity linking complete"
        );

        Ok(count)
    }

    /// Get all links from/to an event.
    pub async fn get_links(&self, event_id: &EventId) -> Result<Vec<EventLink>, ChronicleError> {
        Ok(self.engine.links.get_links_for_event(event_id).await?)
    }

    /// Traverse the link graph from a starting event.
    pub async fn traverse(&self, query: &GraphQuery) -> Result<Vec<EventResult>, ChronicleError> {
        Ok(self.engine.links.traverse(query).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_core::ids::{Confidence, EventId, LinkId, OrgId};
    use chronicle_core::link::LinkDirection;
    use chronicle_store::memory::InMemoryBackend;
    use chronicle_store::traits::EventStore;
    use chronicle_test_fixtures::factories;
    use chrono::Utc;

    fn make_service() -> (LinkService, StorageEngine) {
        let backend = Arc::new(InMemoryBackend::new());
        let engine = StorageEngine {
            events: backend.clone(),
            entity_refs: backend.clone(),
            links: backend.clone(),
            embeddings: backend.clone(),
            schemas: backend.clone(),
        };
        (LinkService::new(engine.clone()), engine)
    }

    #[tokio::test]
    async fn add_entity_ref_to_existing_event() {
        let (svc, engine) = make_service();
        let event = factories::stripe_payment("org_1", "cust_1", 1000);
        let event_id = event.event_id;
        engine.events.insert_events(&[event]).await.unwrap();

        svc.add_entity_ref(event_id, "account", "acc_456", "test_agent")
            .await
            .unwrap();

        let refs = engine.entity_refs.get_refs_for_event(&event_id).await.unwrap();
        let account_ref = refs.iter().find(|r| r.entity_type == "account");
        assert!(account_ref.is_some(), "Should have account ref");
        assert_eq!(account_ref.unwrap().entity_id.as_str(), "acc_456");
    }

    #[tokio::test]
    async fn link_entity_propagates() {
        let (svc, engine) = make_service();

        let v1 = factories::anonymous_page_view("org_1", "sess_1", "/pricing");
        let v2 = factories::anonymous_page_view("org_1", "sess_1", "/signup");
        engine.events.insert_events(&[v1, v2]).await.unwrap();

        let count = svc
            .link_entity(&OrgId::new("org_1"), "session", "sess_1", "customer", "cust_NEW", "agent")
            .await
            .unwrap();

        assert_eq!(count, 2, "Should propagate to 2 session events");
    }

    #[tokio::test]
    async fn create_and_traverse_links() {
        let (svc, engine) = make_service();

        let a = factories::stripe_payment("org_1", "c", 100);
        let b = factories::support_ticket("org_1", "c", "Issue");
        let id_a = a.event_id;
        let id_b = b.event_id;
        engine.events.insert_events(&[a, b]).await.unwrap();

        let link = EventLink {
            link_id: LinkId::new(),
            source_event_id: id_a,
            target_event_id: id_b,
            link_type: "caused_by".to_string(),
            confidence: Confidence::new(0.9).unwrap(),
            reasoning: Some("Payment failure led to ticket".to_string()),
            created_by: "test".to_string(),
            created_at: Utc::now(),
        };
        svc.create_link(&link).await.unwrap();

        let found = svc.get_links(&id_a).await.unwrap();
        assert_eq!(found.len(), 1);

        let traversed = svc
            .traverse(&GraphQuery {
                org_id: OrgId::new("org_1"),
                start_event_id: id_b,
                direction: LinkDirection::Incoming,
                link_types: None,
                max_depth: 5,
                min_confidence: 0.0,
            })
            .await
            .unwrap();
        assert_eq!(traversed.len(), 2);
    }
}
