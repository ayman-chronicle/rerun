//! Batch scaling benchmark: measures write throughput at multiple batch sizes
//! to show how single-event inserts compare to bulk batches.
//!
//! Tests Postgres and Hybrid backends at batch sizes 1, 10, 100, 500, 2000,
//! 5000, and 10000 events per insert_events() call.
//!
//! Requires: Postgres :5433, `hybrid` feature.

#![cfg(all(feature = "hybrid", feature = "postgres"))]

use std::time::Instant;

use chrono::{Duration, Utc};

use chronicle_core::event::EventBuilder;
use chronicle_core::ids::*;
use chronicle_store::hybrid::HybridBackend;
use chronicle_store::memory::InMemoryBackend;
use chronicle_store::postgres::PostgresBackend;
use chronicle_store::traits::EventStore;

const PG_URL: &str = "postgres://chronicle:chronicle@localhost:5433/chronicle";

/// Total events to insert per test. Kept constant across batch sizes
/// so throughput is directly comparable.
const TOTAL_EVENTS: usize = 10_000;

/// Batch sizes to test. Covers single-event through large-batch.
const BATCH_SIZES: &[usize] = &[1, 10, 100, 500, 2_000, 5_000, 10_000];

fn generate_events(org_id: &str, count: usize) -> Vec<chronicle_core::event::Event> {
    (0..count)
        .map(|i| {
            let cust = format!("cust_{:04}", i % 200);
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

struct ScaleResult {
    batch_size: usize,
    total_ms: u128,
    events_per_sec: f64,
    calls: usize,
}

async fn bench_at_scale(
    store: &dyn EventStore,
    events: &[chronicle_core::event::Event],
    batch_size: usize,
) -> ScaleResult {
    let t = Instant::now();
    let mut calls = 0usize;
    for chunk in events.chunks(batch_size) {
        store.insert_events(chunk).await.unwrap();
        calls += 1;
    }
    let ms = t.elapsed().as_millis();
    let eps = if ms > 0 {
        events.len() as f64 / (ms as f64 / 1000.0)
    } else {
        f64::INFINITY
    };

    ScaleResult {
        batch_size,
        total_ms: ms,
        events_per_sec: eps,
        calls,
    }
}

async fn clean_org(pool: &sqlx::PgPool, org_id: &str) {
    sqlx::raw_sql(&format!(
        "DELETE FROM event_links WHERE org_id = '{org_id}';\
         DELETE FROM entity_refs WHERE org_id = '{org_id}';\
         DELETE FROM event_embeddings WHERE org_id = '{org_id}';\
         DELETE FROM events WHERE org_id = '{org_id}';"
    ))
    .execute(pool)
    .await
    .ok();
}

fn print_table(name: &str, results: &[ScaleResult]) {
    eprintln!("\n  {name}:");
    eprintln!(
        "  {:>8} {:>8} {:>10} {:>8}",
        "Batch", "Time", "evt/sec", "Calls"
    );
    eprintln!("  {:-<8} {:-<8} {:-<10} {:-<8}", "", "", "", "");
    for r in results {
        let bar_len = (r.events_per_sec / 5000.0).min(40.0) as usize;
        let bar: String = (0..bar_len).map(|_| '#').collect();
        eprintln!(
            "  {:>8} {:>6}ms {:>10.0} {:>8} {}",
            r.batch_size, r.total_ms, r.events_per_sec, r.calls, bar,
        );
    }
}

#[tokio::test]
async fn benchmark_batch_scaling() {
    eprintln!("\n{}", "=".repeat(68));
    eprintln!("  BATCH SCALING BENCHMARK");
    eprintln!("  {TOTAL_EVENTS} total events per run, varying batch size");
    eprintln!("  Batch sizes: {BATCH_SIZES:?}");
    eprintln!("{}\n", "=".repeat(68));

    let pg = PostgresBackend::new(PG_URL).await.unwrap();
    pg.run_migrations().await.ok();

    let tmp = tempfile::tempdir().unwrap();
    let hybrid = HybridBackend::new(PG_URL, tmp.path().to_path_buf(), Duration::hours(9999))
        .await
        .unwrap();
    hybrid.run_migrations().await.ok();

    // --- Memory ---
    eprintln!("[Memory]");
    let mut mem_results = Vec::new();
    for &bs in BATCH_SIZES {
        let mem = InMemoryBackend::new();
        let events = generate_events("bm_mem", TOTAL_EVENTS);
        let r = bench_at_scale(&mem, &events, bs).await;
        mem_results.push(r);
    }
    print_table("Memory", &mem_results);

    // --- Postgres ---
    eprintln!("\n[Postgres]");
    let mut pg_results = Vec::new();
    for &bs in BATCH_SIZES {
        let org = format!("bp_{bs}");
        clean_org(pg.pg_pool(), &org).await;
        let events = generate_events(&org, TOTAL_EVENTS);
        let r = bench_at_scale(&pg, &events, bs).await;
        pg_results.push(r);
        clean_org(pg.pg_pool(), &org).await;
    }
    print_table("Postgres", &pg_results);

    // --- Hybrid ---
    eprintln!("\n[Hybrid]");
    let mut hy_results = Vec::new();
    for &bs in BATCH_SIZES {
        let org = format!("bh_{bs}");
        clean_org(hybrid.pg_pool(), &org).await;
        let events = generate_events(&org, TOTAL_EVENTS);
        let r = bench_at_scale(&hybrid, &events, bs).await;
        hy_results.push(r);
        clean_org(hybrid.pg_pool(), &org).await;
    }
    print_table("Hybrid (hot)", &hy_results);

    // --- Summary comparison ---
    eprintln!("\n{}", "=".repeat(68));
    eprintln!("  SCALING COMPARISON (evt/sec)");
    eprintln!("{}", "=".repeat(68));
    eprintln!(
        "  {:>8} {:>12} {:>12} {:>12}",
        "Batch", "Memory", "Postgres", "Hybrid"
    );
    eprintln!(
        "  {:-<8} {:-<12} {:-<12} {:-<12}",
        "", "", "", ""
    );
    for i in 0..BATCH_SIZES.len() {
        eprintln!(
            "  {:>8} {:>12.0} {:>12.0} {:>12.0}",
            BATCH_SIZES[i],
            mem_results[i].events_per_sec,
            pg_results[i].events_per_sec,
            hy_results[i].events_per_sec,
        );
    }

    // Speedup from batch=1 to batch=max.
    if pg_results.len() >= 2 {
        let pg_single = pg_results[0].events_per_sec;
        let pg_max = pg_results.last().unwrap().events_per_sec;
        let hy_single = hy_results[0].events_per_sec;
        let hy_max = hy_results.last().unwrap().events_per_sec;
        eprintln!();
        eprintln!(
            "  Postgres speedup: batch=1 ({pg_single:.0}) → batch={} ({pg_max:.0}) = {:.1}x",
            BATCH_SIZES.last().unwrap(),
            pg_max / pg_single
        );
        eprintln!(
            "  Hybrid   speedup: batch=1 ({hy_single:.0}) → batch={} ({hy_max:.0}) = {:.1}x",
            BATCH_SIZES.last().unwrap(),
            hy_max / hy_single
        );
    }
    eprintln!();
}
