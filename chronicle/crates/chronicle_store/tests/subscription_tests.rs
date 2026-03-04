//! Integration tests for the InMemory subscription service.

use std::sync::{Arc, atomic::{AtomicU32, Ordering}};

use chronicle_core::error::StoreError;
use chronicle_core::event::{Event, EventBuilder};
use chronicle_core::ids::*;
use chronicle_store::memory::InMemoryBackend;
use chronicle_store::subscriptions::*;
use chronicle_store::traits::EventStore;

/// Simple handler that counts events received.
struct CountHandler {
    count: Arc<AtomicU32>,
}

impl CountHandler {
    fn new() -> (Self, Arc<AtomicU32>) {
        let count = Arc::new(AtomicU32::new(0));
        (Self { count: count.clone() }, count)
    }
}

#[async_trait::async_trait]
impl EventHandler for CountHandler {
    async fn handle(&self, _event: &Event) -> Result<(), StoreError> {
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// Handler that collects event types.
struct CollectHandler {
    types: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl CollectHandler {
    fn new() -> (Self, Arc<parking_lot::Mutex<Vec<String>>>) {
        let types = Arc::new(parking_lot::Mutex::new(Vec::new()));
        (Self { types: types.clone() }, types)
    }
}

#[async_trait::async_trait]
impl EventHandler for CollectHandler {
    async fn handle(&self, event: &Event) -> Result<(), StoreError> {
        self.types.lock().push(event.event_type.as_str().to_owned());
        Ok(())
    }
}

fn stripe_event(org: &str, cust: &str, etype: &str) -> Event {
    EventBuilder::new(org, "stripe", "payments", etype)
        .entity("customer", cust)
        .payload(serde_json::json!({"amount": 4999}))
        .build()
}

fn support_event(org: &str, cust: &str) -> Event {
    EventBuilder::new(org, "support", "tickets", "ticket.created")
        .entity("customer", cust)
        .payload(serde_json::json!({"subject": "Help"}))
        .build()
}

#[tokio::test]
async fn subscribe_receives_matching_events() {
    let backend = InMemoryBackend::new();
    let (handler, count) = CountHandler::new();

    let _handle = backend
        .subscribe(SubFilter::default(), SubscriptionPosition::End, Arc::new(handler))
        .await
        .unwrap();

    backend
        .insert_events(&[
            stripe_event("org", "c1", "charge.created"),
            stripe_event("org", "c2", "charge.created"),
        ])
        .await
        .unwrap();

    assert_eq!(count.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn subscribe_filters_by_source() {
    let backend = InMemoryBackend::new();
    let (handler, count) = CountHandler::new();

    let filter = SubFilter {
        sources: Some(vec![Source::new("stripe")]),
        ..Default::default()
    };

    let _handle = backend
        .subscribe(filter, SubscriptionPosition::End, Arc::new(handler))
        .await
        .unwrap();

    backend
        .insert_events(&[
            stripe_event("org", "c1", "charge.created"),
            support_event("org", "c1"),
        ])
        .await
        .unwrap();

    assert_eq!(count.load(Ordering::Relaxed), 1, "only stripe should match");
}

#[tokio::test]
async fn subscribe_filters_by_entity() {
    let backend = InMemoryBackend::new();
    let (handler, types) = CollectHandler::new();

    let filter = SubFilter {
        entity: Some((EntityType::new("customer"), EntityId::new("cust_042"))),
        ..Default::default()
    };

    let _handle = backend
        .subscribe(filter, SubscriptionPosition::End, Arc::new(handler))
        .await
        .unwrap();

    backend
        .insert_events(&[
            stripe_event("org", "cust_042", "charge.created"),
            stripe_event("org", "cust_999", "charge.refunded"),
            support_event("org", "cust_042"),
        ])
        .await
        .unwrap();

    let received = types.lock().clone();
    assert_eq!(received.len(), 2);
    assert!(received.contains(&"charge.created".to_owned()));
    assert!(received.contains(&"ticket.created".to_owned()));
}

#[tokio::test]
async fn subscribe_filters_by_payload_contains() {
    let backend = InMemoryBackend::new();
    let (handler, count) = CountHandler::new();

    let filter = SubFilter {
        payload_contains: Some("card_declined".into()),
        ..Default::default()
    };

    let _handle = backend
        .subscribe(filter, SubscriptionPosition::End, Arc::new(handler))
        .await
        .unwrap();

    backend
        .insert_events(&[
            EventBuilder::new("org", "stripe", "payments", "payment.failed")
                .payload(serde_json::json!({"failure_code": "card_declined"}))
                .build(),
            EventBuilder::new("org", "stripe", "payments", "payment.succeeded")
                .payload(serde_json::json!({"status": "ok"}))
                .build(),
        ])
        .await
        .unwrap();

    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn multiple_concurrent_subscriptions() {
    let backend = InMemoryBackend::new();
    let (h1, c1) = CountHandler::new();
    let (h2, c2) = CountHandler::new();

    let f1 = SubFilter { sources: Some(vec![Source::new("stripe")]), ..Default::default() };
    let f2 = SubFilter { sources: Some(vec![Source::new("support")]), ..Default::default() };

    let _handle1 = backend.subscribe(f1, SubscriptionPosition::End, Arc::new(h1)).await.unwrap();
    let _handle2 = backend.subscribe(f2, SubscriptionPosition::End, Arc::new(h2)).await.unwrap();

    assert_eq!(backend.subscription_count(), 2);

    backend
        .insert_events(&[
            stripe_event("org", "c1", "charge.created"),
            support_event("org", "c1"),
            stripe_event("org", "c2", "charge.refunded"),
        ])
        .await
        .unwrap();

    assert_eq!(c1.load(Ordering::Relaxed), 2, "stripe handler sees 2 stripe events");
    assert_eq!(c2.load(Ordering::Relaxed), 1, "support handler sees 1 support event");
}

#[tokio::test]
async fn cancel_stops_delivery() {
    let backend = InMemoryBackend::new();
    let (handler, count) = CountHandler::new();

    let handle = backend
        .subscribe(SubFilter::default(), SubscriptionPosition::End, Arc::new(handler))
        .await
        .unwrap();

    backend.insert_events(&[stripe_event("org", "c1", "charge.created")]).await.unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 1);

    handle.cancel();
    tokio::task::yield_now().await;

    backend.insert_events(&[stripe_event("org", "c2", "charge.created")]).await.unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 1, "no new events after cancel");
}
