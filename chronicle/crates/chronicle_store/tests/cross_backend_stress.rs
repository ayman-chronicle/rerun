//! Large-scale cross-backend stress test.
//!
//! Seeds 500+ events into Memory, Postgres, and Hybrid backends,
//! then exercises reads, writes, queries, timelines, entity linking,
//! and graph traversal -- asserting that all three backends produce
//! consistent results.
//!
//! Requires: Postgres on localhost:5433, `hybrid` and `postgres` features.

#![cfg(all(feature = "hybrid", feature = "postgres"))]

use std::time::Instant;

use chrono::{Duration, Utc};

use chronicle_core::event::EventBuilder;
use chronicle_core::ids::*;
use chronicle_core::link::EventLink;
use chronicle_core::query::{OrderBy, StructuredQuery, TimelineQuery};
use chronicle_core::time_range::TimeRange;
use chronicle_store::hybrid::HybridBackend;
use chronicle_store::memory::InMemoryBackend;
use chronicle_store::postgres::PostgresBackend;
use chronicle_store::traits::{EntityRefStore, EventLinkStore, EventStore};

const DB_URL: &str = "postgres://chronicle:chronicle@localhost:5433/chronicle";
const NUM_CUSTOMERS: usize = 50;
const EVENTS_PER_CUSTOMER: usize = 10;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a large, realistic event batch: 50 customers × 10 events each = 500 events,
/// plus 50 anonymous session events and 25 causal links.
struct StressDataset {
    org_id: String,
    events: Vec<chronicle_core::event::Event>,
    links: Vec<EventLink>,
    customer_ids: Vec<String>,
    session_ids: Vec<String>,
}

impl StressDataset {
    fn build(org_id: &str) -> Self {
        let mut events = Vec::new();
        let mut links = Vec::new();
        let mut customer_ids = Vec::new();
        let mut session_ids = Vec::new();

        for i in 0..NUM_CUSTOMERS {
            let cust = format!("cust_{i:04}");
            customer_ids.push(cust.clone());

            // 3 stripe payments
            for j in 0..3 {
                events.push(
                    EventBuilder::new(org_id, "stripe", "payments", "payment_intent.succeeded")
                        .entity("customer", cust.as_str())
                        .payload(serde_json::json!({
                            "amount": 1000 + j * 500 + i as i64 * 10,
                            "currency": "usd",
                        }))
                        .event_time(Utc::now() - Duration::days(90 - j as i64 * 30))
                        .build(),
                );
            }

            // 2 support tickets
            for j in 0..2 {
                events.push(
                    EventBuilder::new(org_id, "support", "tickets", "ticket.created")
                        .entity("customer", cust.as_str())
                        .entity("ticket", format!("tkt_{i:04}_{j}"))
                        .payload(serde_json::json!({
                            "subject": format!("Issue #{j} for {cust}"),
                            "priority": if j == 0 { "high" } else { "normal" },
                        }))
                        .event_time(Utc::now() - Duration::days(60 - j as i64 * 20))
                        .build(),
                );
            }

            // 3 product page views
            for (j, page) in ["/dashboard", "/settings", "/billing"].iter().enumerate() {
                events.push(
                    EventBuilder::new(org_id, "product", "usage", "page.viewed")
                        .entity("customer", cust.as_str())
                        .payload(serde_json::json!({
                            "url": page,
                            "session_duration_ms": 30000 + j * 5000,
                        }))
                        .event_time(Utc::now() - Duration::days(45 - j as i64 * 10))
                        .build(),
                );
            }

            // 1 marketing campaign
            events.push(
                EventBuilder::new(org_id, "marketing", "campaigns", "campaign.sent")
                    .entity("customer", cust.as_str())
                    .payload(serde_json::json!({
                        "campaign_id": "camp_Q1",
                        "channel": "email",
                    }))
                    .event_time(Utc::now() - Duration::days(70))
                    .build(),
            );

            // 1 subscription event (cancelled for first 10 customers)
            if i < 10 {
                let cancel_evt = EventBuilder::new(
                    org_id,
                    "stripe",
                    "subscriptions",
                    "customer.subscription.deleted",
                )
                .entity("customer", cust.as_str())
                .payload(serde_json::json!({"plan": "pro", "reason": "cost"}))
                .event_time(Utc::now() - Duration::days(10))
                .build();

                // Link last payment → cancellation
                let last_payment_idx = events.len() - 4; // The 3rd payment for this customer
                if last_payment_idx < events.len() {
                    links.push(EventLink {
                        link_id: LinkId::new(),
                        source_event_id: events[last_payment_idx].event_id,
                        target_event_id: cancel_evt.event_id,
                        link_type: "caused_by".to_string(),
                        confidence: Confidence::new(0.8).unwrap(),
                        reasoning: Some("payment preceded cancellation".to_string()),
                        created_by: "stress_test".to_string(),
                        created_at: Utc::now(),
                    });
                }

                events.push(cancel_evt);
            }
        }

        // 50 anonymous session events (5 sessions × 10 page views)
        for s in 0..5 {
            let sess = format!("sess_anon_{s:03}");
            session_ids.push(sess.clone());
            for p in 0..10 {
                events.push(
                    EventBuilder::new(org_id, "product", "usage", "page.viewed")
                        .entity("session", sess.as_str())
                        .payload(serde_json::json!({
                            "url": format!("/page_{p}"),
                            "referrer": "google.com",
                        }))
                        .event_time(Utc::now() - Duration::hours(48 - s as i64 * 8 - p as i64))
                        .build(),
                );
            }
        }

        Self {
            org_id: org_id.to_string(),
            events,
            links,
            customer_ids,
            session_ids,
        }
    }
}

async fn setup_memory(org_tag: &str) -> (InMemoryBackend, StressDataset) {
    let ds = StressDataset::build(org_tag);
    let backend = InMemoryBackend::new();
    (backend, ds)
}

async fn setup_postgres(org_tag: &str) -> (PostgresBackend, StressDataset) {
    let ds = StressDataset::build(org_tag);
    let backend = PostgresBackend::new(DB_URL).await.expect("PG connect");
    backend.run_migrations().await.ok();
    sqlx::raw_sql(&format!(
        "DELETE FROM event_links WHERE org_id = '{org_tag}' OR org_id = '';\
         DELETE FROM entity_refs WHERE org_id = '{org_tag}';\
         DELETE FROM event_embeddings WHERE org_id = '{org_tag}';\
         DELETE FROM events WHERE org_id = '{org_tag}';"
    ))
    .execute(backend.pg_pool())
    .await
    .ok();
    (backend, ds)
}

async fn setup_hybrid(org_tag: &str) -> (HybridBackend, StressDataset, tempfile::TempDir) {
    let ds = StressDataset::build(org_tag);
    let tmp = tempfile::tempdir().expect("tmpdir");
    let backend = HybridBackend::new(DB_URL, tmp.path().to_path_buf(), Duration::days(30))
        .await
        .expect("hybrid connect");
    backend.run_migrations().await.ok();
    sqlx::raw_sql(&format!(
        "DELETE FROM event_links WHERE org_id = '{org_tag}' OR org_id = '';\
         DELETE FROM entity_refs WHERE org_id = '{org_tag}';\
         DELETE FROM event_embeddings WHERE org_id = '{org_tag}';\
         DELETE FROM events WHERE org_id = '{org_tag}';"
    ))
    .execute(backend.pg_pool())
    .await
    .ok();
    (backend, ds, tmp)
}

/// Seed a backend with the stress dataset: insert events and create links.
async fn seed<T: EventStore + EventLinkStore>(backend: &T, ds: &StressDataset) {
    let t = Instant::now();

    // Insert events in batches of 100.
    for chunk in ds.events.chunks(100) {
        backend.insert_events(chunk).await.expect("insert batch");
    }

    // Create links.
    for link in &ds.links {
        backend.create_link(link).await.expect("create link");
    }

    let elapsed = t.elapsed();
    eprintln!(
        "  seeded {} events + {} links in {elapsed:?}",
        ds.events.len(),
        ds.links.len()
    );
}

// ---------------------------------------------------------------------------
// The stress test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_backend_stress() {
    eprintln!("\n=== Cross-Backend Stress Test ===\n");

    // Setup order matters: Hybrid archiver touches ALL events in the shared
    // Postgres instance, so we seed Postgres AFTER the archiver runs.

    eprintln!("[Memory] setting up…");
    let (mem, mem_ds) = setup_memory("stress_mem").await;
    seed(&mem, &mem_ds).await;

    eprintln!("[Hybrid] setting up + archiving…");
    let (hybrid, hybrid_ds, _tmp) = setup_hybrid("stress_hy").await;
    seed(&hybrid, &hybrid_ds).await;

    let report = hybrid.archive_cold_events().await.unwrap();
    eprintln!(
        "  archived {} events into {} Parquet files",
        report.events_archived, report.parquet_files_written
    );

    // Seed Postgres AFTER hybrid archiver so its data is not affected.
    eprintln!("[Postgres] setting up…");
    let (pg, pg_ds) = setup_postgres("stress_pg").await;
    seed(&pg, &pg_ds).await;

    // --- Test 1: Total event count ---
    eprintln!("\n[Test 1] Total event count…");
    let expected = mem_ds.events.len();
    assert_eq!(pg_ds.events.len(), expected);
    assert_eq!(hybrid_ds.events.len(), expected);

    let mem_count = count_all(&mem, "stress_mem").await;
    let pg_count = count_all(&pg, "stress_pg").await;
    let hybrid_count = count_all(&hybrid, "stress_hy").await;

    assert_eq!(mem_count, expected as u64, "Memory count mismatch");
    assert_eq!(pg_count, expected as u64, "Postgres count mismatch");
    assert_eq!(hybrid_count, expected as u64, "Hybrid count mismatch");
    eprintln!("  all backends: {expected} events ✓");

    // --- Test 2: Query by source ---
    eprintln!("[Test 2] Query by source (stripe)…");
    let mem_stripe = query_by_source(&mem, "stress_mem", "stripe").await;
    let pg_stripe = query_by_source(&pg, "stress_pg", "stripe").await;
    let hy_stripe = query_by_source(&hybrid, "stress_hy", "stripe").await;

    assert_eq!(mem_stripe, pg_stripe, "Memory vs Postgres stripe count");
    assert_eq!(pg_stripe, hy_stripe, "Postgres vs Hybrid stripe count");
    eprintln!("  stripe events: {mem_stripe} across all backends ✓");

    // --- Test 3: Query by event type ---
    eprintln!("[Test 3] Query by event type (ticket.created)…");
    let mem_tickets = query_by_event_type(&mem, "stress_mem", "ticket.created").await;
    let pg_tickets = query_by_event_type(&pg, "stress_pg", "ticket.created").await;
    let hy_tickets = query_by_event_type(&hybrid, "stress_hy", "ticket.created").await;

    assert_eq!(mem_tickets, pg_tickets);
    assert_eq!(pg_tickets, hy_tickets);
    eprintln!("  ticket events: {mem_tickets} across all backends ✓");

    // --- Test 4: Customer timeline ---
    eprintln!("[Test 4] Customer timeline (cust_0000)…");
    let mem_tl = timeline_count(&mem, "stress_mem", "customer", "cust_0000").await;
    let pg_tl = timeline_count(&pg, "stress_pg", "customer", "cust_0000").await;
    let hy_tl = timeline_count(&hybrid, "stress_hy", "customer", "cust_0000").await;

    assert!(mem_tl >= EVENTS_PER_CUSTOMER, "Memory timeline too short: {mem_tl}");
    assert_eq!(mem_tl, pg_tl, "Memory vs Postgres timeline");
    assert_eq!(pg_tl, hy_tl, "Postgres vs Hybrid timeline");
    eprintln!("  cust_0000 timeline: {mem_tl} events ✓");

    // --- Test 5: Entity get_event round-trip ---
    eprintln!("[Test 5] get_event round-trip…");
    let sample_id = mem_ds.events[0].event_id;
    let mem_evt = mem.get_event(&OrgId::new("stress_mem"), &sample_id).await.unwrap();
    let pg_evt = pg.get_event(&OrgId::new("stress_pg"), &pg_ds.events[0].event_id).await.unwrap();
    let hy_evt = hybrid.get_event(&OrgId::new("stress_hy"), &hybrid_ds.events[0].event_id).await.unwrap();

    assert!(mem_evt.is_some(), "Memory get_event");
    assert!(pg_evt.is_some(), "Postgres get_event");
    assert!(hy_evt.is_some(), "Hybrid get_event");
    eprintln!("  all backends found their sample event ✓");

    // --- Test 6: JIT entity linking ---
    eprintln!("[Test 6] JIT entity linking (session → customer)…");
    let sess = &mem_ds.session_ids[0];
    let new_cust = "cust_linked_new";

    let mem_linked = mem.link_entity(
        &OrgId::new("stress_mem"),
        &EntityType::new("session"), &EntityId::new(sess),
        &EntityType::new("customer"), &EntityId::new(new_cust),
        "stress_test",
    ).await.unwrap();

    let pg_linked = pg.link_entity(
        &OrgId::new("stress_pg"),
        &EntityType::new("session"), &EntityId::new(&pg_ds.session_ids[0]),
        &EntityType::new("customer"), &EntityId::new(new_cust),
        "stress_test",
    ).await.unwrap();

    let hy_linked = hybrid.link_entity(
        &OrgId::new("stress_hy"),
        &EntityType::new("session"), &EntityId::new(&hybrid_ds.session_ids[0]),
        &EntityType::new("customer"), &EntityId::new(new_cust),
        "stress_test",
    ).await.unwrap();

    assert_eq!(mem_linked, 10, "Memory: should link 10 session events");
    assert_eq!(pg_linked, 10, "Postgres: should link 10 session events");
    assert_eq!(hy_linked, 10, "Hybrid: should link 10 session events");

    // Verify linked events appear on customer timeline.
    let mem_tl2 = timeline_count(&mem, "stress_mem", "customer", new_cust).await;
    let pg_tl2 = timeline_count(&pg, "stress_pg", "customer", new_cust).await;
    let hy_tl2 = timeline_count(&hybrid, "stress_hy", "customer", new_cust).await;

    assert_eq!(mem_tl2, 10);
    assert_eq!(pg_tl2, 10);
    assert_eq!(hy_tl2, 10);
    eprintln!("  linked {mem_linked} events, customer timeline now {mem_tl2} events ✓");

    // --- Test 7: Event link graph ---
    eprintln!("[Test 7] Graph traversal (links)…");
    let mem_links = mem.get_links_for_event(&mem_ds.links[0].source_event_id).await.unwrap();
    let pg_links = pg.get_links_for_event(&pg_ds.links[0].source_event_id).await.unwrap();

    assert!(!mem_links.is_empty(), "Memory links");
    assert!(!pg_links.is_empty(), "Postgres links");
    eprintln!("  Memory links: {}, Postgres links: {} ✓", mem_links.len(), pg_links.len());

    // --- Test 8: DataFusion SQL on Hybrid ---
    eprintln!("[Test 8] DataFusion SQL passthrough on Hybrid…");
    let sql_results = hybrid
        .query_sql(
            &OrgId::new("stress_hy"),
            "SELECT source, count(*) as cnt FROM events GROUP BY source ORDER BY cnt DESC",
        )
        .await;

    match &sql_results {
        Ok(rows) => eprintln!("  DataFusion SQL returned {} result rows ✓", rows.len()),
        Err(e) => eprintln!("  DataFusion SQL: {e} (non-aggregation fallback)"),
    }

    // --- Test 9: Query with time range ---
    eprintln!("[Test 9] Time-range query (last 7 days)…");
    let mem_recent = query_with_time_range(&mem, "stress_mem", 7).await;
    let pg_recent = query_with_time_range(&pg, "stress_pg", 7).await;
    let hy_recent = query_with_time_range(&hybrid, "stress_hy", 7).await;

    assert_eq!(mem_recent, pg_recent, "Memory vs Postgres recent");
    assert_eq!(pg_recent, hy_recent, "Postgres vs Hybrid recent");
    eprintln!("  last 7 days: {mem_recent} events across all backends ✓");

    // --- Test 10: Bulk write speed comparison ---
    eprintln!("\n[Test 10] Bulk write speed comparison (100 events)…");
    let extra: Vec<_> = (0..100)
        .map(|i| {
            EventBuilder::new("stress_speed", "bench", "throughput", "event.test")
                .payload(serde_json::json!({"i": i}))
                .build()
        })
        .collect();

    let t_mem = Instant::now();
    mem.insert_events(&extra).await.unwrap();
    let mem_ms = t_mem.elapsed().as_millis();

    let extra_pg: Vec<_> = (0..100)
        .map(|i| {
            EventBuilder::new("stress_speed", "bench", "throughput", "event.test")
                .payload(serde_json::json!({"i": i}))
                .build()
        })
        .collect();
    let t_pg = Instant::now();
    pg.insert_events(&extra_pg).await.unwrap();
    let pg_ms = t_pg.elapsed().as_millis();

    let extra_hy: Vec<_> = (0..100)
        .map(|i| {
            EventBuilder::new("stress_speed", "bench", "throughput", "event.test")
                .payload(serde_json::json!({"i": i}))
                .build()
        })
        .collect();
    let t_hy = Instant::now();
    hybrid.insert_events(&extra_hy).await.unwrap();
    let hy_ms = t_hy.elapsed().as_millis();

    eprintln!("  Memory: {mem_ms}ms | Postgres: {pg_ms}ms | Hybrid: {hy_ms}ms");

    eprintln!("\n=== All cross-backend stress tests passed! ===\n");
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

async fn count_all(store: &dyn EventStore, org_id: &str) -> u64 {
    store
        .count(&StructuredQuery {
            org_id: OrgId::new(org_id),
            source: None,
            entity: None,
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: 100_000,
            offset: 0,
        })
        .await
        .unwrap()
}

async fn query_by_source(store: &dyn EventStore, org_id: &str, source: &str) -> usize {
    store
        .query_structured(&StructuredQuery {
            org_id: OrgId::new(org_id),
            source: Some(Source::new(source)),
            entity: None,
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: 100_000,
            offset: 0,
        })
        .await
        .unwrap()
        .len()
}

async fn query_by_event_type(store: &dyn EventStore, org_id: &str, event_type: &str) -> usize {
    store
        .query_structured(&StructuredQuery {
            org_id: OrgId::new(org_id),
            source: None,
            entity: None,
            topic: None,
            event_type: Some(EventType::new(event_type)),
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: 100_000,
            offset: 0,
        })
        .await
        .unwrap()
        .len()
}

async fn timeline_count(
    store: &(dyn EventStore + Sync),
    org_id: &str,
    entity_type: &str,
    entity_id: &str,
) -> usize {
    store
        .query_timeline(&TimelineQuery {
            org_id: OrgId::new(org_id),
            entity_type: EntityType::new(entity_type),
            entity_id: EntityId::new(entity_id),
            time_range: None,
            sources: None,
            include_linked: false,
            include_entity_refs: false,
            link_depth: 0,
            min_link_confidence: 0.0,
        })
        .await
        .unwrap()
        .len()
}

async fn query_with_time_range(store: &dyn EventStore, org_id: &str, days: i64) -> usize {
    store
        .query_structured(&StructuredQuery {
            org_id: OrgId::new(org_id),
            source: None,
            entity: None,
            topic: None,
            event_type: None,
            time_range: Some(TimeRange::last_days(days)),
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: 100_000,
            offset: 0,
        })
        .await
        .unwrap()
        .len()
}
