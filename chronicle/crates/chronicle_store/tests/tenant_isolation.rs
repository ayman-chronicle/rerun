//! Multi-tenant isolation tests.
//!
//! Creates events in org_A and org_B, then verifies org_A cannot see
//! org_B's data through any API method. This is the SOC2 "prove it"
//! test -- every query path must be verified.

use std::sync::Arc;

use chronicle_core::entity_ref::EntityRef;
use chronicle_core::event::EventBuilder;
use chronicle_core::ids::*;
use chronicle_core::link::EventLink;
use chronicle_core::query::{GraphQuery, OrderBy, StructuredQuery, TimelineQuery};

use chronicle_store::memory::InMemoryBackend;
use chronicle_store::tenant_guard::TenantGuard;
use chronicle_store::traits::*;
use chronicle_store::StorageEngine;

fn make_engine() -> (StorageEngine, Arc<InMemoryBackend>) {
    let backend = Arc::new(InMemoryBackend::new());
    let engine = StorageEngine {
        events: backend.clone(),
        entity_refs: backend.clone(),
        links: backend.clone(),
        embeddings: backend.clone(),
        schemas: backend.clone(),
        subscriptions: Some(backend.clone()),
    };
    (engine, backend)
}

fn org_a() -> OrgId { OrgId::new("org_A") }
fn org_b() -> OrgId { OrgId::new("org_B") }

/// Seed: create events in both orgs with overlapping entity types.
async fn seed(engine: &StorageEngine) -> (EventId, EventId, EventId, EventId) {
    let a1 = EventBuilder::new("org_A", "stripe", "payments", "charge.succeeded")
        .entity("customer", "cust_shared_name")
        .build();
    let a2 = EventBuilder::new("org_A", "support", "tickets", "ticket.created")
        .entity("customer", "cust_shared_name")
        .build();
    let b1 = EventBuilder::new("org_B", "stripe", "payments", "charge.succeeded")
        .entity("customer", "cust_shared_name")
        .build();
    let b2 = EventBuilder::new("org_B", "gorgias", "tickets", "ticket.created")
        .entity("customer", "cust_b_only")
        .build();

    let a1_id = a1.event_id;
    let a2_id = a2.event_id;
    let b1_id = b1.event_id;
    let b2_id = b2.event_id;

    engine.events.insert_events(&[a1, a2, b1, b2]).await.unwrap();

    (a1_id, a2_id, b1_id, b2_id)
}

// ---------------------------------------------------------------------------
// Event isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_event_cross_org_returns_none() {
    let (engine, _) = make_engine();
    let (_, _, b1_id, _) = seed(&engine).await;

    let result = engine.events.get_event(&org_a(), &b1_id).await.unwrap();
    assert!(result.is_none(), "org_A must not see org_B's event");
}

#[tokio::test]
async fn structured_query_only_returns_own_org() {
    let (engine, _) = make_engine();
    seed(&engine).await;

    let query = StructuredQuery {
        org_id: org_a(),
        source: None, entity: None, topic: None, event_type: None,
        time_range: None, payload_filters: vec![], group_by: None,
        order_by: OrderBy::EventTimeDesc, limit: 100, offset: 0,
    };
    let results = engine.events.query_structured(&query).await.unwrap();
    assert_eq!(results.len(), 2, "org_A has exactly 2 events");
    assert!(results.iter().all(|r| r.event.org_id == org_a()));
}

#[tokio::test]
async fn count_only_counts_own_org() {
    let (engine, _) = make_engine();
    seed(&engine).await;

    let query = StructuredQuery {
        org_id: org_b(),
        source: None, entity: None, topic: None, event_type: None,
        time_range: None, payload_filters: vec![], group_by: None,
        order_by: OrderBy::EventTimeDesc, limit: 100, offset: 0,
    };
    let count = engine.events.count(&query).await.unwrap();
    assert_eq!(count, 2, "org_B has exactly 2 events");
}

// ---------------------------------------------------------------------------
// Entity ref isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_refs_cross_org_returns_empty() {
    let (engine, _) = make_engine();
    let (a1_id, _, _, _) = seed(&engine).await;

    let refs = engine.entity_refs.get_refs_for_event(&org_b(), &a1_id).await.unwrap();
    assert!(refs.is_empty(), "org_B must not see org_A's entity refs");
}

#[tokio::test]
async fn get_events_for_entity_cross_org_returns_empty() {
    let (engine, _) = make_engine();
    seed(&engine).await;

    let events = engine.entity_refs
        .get_events_for_entity(&org_a(), &EntityType::new("customer"), &EntityId::new("cust_b_only"))
        .await.unwrap();
    assert!(events.is_empty(), "org_A must not see org_B's entity cust_b_only");
}

#[tokio::test]
async fn shared_entity_name_isolated_per_org() {
    let (engine, _) = make_engine();
    seed(&engine).await;

    let a_events = engine.entity_refs
        .get_events_for_entity(&org_a(), &EntityType::new("customer"), &EntityId::new("cust_shared_name"))
        .await.unwrap();
    let b_events = engine.entity_refs
        .get_events_for_entity(&org_b(), &EntityType::new("customer"), &EntityId::new("cust_shared_name"))
        .await.unwrap();

    assert_eq!(a_events.len(), 2, "org_A has 2 events for cust_shared_name");
    assert_eq!(b_events.len(), 1, "org_B has 1 event for cust_shared_name");

    for eid in &a_events {
        assert!(!b_events.contains(eid), "No event ID should appear in both orgs");
    }
}

#[tokio::test]
async fn list_entity_types_isolated() {
    let (engine, _) = make_engine();
    seed(&engine).await;

    let a_types = engine.entity_refs.list_entity_types(&org_a()).await.unwrap();
    let b_types = engine.entity_refs.list_entity_types(&org_b()).await.unwrap();

    assert!(a_types.iter().all(|t| t.entity_type == EntityType::new("customer")));
    assert!(b_types.iter().all(|t| t.entity_type == EntityType::new("customer")));
}

#[tokio::test]
async fn add_refs_isolated() {
    let (engine, _) = make_engine();
    let (a1_id, _, _, _) = seed(&engine).await;

    let extra_ref = EntityRef::new(a1_id, "account", "acc_001", "test");
    engine.entity_refs.add_refs(&org_a(), &[extra_ref]).await.unwrap();

    let a_refs = engine.entity_refs.get_refs_for_event(&org_a(), &a1_id).await.unwrap();
    let b_refs = engine.entity_refs.get_refs_for_event(&org_b(), &a1_id).await.unwrap();

    assert!(a_refs.iter().any(|r| r.entity_type == EntityType::new("account")));
    assert!(b_refs.is_empty(), "org_B must not see refs added by org_A");
}

// ---------------------------------------------------------------------------
// Link isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn links_isolated_per_org() {
    let (engine, _) = make_engine();
    let (a1_id, a2_id, b1_id, b2_id) = seed(&engine).await;

    let link_a = EventLink {
        link_id: LinkId::new(),
        source_event_id: a1_id,
        target_event_id: a2_id,
        link_type: "caused_by".to_string(),
        confidence: Confidence::new(0.9).unwrap(),
        reasoning: None,
        created_by: "test".to_string(),
        created_at: chrono::Utc::now(),
    };
    let link_b = EventLink {
        link_id: LinkId::new(),
        source_event_id: b1_id,
        target_event_id: b2_id,
        link_type: "caused_by".to_string(),
        confidence: Confidence::new(0.9).unwrap(),
        reasoning: None,
        created_by: "test".to_string(),
        created_at: chrono::Utc::now(),
    };

    engine.links.create_link(&org_a(), &link_a).await.unwrap();
    engine.links.create_link(&org_b(), &link_b).await.unwrap();

    let a_links = engine.links.get_links_for_event(&org_a(), &a1_id).await.unwrap();
    let b_links_via_a = engine.links.get_links_for_event(&org_a(), &b1_id).await.unwrap();

    assert_eq!(a_links.len(), 1, "org_A should see its own link");
    assert!(b_links_via_a.is_empty(), "org_A must not see org_B's links");
}

#[tokio::test]
async fn traverse_isolated_per_org() {
    let (engine, _) = make_engine();
    let (a1_id, a2_id, b1_id, b2_id) = seed(&engine).await;

    let link_a = EventLink {
        link_id: LinkId::new(),
        source_event_id: a1_id, target_event_id: a2_id,
        link_type: "caused_by".to_string(),
        confidence: Confidence::new(0.9).unwrap(),
        reasoning: None, created_by: "test".to_string(), created_at: chrono::Utc::now(),
    };
    let link_b = EventLink {
        link_id: LinkId::new(),
        source_event_id: b1_id, target_event_id: b2_id,
        link_type: "caused_by".to_string(),
        confidence: Confidence::new(0.9).unwrap(),
        reasoning: None, created_by: "test".to_string(), created_at: chrono::Utc::now(),
    };

    engine.links.create_link(&org_a(), &link_a).await.unwrap();
    engine.links.create_link(&org_b(), &link_b).await.unwrap();

    let query = GraphQuery {
        org_id: org_a(),
        start_event_id: a1_id,
        direction: chronicle_core::link::LinkDirection::Both,
        link_types: None, max_depth: 5, min_confidence: 0.0,
    };
    let results = engine.links.traverse(&query).await.unwrap();
    assert_eq!(results.len(), 2, "Should find both org_A events");
    assert!(results.iter().all(|r| r.event.org_id == org_a()));
}

// ---------------------------------------------------------------------------
// Embedding isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn embedding_isolation() {
    let (engine, _) = make_engine();
    let (a1_id, _, _, _) = seed(&engine).await;

    let emb = EventEmbedding {
        event_id: a1_id,
        org_id: org_a(),
        embedding: vec![0.1, 0.2, 0.3],
        embedded_text: "test".to_string(),
        model_version: "v1".to_string(),
    };
    engine.embeddings.store_embeddings(&[emb]).await.unwrap();

    assert!(engine.embeddings.has_embedding(&org_a(), &a1_id).await.unwrap());
    assert!(!engine.embeddings.has_embedding(&org_b(), &a1_id).await.unwrap(),
        "org_B must not see org_A's embedding");
}

// ---------------------------------------------------------------------------
// Timeline isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn timeline_isolated() {
    let (engine, _) = make_engine();
    seed(&engine).await;

    let query = TimelineQuery {
        org_id: org_a(),
        entity_type: EntityType::new("customer"),
        entity_id: EntityId::new("cust_shared_name"),
        time_range: None, sources: None,
        include_linked: false, include_entity_refs: false,
        link_depth: 0, min_link_confidence: 0.0,
    };
    let results = engine.events.query_timeline(&query).await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.event.org_id == org_a()));
}

// ---------------------------------------------------------------------------
// TenantGuard ownership validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tenant_guard_rejects_cross_org_write() {
    let (engine, _) = make_engine();
    let (_, _, b1_id, _) = seed(&engine).await;

    let guard = TenantGuard::new(engine, org_a());
    let result = guard.assert_event_owned(&b1_id).await;
    assert!(result.is_err(), "org_A must not claim ownership of org_B's event");
}
