//! Integration tests for the in-memory backend.
//!
//! Runs the shared trait test suites from `chronicle_test_fixtures`
//! against `InMemoryBackend`. Every storage backend must pass these
//! same suites.

use chronicle_store::memory::InMemoryBackend;
use chronicle_test_fixtures::trait_tests;

#[tokio::test]
async fn memory_passes_event_store_suite() {
    let backend = InMemoryBackend::new();
    trait_tests::run_event_store_tests(&backend).await;
}

#[tokio::test]
async fn memory_passes_entity_ref_suite() {
    let backend = InMemoryBackend::new();
    trait_tests::run_entity_ref_tests(&backend, &backend).await;
}

// Additional in-memory-specific tests that go beyond the trait suites.

mod event_tests {
    use chronicle_store::memory::InMemoryBackend;
    use chronicle_store::traits::{EventStore, EntityRefStore};
    use chronicle_test_fixtures::factories;
    use chronicle_core::ids::{OrgId, EntityType, EntityId};
    use chronicle_core::query::{StructuredQuery, OrderBy, TimelineQuery};

    #[tokio::test]
    async fn query_by_entity() {
        let backend = InMemoryBackend::new();

        let evt1 = factories::stripe_payment("org_1", "cust_A", 1000);
        let evt2 = factories::stripe_payment("org_1", "cust_B", 2000);
        let evt3 = factories::support_ticket("org_1", "cust_A", "Help me");

        backend.insert_events(&[evt1, evt2, evt3]).await.unwrap();

        let query = StructuredQuery {
            org_id: OrgId::new("org_1"),
            entity: Some((EntityType::new("customer"), EntityId::new("cust_A"))),
            source: None,
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: 100,
            offset: 0,
        };

        let results = backend.query_structured(&query).await.unwrap();
        assert_eq!(results.len(), 2, "Should find 2 events for cust_A (payment + ticket)");

        let sources: Vec<&str> = results.iter().map(|r| r.event.source.as_str()).collect();
        assert!(sources.contains(&"stripe"), "Should include stripe payment");
        assert!(sources.contains(&"support"), "Should include support ticket");
    }

    #[tokio::test]
    async fn timeline_across_sources() {
        let backend = InMemoryBackend::new();

        let payment = factories::stripe_payment("org_1", "cust_T", 5000);
        let ticket = factories::support_ticket("org_1", "cust_T", "Question");
        let page = factories::product_page_view("org_1", "cust_T", "/settings");

        backend.insert_events(&[payment, ticket, page]).await.unwrap();

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

        let results = backend.query_timeline(&query).await.unwrap();
        assert_eq!(results.len(), 3, "Timeline should include events from all 3 sources");

        // Verify chronological order
        for pair in results.windows(2) {
            assert!(pair[0].event.event_time <= pair[1].event.event_time);
        }
    }

    #[tokio::test]
    async fn jit_entity_linking() {
        let backend = InMemoryBackend::new();

        // Anonymous session events
        let v1 = factories::anonymous_page_view("org_1", "sess_X", "/pricing");
        let v2 = factories::anonymous_page_view("org_1", "sess_X", "/signup");
        backend.insert_events(&[v1, v2]).await.unwrap();

        // Before linking: customer timeline is empty
        let query = TimelineQuery {
            org_id: OrgId::new("org_1"),
            entity_type: EntityType::new("customer"),
            entity_id: EntityId::new("cust_NEW"),
            time_range: None,
            sources: None,
            include_linked: false,
            include_entity_refs: false,
            link_depth: 0,
            min_link_confidence: 0.0,
        };
        let before = backend.query_timeline(&query).await.unwrap();
        assert_eq!(before.len(), 0, "No events for customer before linking");

        // JIT link: session -> customer
        let linked = backend.link_entity(
            &OrgId::new("org_1"),
            &EntityType::new("session"),
            &EntityId::new("sess_X"),
            &EntityType::new("customer"),
            &EntityId::new("cust_NEW"),
            "test_agent",
        ).await.unwrap();
        assert_eq!(linked, 2, "Should link 2 session events to the customer");

        // After linking: customer timeline includes the anonymous events
        let after = backend.query_timeline(&query).await.unwrap();
        assert_eq!(after.len(), 2, "Customer should now see 2 linked events");
    }
}

mod link_tests {
    use chronicle_store::memory::InMemoryBackend;
    use chronicle_store::traits::{EventStore, EventLinkStore};
    use chronicle_test_fixtures::factories;
    use chronicle_core::ids::{OrgId, EventId};
    use chronicle_core::link::LinkDirection;
    use chronicle_core::query::GraphQuery;

    #[tokio::test]
    async fn graph_traversal() {
        let backend = InMemoryBackend::new();

        let evt_a = factories::stripe_payment("org_1", "cust_1", 1000);
        let evt_b = factories::support_ticket("org_1", "cust_1", "Issue");
        let evt_c = factories::stripe_subscription_cancelled("org_1", "cust_1");

        let id_a = evt_a.event_id;
        let id_b = evt_b.event_id;
        let id_c = evt_c.event_id;

        backend.insert_events(&[evt_a, evt_b, evt_c]).await.unwrap();

        // Chain: A -> B -> C
        let link_ab = factories::causal_link(id_a, id_b, 0.9);
        let link_bc = factories::causal_link(id_b, id_c, 0.85);
        backend.create_link(&OrgId::new("org_1"), &link_ab).await.unwrap();
        backend.create_link(&OrgId::new("org_1"), &link_bc).await.unwrap();

        // Traverse from C incoming, depth 5
        let query = GraphQuery {
            org_id: OrgId::new("org_1"),
            start_event_id: id_c,
            direction: LinkDirection::Incoming,
            link_types: Some(vec!["caused_by".to_string()]),
            max_depth: 5,
            min_confidence: 0.7,
        };

        let results = backend.traverse(&query).await.unwrap();
        assert_eq!(results.len(), 3, "Should find all 3 events in the chain");
    }

    #[tokio::test]
    async fn depth_limiting() {
        let backend = InMemoryBackend::new();

        let evt_a = factories::stripe_payment("org_1", "c", 100);
        let evt_b = factories::support_ticket("org_1", "c", "X");
        let evt_c = factories::stripe_subscription_cancelled("org_1", "c");

        let id_a = evt_a.event_id;
        let id_b = evt_b.event_id;
        let id_c = evt_c.event_id;

        backend.insert_events(&[evt_a, evt_b, evt_c]).await.unwrap();

        backend.create_link(&OrgId::new("org_1"), &factories::causal_link(id_a, id_b, 0.9)).await.unwrap();
        backend.create_link(&OrgId::new("org_1"), &factories::causal_link(id_b, id_c, 0.9)).await.unwrap();

        // Depth 1: should only find C and B (one hop)
        let query = GraphQuery {
            org_id: OrgId::new("org_1"),
            start_event_id: id_c,
            direction: LinkDirection::Incoming,
            link_types: None,
            max_depth: 1,
            min_confidence: 0.0,
        };

        let results = backend.traverse(&query).await.unwrap();
        assert_eq!(results.len(), 2, "Depth 1 should find start + 1 hop");
    }

    #[tokio::test]
    async fn confidence_filtering() {
        let backend = InMemoryBackend::new();

        let evt_a = factories::stripe_payment("org_1", "c", 100);
        let evt_b = factories::support_ticket("org_1", "c", "X");

        let id_a = evt_a.event_id;
        let id_b = evt_b.event_id;

        backend.insert_events(&[evt_a, evt_b]).await.unwrap();
        backend.create_link(&OrgId::new("org_1"), &factories::causal_link(id_a, id_b, 0.3)).await.unwrap();

        let query = GraphQuery {
            org_id: OrgId::new("org_1"),
            start_event_id: id_b,
            direction: LinkDirection::Incoming,
            link_types: None,
            max_depth: 5,
            min_confidence: 0.5,
        };

        let results = backend.traverse(&query).await.unwrap();
        assert_eq!(results.len(), 1, "Low confidence link should be filtered out");
    }
}
