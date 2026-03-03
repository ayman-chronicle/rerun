//! Integration tests for the hybrid backend.
//!
//! Runs the shared trait test suites from `chronicle_test_fixtures`
//! plus hybrid-specific tests for archiver and hot/cold query routing.
//!
//! Requires: Postgres running on localhost:5433, `hybrid` and `postgres` features.

#![cfg(all(feature = "hybrid", feature = "postgres"))]

use chrono::{Duration, Utc};

use chronicle_core::event::EventBuilder;
use chronicle_core::ids::*;
use chronicle_core::query::{OrderBy, StructuredQuery};
use chronicle_core::time_range::TimeRange;
use chronicle_store::hybrid::HybridBackend;
use chronicle_store::traits::{EntityRefStore, EventStore};
use chronicle_test_fixtures::trait_tests;

const TEST_DB_URL: &str = "postgres://chronicle:chronicle@localhost:5433/chronicle";

async fn setup_hybrid(archive_after: Duration) -> (HybridBackend, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let backend = HybridBackend::new(TEST_DB_URL, tmp.path().to_path_buf(), archive_after)
        .await
        .expect("connect to Postgres");
    backend.run_migrations().await.expect("run migrations");

    sqlx::raw_sql(
        "TRUNCATE event_embeddings, event_links, entity_refs, source_schemas, events CASCADE",
    )
    .execute(backend.pg_pool())
    .await
    .expect("clean database");

    (backend, tmp)
}

// ---------------------------------------------------------------------------
// Shared trait test suites (same as PostgresBackend)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn hybrid_passes_event_store_suite() {
    let (backend, _tmp) = setup_hybrid(Duration::days(365)).await;
    trait_tests::run_event_store_tests(&backend).await;
}

#[tokio::test]
async fn hybrid_passes_entity_ref_suite() {
    let (backend, _tmp) = setup_hybrid(Duration::days(365)).await;
    trait_tests::run_entity_ref_tests(&backend, &backend).await;
}

// ---------------------------------------------------------------------------
// Archiver tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn archive_moves_events_to_parquet() {
    let (backend, _tmp) = setup_hybrid(Duration::zero()).await;

    let events: Vec<_> = (0..5)
        .map(|i| {
            EventBuilder::new("org_arch", "stripe", "payments", "payment_intent.succeeded")
                .entity("customer", format!("cust_{i}"))
                .payload(serde_json::json!({"amount": 1000 + i}))
                .event_time(Utc::now() - Duration::hours(1))
                .build()
        })
        .collect();

    backend.insert_events(&events).await.unwrap();

    let count_before = count_pg_events(&backend, "org_arch").await;
    assert_eq!(count_before, 5);

    let report = backend.archive_cold_events().await.unwrap();
    assert_eq!(report.events_archived, 5);
    assert!(report.parquet_files_written >= 1);

    let count_after = count_pg_events(&backend, "org_arch").await;
    assert_eq!(count_after, 0, "archived events should be deleted from Postgres");

    // Events still queryable from Parquet via the cold path.
    let query = StructuredQuery {
        org_id: OrgId::new("org_arch"),
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
    let results = backend.query_cold_structured(&query).await.unwrap();
    assert_eq!(results.len(), 5, "all 5 events should be in Parquet");
}

#[tokio::test]
async fn entity_refs_survive_archival() {
    let (backend, _tmp) = setup_hybrid(Duration::zero()).await;

    let event = EventBuilder::new("org_ref_surv", "stripe", "payments", "charge.created")
        .entity("customer", "cust_persist")
        .event_time(Utc::now() - Duration::hours(2))
        .build();
    let event_id = event.event_id;

    backend.insert_events(&[event]).await.unwrap();
    backend.archive_cold_events().await.unwrap();

    let refs = backend.get_refs_for_event(&event_id).await.unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].entity_type, "customer");
}

// ---------------------------------------------------------------------------
// Hot/cold query routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn query_routes_to_cold_for_old_events() {
    let (backend, _tmp) = setup_hybrid(Duration::hours(1)).await;

    let old_event = EventBuilder::new("org_route", "stripe", "payments", "charge.created")
        .payload(serde_json::json!({"amount": 9999}))
        .event_time(Utc::now() - Duration::hours(2))
        .build();
    let old_id = old_event.event_id;

    backend.insert_events(&[old_event]).await.unwrap();
    backend.archive_cold_events().await.unwrap();

    let result = backend
        .get_event(&OrgId::new("org_route"), &old_id)
        .await
        .unwrap();
    assert!(
        result.is_some(),
        "archived event should be findable via get_event"
    );
}

#[tokio::test]
async fn query_routes_to_hot_for_recent_events() {
    let (backend, _tmp) = setup_hybrid(Duration::hours(24)).await;

    let recent = EventBuilder::new("org_hot", "stripe", "payments", "charge.created")
        .entity("customer", "cust_hot")
        .payload(serde_json::json!({"amount": 5555}))
        .build();
    backend.insert_events(&[recent]).await.unwrap();

    let query = StructuredQuery {
        org_id: OrgId::new("org_hot"),
        source: Some(Source::new("stripe")),
        entity: None,
        topic: None,
        event_type: None,
        time_range: Some(TimeRange::last_hours(1)),
        payload_filters: vec![],
        group_by: None,
        order_by: OrderBy::EventTimeDesc,
        limit: 100,
        offset: 0,
    };

    let results = backend.query_structured(&query).await.unwrap();
    assert!(!results.is_empty(), "recent event should come from hot Postgres");
}

#[tokio::test]
async fn query_fans_out_across_hot_and_cold() {
    let (backend, _tmp) = setup_hybrid(Duration::hours(1)).await;

    let old = EventBuilder::new("org_fanout", "stripe", "payments", "charge.created")
        .payload(serde_json::json!({"age": "old"}))
        .event_time(Utc::now() - Duration::hours(3))
        .build();

    let recent = EventBuilder::new("org_fanout", "stripe", "payments", "charge.created")
        .payload(serde_json::json!({"age": "new"}))
        .build();

    backend.insert_events(&[old, recent]).await.unwrap();
    backend.archive_cold_events().await.unwrap();

    let query = StructuredQuery {
        org_id: OrgId::new("org_fanout"),
        source: None,
        entity: None,
        topic: None,
        event_type: None,
        time_range: None,
        payload_filters: vec![],
        group_by: None,
        order_by: OrderBy::EventTimeAsc,
        limit: 100,
        offset: 0,
    };

    let results = backend.query_structured(&query).await.unwrap();
    assert_eq!(results.len(), 2, "should find 1 hot + 1 cold event");
}

// ---------------------------------------------------------------------------
// Parquet I/O round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn parquet_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.parquet");

    let events: Vec<_> = (0..10)
        .map(|i| {
            EventBuilder::new("org_pq", "s", "t", "e")
                .payload(serde_json::json!({"i": i}))
                .build()
        })
        .collect();

    let batch = chronicle_store::hybrid::parquet_io::events_to_record_batch(&events).unwrap();
    let file = std::fs::File::create(&path).unwrap();

    use datafusion::parquet::arrow::ArrowWriter;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();

    let ctx = datafusion::prelude::SessionContext::new();
    ctx.register_parquet("events", path.to_str().unwrap(), Default::default())
        .await
        .unwrap();

    let df = ctx.sql("SELECT * FROM events").await.unwrap();
    let batches = df.collect().await.unwrap();

    let results = chronicle_store::hybrid::parquet_io::batches_to_event_results(&batches);
    assert_eq!(results.len(), 10);
    assert_eq!(results[0].event.org_id, OrgId::new("org_pq"));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn count_pg_events(backend: &HybridBackend, org_id: &str) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM events WHERE org_id = $1")
        .bind(org_id)
        .fetch_one(backend.pg_pool())
        .await
        .unwrap();
    row.0
}
