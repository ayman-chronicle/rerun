//! Write overhead benchmark: raw Postgres INSERT vs Chronicle insert_events.
//!
//! Measures the cost of Chronicle's abstraction layer by comparing identical
//! data written via:
//!
//! 1. **Raw UNNEST** — direct `sqlx::query` with the same SQL that
//!    `PostgresBackend` generates, but no Chronicle types or trait dispatch.
//! 2. **Chronicle** — `PostgresBackend::insert_events()` with entity ref
//!    propagation, concurrent pipeline, and deferred WAL.
//!
//! Run with:
//! ```bash
//! cargo test -p chronicle_store --features postgres --test bench_write_overhead -- --test-threads=1 --nocapture
//! ```

#![cfg(feature = "postgres")]

use std::time::Instant;

use chrono::{Duration, Utc};
use sqlx::PgPool;

use chronicle_core::event::{Event, EventBuilder};
use chronicle_core::ids::*;
use chronicle_store::postgres::PostgresBackend;
use chronicle_store::traits::EventStore;

const PG_URL: &str = "postgres://chronicle:chronicle@localhost:5433/chronicle";

const SIZES: &[usize] = &[100, 1_000, 5_000, 10_000, 25_000];
const WARMUP_SIZE: usize = 500;
const NUM_CUSTOMERS: usize = 200;

// ---------------------------------------------------------------------------
// Event generation
// ---------------------------------------------------------------------------

fn generate_events(org_id: &str, count: usize) -> Vec<Event> {
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
                    "currency": "usd",
                }))
                .event_time(Utc::now() - Duration::hours((count - i) as i64))
                .build()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Raw Postgres writer (no Chronicle types)
// ---------------------------------------------------------------------------

/// Insert events using the same UNNEST SQL as PostgresBackend but with no
/// Chronicle trait dispatch, no entity ref extraction, no concurrent pipeline.
async fn raw_unnest_insert(pool: &PgPool, events: &[Event]) -> Result<(), sqlx::Error> {
    let n = events.len();
    let mut event_ids = Vec::with_capacity(n);
    let mut org_ids = Vec::with_capacity(n);
    let mut sources = Vec::with_capacity(n);
    let mut topics = Vec::with_capacity(n);
    let mut event_types = Vec::with_capacity(n);
    let mut event_times = Vec::with_capacity(n);
    let mut ingestion_times = Vec::with_capacity(n);
    let mut payloads: Vec<Option<serde_json::Value>> = Vec::with_capacity(n);
    let mut media_types: Vec<Option<String>> = Vec::with_capacity(n);
    let mut media_refs: Vec<Option<String>> = Vec::with_capacity(n);
    let mut media_sizes: Vec<Option<i64>> = Vec::with_capacity(n);
    let mut raw_bodies: Vec<Option<String>> = Vec::with_capacity(n);

    for e in events {
        event_ids.push(e.event_id.to_string());
        org_ids.push(e.org_id.as_str().to_string());
        sources.push(e.source.as_str().to_string());
        topics.push(e.topic.as_str().to_string());
        event_types.push(e.event_type.as_str().to_string());
        event_times.push(e.event_time);
        ingestion_times.push(e.ingestion_time);
        payloads.push(e.payload.clone());
        media_types.push(None);
        media_refs.push(None);
        media_sizes.push(None);
        raw_bodies.push(None);
    }

    sqlx::query(
        "INSERT INTO events \
         (event_id, org_id, source, topic, event_type, event_time, \
          ingestion_time, payload, media_type, media_ref, \
          media_size_bytes, raw_body) \
         SELECT * FROM UNNEST(\
          $1::text[], $2::text[], $3::text[], $4::text[], $5::text[], \
          $6::timestamptz[], $7::timestamptz[], $8::jsonb[], $9::text[], \
          $10::text[], $11::bigint[], $12::text[]) \
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&event_ids)
    .bind(&org_ids)
    .bind(&sources)
    .bind(&topics)
    .bind(&event_types)
    .bind(&event_times)
    .bind(&ingestion_times)
    .bind(&payloads)
    .bind(&media_types)
    .bind(&media_refs)
    .bind(&media_sizes)
    .bind(&raw_bodies)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

async fn cleanup(pool: &PgPool, _org_prefix: &str) {
    sqlx::raw_sql("TRUNCATE events CASCADE")
        .execute(pool)
        .await
        .ok();
}

/// Ensure the Chronicle schema exists (the existing DB may have a legacy schema).
async fn ensure_chronicle_schema(pg: &PostgresBackend) {
    pg.run_migrations().await.expect(
        "Failed to run Chronicle migrations. \
         If the database has an incompatible 'events' table from another project, \
         drop it first: docker exec chronicle_pg psql -U chronicle -d chronicle \
         -c 'DROP TABLE IF EXISTS source_schemas, event_embeddings, event_links, entity_refs, events CASCADE'",
    );
}

// ---------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------

struct WriteResult {
    label: String,
    count: usize,
    elapsed_ms: u128,
    events_per_sec: f64,
}

impl WriteResult {
    fn print(&self) {
        eprintln!(
            "  {:>24} │ {:>6} events │ {:>6}ms │ {:>10.0} evt/sec",
            self.label, self.count, self.elapsed_ms, self.events_per_sec,
        );
    }
}

fn measure(label: &str, count: usize, elapsed: std::time::Duration) -> WriteResult {
    let ms = elapsed.as_millis();
    let eps = count as f64 / elapsed.as_secs_f64();
    WriteResult {
        label: label.to_owned(),
        count,
        elapsed_ms: ms,
        events_per_sec: eps,
    }
}

#[tokio::test]
async fn bench_write_overhead() {
    let pg = PostgresBackend::new(PG_URL).await.unwrap();
    ensure_chronicle_schema(&pg).await;
    let pool = pg.pg_pool().clone();

    eprintln!("\n{}", "=".repeat(72));
    eprintln!("  WRITE OVERHEAD: Raw Postgres UNNEST vs Chronicle insert_events");
    eprintln!("{}\n", "=".repeat(72));
    eprintln!(
        "  {:>24} │ {:>12} │ {:>8} │ {:>14}",
        "Method", "Events", "Time", "Throughput"
    );
    eprintln!("  {}", "─".repeat(68));

    // Warmup: prime the connection pool.
    let warmup = generate_events("bench_warmup", WARMUP_SIZE);
    cleanup(&pool, "bench_warmup").await;
    raw_unnest_insert(&pool, &warmup).await.unwrap();
    cleanup(&pool, "bench_warmup").await;
    pg.insert_events(&warmup).await.unwrap();
    cleanup(&pool, "bench_warmup").await;

    let mut raw_results = Vec::new();
    let mut chronicle_results = Vec::new();

    for &size in SIZES {
        let org_raw = format!("bench_raw_{size}");
        let org_chr = format!("bench_chr_{size}");

        let events_raw = generate_events(&org_raw, size);
        let events_chr = generate_events(&org_chr, size);

        cleanup(&pool, &org_raw).await;
        cleanup(&pool, &org_chr).await;

        // --- Raw UNNEST ---
        let t = Instant::now();
        raw_unnest_insert(&pool, &events_raw).await.unwrap();
        let raw_r = measure(&format!("Raw UNNEST ({size})"), size, t.elapsed());
        raw_r.print();
        raw_results.push(raw_r);

        // --- Chronicle ---
        let t = Instant::now();
        pg.insert_events(&events_chr).await.unwrap();
        let chr_r = measure(&format!("Chronicle ({size})"), size, t.elapsed());
        chr_r.print();
        chronicle_results.push(chr_r);

        cleanup(&pool, &org_raw).await;
        cleanup(&pool, &org_chr).await;
    }

    // --- Summary ---
    eprintln!("\n  {}", "─".repeat(68));
    eprintln!(
        "  {:>8} │ {:>12} │ {:>12} │ {:>10} │ {:>12}",
        "Events", "Raw (evt/s)", "Chron (evt/s)", "Overhead", "Extra work"
    );
    eprintln!("  {}", "─".repeat(68));

    for (raw, chr) in raw_results.iter().zip(chronicle_results.iter()) {
        let overhead_pct = if raw.events_per_sec > 0.0 {
            ((raw.events_per_sec - chr.events_per_sec) / raw.events_per_sec) * 100.0
        } else {
            0.0
        };
        eprintln!(
            "  {:>8} │ {:>12.0} │ {:>12.0} │ {:>9.1}% │ entity_refs + concurrent pipeline",
            raw.count, raw.events_per_sec, chr.events_per_sec, overhead_pct,
        );
    }

    eprintln!();
    eprintln!("  Chronicle overhead includes:");
    eprintln!("    - Trait dispatch (EventStore → PostgresBackend)");
    eprintln!("    - EventColumns + RefColumns struct construction");
    eprintln!("    - Entity ref extraction and UNNEST insert (entity_refs table)");
    eprintln!("    - Concurrent pipeline split (>2000 events)");
    eprintln!("    - Deferred WAL sync (SET LOCAL synchronous_commit = off)");
    eprintln!();
}
