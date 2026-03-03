//! Large-scale performance benchmark across all four backends.
//!
//! Measures write throughput, query latency, timeline speed, entity link
//! propagation, count aggregation, and (for Hybrid) archive throughput.
//!
//! This is NOT a micro-benchmark -- it's a realistic workload that helps
//! determine whether the architecture is genuinely fast.
//!
//! Requires: Postgres :5433, KurrentDB :2113, `hybrid` + `kurrent` features.

#![cfg(all(feature = "hybrid", feature = "kurrent", feature = "postgres"))]

use std::time::Instant;

use chrono::{Duration, Utc};

use chronicle_core::event::EventBuilder;
use chronicle_core::ids::*;
use chronicle_core::query::{OrderBy, StructuredQuery, TimelineQuery};
use chronicle_store::hybrid::HybridBackend;
use chronicle_store::kurrent::KurrentBackend;
use chronicle_store::memory::InMemoryBackend;
use chronicle_store::postgres::PostgresBackend;
use chronicle_store::traits::{EntityRefStore, EventStore};

const PG_URL: &str = "postgres://chronicle:chronicle@localhost:5433/chronicle";
const KURRENT_URL: &str = "kurrentdb://localhost:2113?tls=false";

const BULK_SIZE: usize = 10_000;
const BATCH_SIZE: usize = 500;
const NUM_CUSTOMERS: usize = 200;
const QUERY_LIMIT: usize = 100_000;

// ---------------------------------------------------------------------------
// Event generators
// ---------------------------------------------------------------------------

fn generate_events(org_id: &str, count: usize) -> Vec<chronicle_core::event::Event> {
    (0..count)
        .map(|i| {
            let cust = format!("cust_{:04}", i % NUM_CUSTOMERS);
            let source = match i % 4 {
                0 => "stripe",
                1 => "support",
                2 => "product",
                _ => "marketing",
            };
            let event_type = match i % 4 {
                0 => "payment_intent.succeeded",
                1 => "ticket.created",
                2 => "page.viewed",
                _ => "campaign.sent",
            };
            let topic = match i % 4 {
                0 => "payments",
                1 => "tickets",
                2 => "usage",
                _ => "campaigns",
            };

            EventBuilder::new(org_id, source, topic, event_type)
                .entity("customer", cust.as_str())
                .payload(serde_json::json!({
                    "amount": 1000 + i as i64,
                    "index": i,
                }))
                .event_time(Utc::now() - Duration::hours((count - i) as i64))
                .build()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Benchmark harness
// ---------------------------------------------------------------------------

struct BenchResult {
    backend: String,
    write_total_ms: u128,
    write_events_per_sec: f64,
    query_by_source_ms: u128,
    query_by_source_count: usize,
    query_by_entity_ms: u128,
    query_by_entity_count: usize,
    timeline_ms: u128,
    timeline_count: usize,
    count_ms: u128,
    count_total: u64,
    entity_link_ms: u128,
    entity_link_count: u64,
}

impl BenchResult {
    fn print(&self) {
        eprintln!("  ┌─ {}", self.backend);
        eprintln!("  │ Write:     {:>6}ms  ({:.0} events/sec)", self.write_total_ms, self.write_events_per_sec);
        eprintln!("  │ Q source:  {:>6}ms  ({} results)", self.query_by_source_ms, self.query_by_source_count);
        eprintln!("  │ Q entity:  {:>6}ms  ({} results)", self.query_by_entity_ms, self.query_by_entity_count);
        eprintln!("  │ Timeline:  {:>6}ms  ({} events)", self.timeline_ms, self.timeline_count);
        eprintln!("  │ Count:     {:>6}ms  ({} total)", self.count_ms, self.count_total);
        eprintln!("  │ Link:      {:>6}ms  ({} linked)", self.entity_link_ms, self.entity_link_count);
        eprintln!("  └─");
    }
}

async fn bench_backend(
    store: &(dyn EventStore + Sync),
    ref_store: &(dyn EntityRefStore + Sync),
    name: &str,
    org_id: &str,
    events: &[chronicle_core::event::Event],
) -> BenchResult {
    // --- Write ---
    let t = Instant::now();
    for chunk in events.chunks(BATCH_SIZE) {
        store.insert_events(chunk).await.unwrap();
    }
    let write_ms = t.elapsed().as_millis();
    let write_eps = events.len() as f64 / (write_ms as f64 / 1000.0);

    // --- Query by source ---
    let t = Instant::now();
    let source_results = store
        .query_structured(&StructuredQuery {
            org_id: OrgId::new(org_id),
            source: Some(Source::new("stripe")),
            entity: None,
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: QUERY_LIMIT,
            offset: 0,
        })
        .await
        .unwrap();
    let q_source_ms = t.elapsed().as_millis();

    // --- Query by entity ---
    let t = Instant::now();
    let entity_results = store
        .query_structured(&StructuredQuery {
            org_id: OrgId::new(org_id),
            source: None,
            entity: Some((EntityType::new("customer"), EntityId::new("cust_0000"))),
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: QUERY_LIMIT,
            offset: 0,
        })
        .await
        .unwrap();
    let q_entity_ms = t.elapsed().as_millis();

    // --- Timeline ---
    let t = Instant::now();
    let timeline = store
        .query_timeline(&TimelineQuery {
            org_id: OrgId::new(org_id),
            entity_type: EntityType::new("customer"),
            entity_id: EntityId::new("cust_0050"),
            time_range: None,
            sources: None,
            include_linked: false,
            include_entity_refs: false,
            link_depth: 0,
            min_link_confidence: 0.0,
        })
        .await
        .unwrap();
    let tl_ms = t.elapsed().as_millis();

    // --- Count ---
    let t = Instant::now();
    let total = store
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
            limit: QUERY_LIMIT,
            offset: 0,
        })
        .await
        .unwrap();
    let count_ms = t.elapsed().as_millis();

    // --- Entity link propagation ---
    let t = Instant::now();
    let linked = ref_store
        .link_entity(
            &OrgId::new(org_id),
            &EntityType::new("customer"),
            &EntityId::new("cust_0001"),
            &EntityType::new("account"),
            &EntityId::new("acc_bench"),
            "benchmark",
        )
        .await
        .unwrap();
    let link_ms = t.elapsed().as_millis();

    BenchResult {
        backend: name.to_string(),
        write_total_ms: write_ms,
        write_events_per_sec: write_eps,
        query_by_source_ms: q_source_ms,
        query_by_source_count: source_results.len(),
        query_by_entity_ms: q_entity_ms,
        query_by_entity_count: entity_results.len(),
        timeline_ms: tl_ms,
        timeline_count: timeline.len(),
        count_ms: count_ms,
        count_total: total,
        entity_link_ms: link_ms,
        entity_link_count: linked,
    }
}

// ---------------------------------------------------------------------------
// The benchmark
// ---------------------------------------------------------------------------

#[tokio::test]
async fn benchmark_all_backends() {
    eprintln!("\n{}", "=".repeat(60));
    eprintln!("  CHRONICLE BACKEND BENCHMARK");
    eprintln!("  {BULK_SIZE} events, {NUM_CUSTOMERS} customers, batch size {BATCH_SIZE}");
    eprintln!("{}\n", "=".repeat(60));

    // --- Memory ---
    eprintln!("[1/4] Memory backend…");
    let mem = InMemoryBackend::new();
    let mem_events = generate_events("bench_mem", BULK_SIZE);
    let mem_result = bench_backend(&mem, &mem, "Memory", "bench_mem", &mem_events).await;
    mem_result.print();

    // --- Postgres ---
    eprintln!("[2/4] Postgres backend…");
    let pg = PostgresBackend::new(PG_URL).await.unwrap();
    pg.run_migrations().await.ok();
    sqlx::raw_sql(
        "DELETE FROM event_links WHERE org_id LIKE 'bench_%';\
         DELETE FROM entity_refs WHERE org_id LIKE 'bench_%';\
         DELETE FROM event_embeddings WHERE org_id LIKE 'bench_%';\
         DELETE FROM events WHERE org_id LIKE 'bench_%';",
    )
    .execute(pg.pg_pool())
    .await
    .ok();
    let pg_events = generate_events("bench_pg", BULK_SIZE);
    let pg_result = bench_backend(&pg, &pg, "Postgres", "bench_pg", &pg_events).await;
    pg_result.print();

    // --- Kurrent ---
    eprintln!("[3/4] Kurrent backend…");
    let kurrent = KurrentBackend::new(KURRENT_URL, PG_URL).await.unwrap();
    kurrent.run_migrations().await.ok();
    sqlx::raw_sql(
        "DELETE FROM event_links WHERE org_id = 'bench_ku';\
         DELETE FROM entity_refs WHERE org_id = 'bench_ku';\
         DELETE FROM event_embeddings WHERE org_id = 'bench_ku';\
         DELETE FROM events WHERE org_id = 'bench_ku';",
    )
    .execute(kurrent.pg_pool())
    .await
    .ok();
    let ku_events = generate_events("bench_ku", BULK_SIZE);
    let ku_result =
        bench_backend(&kurrent, &kurrent, "Kurrent", "bench_ku", &ku_events).await;
    ku_result.print();

    // --- Hybrid ---
    eprintln!("[4/4] Hybrid backend…");
    let tmp = tempfile::tempdir().unwrap();
    let hybrid = HybridBackend::new(PG_URL, tmp.path().to_path_buf(), Duration::hours(5000))
        .await
        .unwrap();
    hybrid.run_migrations().await.ok();
    sqlx::raw_sql(
        "DELETE FROM event_links WHERE org_id = 'bench_hy';\
         DELETE FROM entity_refs WHERE org_id = 'bench_hy';\
         DELETE FROM event_embeddings WHERE org_id = 'bench_hy';\
         DELETE FROM events WHERE org_id = 'bench_hy';",
    )
    .execute(hybrid.pg_pool())
    .await
    .ok();
    let hy_events = generate_events("bench_hy", BULK_SIZE);
    let hy_result =
        bench_backend(&hybrid, &hybrid, "Hybrid (hot)", "bench_hy", &hy_events).await;
    hy_result.print();

    // --- Hybrid: archive and re-query from cold ---
    eprintln!("[Hybrid] Archiving to Parquet…");
    // Set archive_after to 0 so all events are cold.
    let hybrid_cold = HybridBackend::from_postgres(
        PostgresBackend::from_pool(sqlx::PgPool::connect(PG_URL).await.unwrap()),
        tmp.path().to_path_buf(),
        Duration::zero(),
    );
    // Need to run migration 002 for FK drop.
    hybrid_cold.run_migrations().await.ok();

    let t = Instant::now();
    let report = hybrid_cold.archive_cold_events().await.unwrap();
    let archive_ms = t.elapsed().as_millis();
    eprintln!(
        "  Archive: {}ms ({} events → {} Parquet files)",
        archive_ms, report.events_archived, report.parquet_files_written
    );

    // Query cold data.
    let t = Instant::now();
    let cold_results = hybrid_cold
        .query_cold_structured(&StructuredQuery {
            org_id: OrgId::new("bench_hy"),
            source: Some(Source::new("stripe")),
            entity: None,
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: QUERY_LIMIT,
            offset: 0,
        })
        .await
        .unwrap();
    let cold_query_ms = t.elapsed().as_millis();
    eprintln!(
        "  Cold query (stripe): {}ms ({} results)",
        cold_query_ms,
        cold_results.len()
    );

    // --- Summary ---
    eprintln!("\n{}", "=".repeat(60));
    eprintln!("  SUMMARY ({BULK_SIZE} events)");
    eprintln!("{}", "=".repeat(60));
    eprintln!(
        "  {:12} {:>8} {:>10} {:>8} {:>8} {:>8}",
        "Backend", "Write", "evt/sec", "Q src", "Q ent", "Timeline"
    );
    eprintln!("  {:-<12} {:-<8} {:-<10} {:-<8} {:-<8} {:-<8}", "", "", "", "", "", "");
    for r in [&mem_result, &pg_result, &ku_result, &hy_result] {
        eprintln!(
            "  {:12} {:>6}ms {:>10.0} {:>6}ms {:>6}ms {:>6}ms",
            r.backend,
            r.write_total_ms,
            r.write_events_per_sec,
            r.query_by_source_ms,
            r.query_by_entity_ms,
            r.timeline_ms,
        );
    }
    eprintln!("  {:12} {:>6}ms {:>10.0} {:>6}ms", "Hybrid(cold)", archive_ms, 0.0, cold_query_ms);
    eprintln!();

    // Sanity: all backends should have the same event count.
    assert_eq!(mem_result.count_total, BULK_SIZE as u64);
    assert_eq!(pg_result.count_total, BULK_SIZE as u64);
    assert_eq!(ku_result.count_total, BULK_SIZE as u64);
}
