//! Shared trait test suites.
//!
//! Every storage backend must pass these tests to be considered correct.
//! Call `run_event_store_tests(&backend).await` from your backend's test
//! module -- the same suite runs against `PostgresBackend`, `HybridBackend`,
//! and `KurrentBackend`.
//!
//! These are defined as plain async functions (not `#[test]`) so they can
//! be called from any test harness with any backend instance.

use chronicle_core::{
    event::EventBuilder,
    ids::{EntityId, EntityType, EventId, OrgId, Source},
    query::{OrderBy, StructuredQuery, TimelineQuery},
    time_range::TimeRange,
};
use chronicle_store::traits::{EntityRefStore, EventStore};

use crate::factories;

// ---------------------------------------------------------------------------
// EventStore test suite
// ---------------------------------------------------------------------------

/// Run the full `EventStore` test suite against any backend.
///
/// The backend must already be initialized (migrations run, etc.).
pub async fn run_event_store_tests(store: &dyn EventStore) {
    test_insert_and_get_single_event(store).await;
    test_insert_batch(store).await;
    test_get_nonexistent_returns_none(store).await;
    test_structured_query_by_source(store).await;
    test_structured_query_by_event_type(store).await;
    test_structured_query_with_limit(store).await;
    test_count(store).await;
}

async fn test_insert_and_get_single_event(store: &dyn EventStore) {
    let event = factories::stripe_payment("test_org", "cust_1", 4999);
    let event_id = event.event_id;
    let org_id = event.org_id;

    let ids = store.insert_events(&[event]).await.expect("insert should succeed");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], event_id);

    let result = store.get_event(&org_id, &event_id).await.expect("get should succeed");
    assert!(result.is_some(), "Event should be found after insert");
    let result = result.unwrap();
    assert_eq!(result.event.source, "stripe");
    assert_eq!(result.event.event_type, "payment_intent.succeeded");
}

async fn test_insert_batch(store: &dyn EventStore) {
    let events: Vec<_> = (0..10)
        .map(|i| factories::stripe_payment("test_org", &format!("cust_{i}"), 1000 + i * 100))
        .collect();

    let ids = store.insert_events(&events).await.expect("batch insert should succeed");
    assert_eq!(ids.len(), 10);
}

async fn test_get_nonexistent_returns_none(store: &dyn EventStore) {
    let org_id = OrgId::new("test_org");
    let fake_id = EventId::new();
    let result = store.get_event(&org_id, &fake_id).await.expect("get should not error");
    assert!(result.is_none(), "Nonexistent event should return None");
}

async fn test_structured_query_by_source(store: &dyn EventStore) {
    // Insert events from two sources
    let stripe = factories::stripe_payment("test_org", "cust_q1", 999);
    let support = factories::support_ticket("test_org", "cust_q1", "Help");
    store.insert_events(&[stripe, support]).await.unwrap();

    let query = StructuredQuery {
        org_id: OrgId::new("test_org"),
        source: Some(Source::new("stripe")),
        entity: None,
        topic: None,
        event_type: None,
        time_range: None,
        payload_filters: vec![],
        group_by: None,
        order_by: OrderBy::EventTimeDesc,
        limit: 100,
        offset: 0,
    };

    let results = store.query_structured(&query).await.expect("query should succeed");
    assert!(
        results.iter().all(|r| r.event.source == "stripe"),
        "All results should be from stripe"
    );
}

async fn test_structured_query_by_event_type(store: &dyn EventStore) {
    let query = StructuredQuery {
        org_id: OrgId::new("test_org"),
        source: None,
        entity: None,
        topic: None,
        event_type: Some(chronicle_core::ids::EventType::new("payment_intent.succeeded")),
        time_range: None,
        payload_filters: vec![],
        group_by: None,
        order_by: OrderBy::EventTimeDesc,
        limit: 100,
        offset: 0,
    };

    let results = store.query_structured(&query).await.expect("query should succeed");
    for r in &results {
        assert_eq!(r.event.event_type, "payment_intent.succeeded");
    }
}

async fn test_structured_query_with_limit(store: &dyn EventStore) {
    let query = StructuredQuery {
        org_id: OrgId::new("test_org"),
        source: None,
        entity: None,
        topic: None,
        event_type: None,
        time_range: None,
        payload_filters: vec![],
        group_by: None,
        order_by: OrderBy::EventTimeDesc,
        limit: 3,
        offset: 0,
    };

    let results = store.query_structured(&query).await.expect("query should succeed");
    assert!(results.len() <= 3, "Limit should be respected");
}

async fn test_count(store: &dyn EventStore) {
    let query = StructuredQuery {
        org_id: OrgId::new("test_org"),
        source: Some(Source::new("stripe")),
        entity: None,
        topic: None,
        event_type: None,
        time_range: None,
        payload_filters: vec![],
        group_by: None,
        order_by: OrderBy::EventTimeDesc,
        limit: 100,
        offset: 0,
    };

    let count = store.count(&query).await.expect("count should succeed");
    assert!(count > 0, "Should have at least one stripe event from prior tests");
}

// ---------------------------------------------------------------------------
// EntityRefStore test suite
// ---------------------------------------------------------------------------

/// Run the full `EntityRefStore` test suite against any backend.
pub async fn run_entity_ref_tests(store: &(dyn EntityRefStore + Sync), event_store: &dyn EventStore) {
    test_add_and_get_refs(store, event_store).await;
    test_get_events_for_entity(store, event_store).await;
    test_duplicate_ref_is_idempotent(store, event_store).await;
}

async fn test_add_and_get_refs(store: &(dyn EntityRefStore + Sync), event_store: &dyn EventStore) {
    let event = factories::stripe_payment("test_org", "cust_ref_1", 1000);
    let event_id = event.event_id;
    event_store.insert_events(&[event.clone()]).await.unwrap();

    let refs = event.materialize_entity_refs("test");
    store.add_refs(&refs).await.expect("add_refs should succeed");

    let retrieved = store.get_refs_for_event(&event_id).await.expect("get should succeed");
    assert!(!retrieved.is_empty(), "Should have at least one ref");
    assert_eq!(retrieved[0].entity_type, "customer");
    assert_eq!(retrieved[0].entity_id.as_str(), "cust_ref_1");
}

async fn test_get_events_for_entity(store: &(dyn EntityRefStore + Sync), _event_store: &dyn EventStore) {
    let org_id = OrgId::new("test_org");
    let entity_type = EntityType::new("customer");
    let entity_id = EntityId::new("cust_ref_1");

    let event_ids = store
        .get_events_for_entity(&org_id, &entity_type, &entity_id)
        .await
        .expect("should succeed");
    assert!(!event_ids.is_empty(), "Should find events for the customer");
}

async fn test_duplicate_ref_is_idempotent(store: &(dyn EntityRefStore + Sync), event_store: &dyn EventStore) {
    let event = factories::stripe_payment("test_org", "cust_dup", 500);
    event_store.insert_events(&[event.clone()]).await.unwrap();

    let refs = event.materialize_entity_refs("test");
    store.add_refs(&refs).await.unwrap();
    // Second insert should not error
    store.add_refs(&refs).await.expect("duplicate add_refs should be idempotent");
}
