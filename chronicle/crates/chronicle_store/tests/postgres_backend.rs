//! Integration tests for PostgresBackend.
//!
//! Runs against a real Postgres instance on localhost:5433.
//! Each test gets its own org_id to avoid interference.

#![cfg(feature = "postgres")]

use std::sync::Arc;

use chronicle_store::postgres::PostgresBackend;
use chronicle_store::traits::*;
use chronicle_store::StorageEngine;
use chronicle_test_fixtures::{factories, trait_tests};
use chronicle_core::ids::*;
use chronicle_core::query::*;

const TEST_DB_URL: &str = "postgres://chronicle:chronicle@localhost:5433/chronicle";

async fn backend() -> PostgresBackend {
    let backend = PostgresBackend::new(TEST_DB_URL).await
        .expect("Failed to connect to test Postgres. Is it running on port 5433?");
    let _ = backend.run_migrations().await;
    backend
}

#[tokio::test]
async fn pg_trait_suite_events() {
    let b = backend().await;
    trait_tests::run_event_store_tests(&b).await;
}

#[tokio::test]
async fn pg_trait_suite_entity_refs() {
    let b = backend().await;
    trait_tests::run_entity_ref_tests(&b, &b).await;
}

#[tokio::test]
async fn pg_insert_and_query() {
    let b = backend().await;

    let events = vec![
        factories::stripe_payment("pg_iq", "cust_1", 4999),
        factories::support_ticket("pg_iq", "cust_1", "Postgres test"),
        factories::stripe_payment("pg_iq", "cust_2", 2999),
    ];
    b.insert_events(&events).await.unwrap();

    let results = b.query_structured(&StructuredQuery {
        org_id: OrgId::new("pg_iq"),
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
    }).await.unwrap();

    assert_eq!(results.len(), 2, "Should find 2 stripe events");
}

#[tokio::test]
async fn pg_timeline() {
    let b = backend().await;

    let events = vec![
        factories::stripe_payment("pg_tl", "cust_tl", 1000),
        factories::support_ticket("pg_tl", "cust_tl", "Help"),
        factories::product_page_view("pg_tl", "cust_tl", "/dashboard"),
    ];
    b.insert_events(&events).await.unwrap();

    let timeline = b.query_timeline(&TimelineQuery {
        org_id: OrgId::new("pg_tl"),
        entity_type: EntityType::new("customer"),
        entity_id: EntityId::new("cust_tl"),
        time_range: None,
        sources: None,
        include_linked: false,
        include_entity_refs: false,
        link_depth: 0,
        min_link_confidence: 0.0,
    }).await.unwrap();

    assert_eq!(timeline.len(), 3, "Timeline should span 3 sources");
    for pair in timeline.windows(2) {
        assert!(pair[0].event.event_time <= pair[1].event.event_time);
    }
}

#[tokio::test]
async fn pg_entity_linking() {
    let b = backend().await;

    let v1 = factories::anonymous_page_view("pg_el", "sess_pg", "/pricing");
    let v2 = factories::anonymous_page_view("pg_el", "sess_pg", "/signup");
    b.insert_events(&[v1, v2]).await.unwrap();

    let linked = b.link_entity(
        &OrgId::new("pg_el"),
        &EntityType::new("session"),
        &EntityId::new("sess_pg"),
        &EntityType::new("customer"),
        &EntityId::new("cust_pg_new"),
        "test",
    ).await.unwrap();
    assert_eq!(linked, 2);

    let timeline = b.query_timeline(&TimelineQuery {
        org_id: OrgId::new("pg_el"),
        entity_type: EntityType::new("customer"),
        entity_id: EntityId::new("cust_pg_new"),
        time_range: None,
        sources: None,
        include_linked: false,
        include_entity_refs: false,
        link_depth: 0,
        min_link_confidence: 0.0,
    }).await.unwrap();

    assert_eq!(timeline.len(), 2, "Customer should see linked session events");
}

#[tokio::test]
async fn pg_event_links() {
    let b = backend().await;

    let a = factories::stripe_payment("pg_lk", "cust_lk", 100);
    let b_evt = factories::support_ticket("pg_lk", "cust_lk", "Issue");
    let id_a = a.event_id;
    let id_b = b_evt.event_id;
    b.insert_events(&[a, b_evt]).await.unwrap();

    let link = factories::causal_link(id_a, id_b, 0.9);
    b.create_link(&link).await.unwrap();

    let found = b.get_links_for_event(&id_a).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].link_type, "caused_by");
}
