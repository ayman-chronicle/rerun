//! Integration tests for the REST API.
//!
//! Starts a real axum server on a random port, makes HTTP requests
//! with reqwest, and verifies responses. Uses InMemoryBackend
//! so no database is needed.

use std::sync::Arc;
use std::net::SocketAddr;

use chronicle_server::rest::build_router;
use chronicle_server::ServerState;
use chronicle_store::memory::InMemoryBackend;
use chronicle_store::StorageEngine;

/// Start a test server on a random port and return the base URL.
async fn start_test_server() -> String {
    let backend = Arc::new(InMemoryBackend::new());
    let engine = StorageEngine {
        events: backend.clone(),
        entity_refs: backend.clone(),
        links: backend.clone(),
        embeddings: backend.clone(),
        schemas: backend.clone(),
            subscriptions: Some(backend.clone()),
    };
    let state = ServerState::new(engine);
    let router = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn ingest_and_query_round_trip() {
    let base = start_test_server().await;
    let client = reqwest::Client::new();

    // Ingest an event
    let resp = client
        .post(format!("{base}/v1/events"))
        .json(&serde_json::json!({
            "org_id": "org_test",
            "source": "stripe",
            "topic": "payments",
            "event_type": "payment_intent.succeeded",
            "entities": {"customer": "cust_1"},
            "payload": {"amount": 4999, "currency": "usd"},
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["count"], 1);
    assert!(!body["event_ids"][0].as_str().unwrap().is_empty());

    // Query it back
    let resp = client
        .get(format!("{base}/v1/events"))
        .query(&[
            ("org_id", "org_test"),
            ("source", "stripe"),
        ])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let results: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["event"]["source"], "stripe");
}

#[tokio::test]
async fn batch_ingest() {
    let base = start_test_server().await;
    let client = reqwest::Client::new();

    let events: Vec<serde_json::Value> = (0..5)
        .map(|i| {
            serde_json::json!({
                "org_id": "org_test",
                "source": "stripe",
                "topic": "payments",
                "event_type": "charge.created",
                "entities": {"customer": format!("cust_{i}")},
                "payload": {"amount": i * 1000},
            })
        })
        .collect();

    let resp = client
        .post(format!("{base}/v1/events/batch"))
        .json(&events)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["count"], 5);
}

#[tokio::test]
async fn timeline_query() {
    let base = start_test_server().await;
    let client = reqwest::Client::new();

    // Ingest events for one customer across sources
    for (source, topic, event_type) in &[
        ("stripe", "payments", "charge.created"),
        ("support", "tickets", "ticket.created"),
        ("product", "usage", "page.viewed"),
    ] {
        client
            .post(format!("{base}/v1/events"))
            .json(&serde_json::json!({
                "org_id": "org_test",
                "source": source,
                "topic": topic,
                "event_type": event_type,
                "entities": {"customer": "cust_timeline"},
                "payload": {"test": true},
            }))
            .send()
            .await
            .unwrap();
    }

    let resp = client
        .get(format!("{base}/v1/timeline/customer/cust_timeline"))
        .query(&[("org_id", "org_test")])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let results: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(results.len(), 3, "Timeline should have events from all 3 sources");
}

#[tokio::test]
async fn jit_entity_linking_via_api() {
    let base = start_test_server().await;
    let client = reqwest::Client::new();

    // Ingest anonymous session events
    for url in &["/pricing", "/signup"] {
        client
            .post(format!("{base}/v1/events"))
            .json(&serde_json::json!({
                "org_id": "org_test",
                "source": "product",
                "topic": "usage",
                "event_type": "page.viewed",
                "entities": {"session": "sess_anon"},
                "payload": {"url": url},
            }))
            .send()
            .await
            .unwrap();
    }

    // Before linking: customer timeline is empty
    let resp = client
        .get(format!("{base}/v1/timeline/customer/cust_linked"))
        .query(&[("org_id", "org_test")])
        .send()
        .await
        .unwrap();
    let before: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(before.len(), 0);

    // JIT link session to customer
    let resp = client
        .post(format!("{base}/v1/link-entity"))
        .json(&serde_json::json!({
            "org_id": "org_test",
            "from_entity_type": "session",
            "from_entity_id": "sess_anon",
            "to_entity_type": "customer",
            "to_entity_id": "cust_linked",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let link_result: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(link_result["linked_count"], 2);

    // After linking: customer timeline includes the anonymous events
    let resp = client
        .get(format!("{base}/v1/timeline/customer/cust_linked"))
        .query(&[("org_id", "org_test")])
        .send()
        .await
        .unwrap();
    let after: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(after.len(), 2, "Customer should now see linked session events");
}

#[tokio::test]
async fn discovery_endpoints() {
    let base = start_test_server().await;
    let client = reqwest::Client::new();

    // Ingest some events
    client
        .post(format!("{base}/v1/events"))
        .json(&serde_json::json!({
            "org_id": "org_disc",
            "source": "stripe",
            "topic": "payments",
            "event_type": "charge.created",
            "entities": {"customer": "cust_1"},
        }))
        .send()
        .await
        .unwrap();

    // List entity types
    let resp = client
        .get(format!("{base}/v1/discover/entity-types"))
        .query(&[("org_id", "org_disc")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let types: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(!types.is_empty(), "Should have at least 'customer' entity type");

    // List entities of type
    let resp = client
        .get(format!("{base}/v1/discover/entities/customer"))
        .query(&[("org_id", "org_disc")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let entities: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(!entities.is_empty());
}
