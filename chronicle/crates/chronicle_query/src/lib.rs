//! Query engine for Chronicle.
//!
//! Provides a unified [`QueryService`] that dispatches to the right
//! storage trait based on query type. Also handles:
//!
//! - **Composite queries**: structured filter + semantic search combined
//! - **Discovery**: list sources, entity types, schemas
//! - **Result enrichment**: attach entity refs to results when requested

use std::sync::Arc;

use chronicle_core::error::ChronicleError;
use chronicle_core::ids::{EntityId, EntityType, EventId, EventType, OrgId, Source};
use chronicle_core::query::{
    EventResult, GraphQuery, SemanticQuery, StructuredQuery, TimelineQuery,
};
use chronicle_store::{
    EntityInfo, EntityTypeInfo, SchemaRegistry, SourceInfo, SourceSchema, StorageEngine,
};

/// Unified query interface over the [`StorageEngine`].
///
/// All query methods go through here. Services and SDKs call
/// `QueryService` rather than individual storage traits directly.
#[derive(Clone)]
pub struct QueryService {
    engine: StorageEngine,
}

impl QueryService {
    pub fn new(engine: StorageEngine) -> Self {
        Self { engine }
    }

    /// Structured query: filter on envelope fields, entity, time range.
    pub async fn query(&self, query: &StructuredQuery) -> Result<Vec<EventResult>, ChronicleError> {
        Ok(self.engine.events.query_structured(query).await?)
    }

    /// Timeline: all events for a specific entity, chronological.
    pub async fn timeline(&self, query: &TimelineQuery) -> Result<Vec<EventResult>, ChronicleError> {
        Ok(self.engine.events.query_timeline(query).await?)
    }

    /// Semantic search: natural language query over embedded events.
    pub async fn search(&self, query: &SemanticQuery) -> Result<Vec<EventResult>, ChronicleError> {
        Ok(self.engine.embeddings.search(query).await?)
    }

    /// Graph traversal: follow causal/relational links.
    pub async fn links(&self, query: &GraphQuery) -> Result<Vec<EventResult>, ChronicleError> {
        Ok(self.engine.links.traverse(query).await?)
    }

    /// Raw SQL query.
    pub async fn sql(&self, org_id: &OrgId, sql: &str) -> Result<Vec<EventResult>, ChronicleError> {
        Ok(self.engine.events.query_sql(org_id, sql).await?)
    }

    /// Get a single event by ID.
    pub async fn get_event(&self, org_id: &OrgId, id: &EventId) -> Result<Option<EventResult>, ChronicleError> {
        Ok(self.engine.events.get_event(org_id, id).await?)
    }

    /// Count events matching a query (no data returned).
    pub async fn count(&self, query: &StructuredQuery) -> Result<u64, ChronicleError> {
        Ok(self.engine.events.count(query).await?)
    }

    // ----- Discovery methods -----

    /// List all connected data sources for an org.
    pub async fn describe_sources(&self, org_id: &OrgId) -> Result<Vec<SourceInfo>, ChronicleError> {
        Ok(self.engine.schemas.describe_sources(org_id).await?)
    }

    /// Get the payload schema for a specific (source, event_type).
    pub async fn describe_schema(
        &self,
        org_id: &OrgId,
        source: &Source,
        event_type: &EventType,
    ) -> Result<Option<SourceSchema>, ChronicleError> {
        Ok(self.engine.schemas.get_schema(org_id, source, event_type).await?)
    }

    /// List all entity types in use.
    pub async fn describe_entity_types(&self, org_id: &OrgId) -> Result<Vec<EntityTypeInfo>, ChronicleError> {
        Ok(self.engine.entity_refs.list_entity_types(org_id).await?)
    }

    /// List entities of a specific type.
    pub async fn list_entities(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        limit: usize,
    ) -> Result<Vec<EntityInfo>, ChronicleError> {
        Ok(self.engine.entity_refs.list_entities(org_id, entity_type, limit).await?)
    }

    /// Get entity refs for a specific event.
    pub async fn get_entity_refs(&self, event_id: &EventId) -> Result<Vec<chronicle_core::EntityRef>, ChronicleError> {
        Ok(self.engine.entity_refs.get_refs_for_event(event_id).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_core::ids::{EntityId, EntityType, OrgId, Source};
    use chronicle_core::query::OrderBy;
    use chronicle_store::memory::InMemoryBackend;
    use chronicle_test_fixtures::factories;

    fn make_service() -> (QueryService, Arc<InMemoryBackend>) {
        let backend = Arc::new(InMemoryBackend::new());
        let engine = StorageEngine {
            events: backend.clone(),
            entity_refs: backend.clone(),
            links: backend.clone(),
            embeddings: backend.clone(),
            schemas: backend.clone(),
            subscriptions: Some(backend.clone()),
        };
        (QueryService::new(engine), backend)
    }

    #[tokio::test]
    async fn query_service_structured() {
        let (svc, _backend) = make_service();

        let events = vec![
            factories::stripe_payment("org_1", "cust_1", 1000),
            factories::support_ticket("org_1", "cust_1", "Help"),
        ];
        svc.engine.events.insert_events(&events).await.unwrap();

        let query = StructuredQuery {
            org_id: OrgId::new("org_1"),
            source: Some(Source::new("stripe")),
            entity: None,
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: 50,
            offset: 0,
        };

        let results = svc.query(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event.source, "stripe");
    }

    #[tokio::test]
    async fn query_service_timeline() {
        let (svc, _backend) = make_service();

        let events = vec![
            factories::stripe_payment("org_1", "cust_T", 1000),
            factories::support_ticket("org_1", "cust_T", "Issue"),
            factories::stripe_payment("org_1", "cust_OTHER", 2000),
        ];
        svc.engine.events.insert_events(&events).await.unwrap();

        let query = TimelineQuery {
            org_id: OrgId::new("org_1"),
            entity_type: EntityType::new("customer"),
            entity_id: EntityId::new("cust_T"),
            time_range: None,
            sources: None,
            include_linked: false,
            include_entity_refs: false,
            link_depth: 0,
            min_link_confidence: 0.0,
        };

        let results = svc.timeline(&query).await.unwrap();
        assert_eq!(results.len(), 2, "Should only return cust_T's events");
    }

    #[tokio::test]
    async fn query_service_graph() {
        let (svc, _backend) = make_service();

        let evt_a = factories::stripe_payment("org_1", "c", 100);
        let evt_b = factories::support_ticket("org_1", "c", "X");
        let id_a = evt_a.event_id;
        let id_b = evt_b.event_id;

        svc.engine.events.insert_events(&[evt_a, evt_b]).await.unwrap();
        let link = factories::causal_link(id_a, id_b, 0.9);
        svc.engine.links.create_link(&link).await.unwrap();

        let query = GraphQuery {
            org_id: OrgId::new("org_1"),
            start_event_id: id_b,
            direction: chronicle_core::link::LinkDirection::Incoming,
            link_types: None,
            max_depth: 5,
            min_confidence: 0.0,
        };

        let results = svc.links(&query).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn query_service_discovery() {
        let (svc, _backend) = make_service();

        let events = vec![
            factories::stripe_payment("org_1", "c1", 100),
            factories::support_ticket("org_1", "c2", "Help"),
        ];
        svc.engine.events.insert_events(&events).await.unwrap();

        let types = svc.describe_entity_types(&OrgId::new("org_1")).await.unwrap();
        assert!(!types.is_empty(), "Should find entity types from ingested events");
    }
}
