//! Read performance benchmark: raw Postgres SQL vs Chronicle query engine.
//!
//! Seeds 50K events across 4 sources for 500 customers, then measures
//! query latency for five scenarios. Chronicle's `EventStore` trait
//! methods are compared against equivalent hand-written SQL.
//!
//! Run with:
//! ```bash
//! cargo test -p chronicle_store --features postgres --test bench_read_performance -- --test-threads=1 --nocapture
//! ```

#![cfg(feature = "postgres")]

use std::time::Instant;

use chrono::{Duration, Utc};
use sqlx::{PgPool, Row};

use chronicle_core::event::EventBuilder;
use chronicle_core::ids::*;
use chronicle_core::query::{OrderBy, StructuredQuery, TimelineQuery};
use chronicle_store::postgres::PostgresBackend;
use chronicle_store::traits::EventStore;

const PG_URL: &str = "postgres://chronicle:chronicle@localhost:5433/chronicle";
const SEED_SIZE: usize = 50_000;
const NUM_CUSTOMERS: usize = 500;
const ORG: &str = "bench_read";
const QUERY_LIMIT: usize = 10_000;
const ITERATIONS: usize = 5;

// ---------------------------------------------------------------------------
// Seed data
// ---------------------------------------------------------------------------

fn generate_events(count: usize) -> Vec<chronicle_core::event::Event> {
    let now = Utc::now();
    (0..count)
        .map(|i| {
            let cust = format!("cust_{:04}", i % NUM_CUSTOMERS);
            let (source, event_type, topic) = match i % 4 {
                0 => ("stripe", "payment_intent.succeeded", "payments"),
                1 => ("support", "ticket.created", "tickets"),
                2 => ("product", "page.viewed", "usage"),
                _ => ("marketing", "campaign.sent", "campaigns"),
            };

            EventBuilder::new(ORG, source, topic, event_type)
                .entity("customer", cust.as_str())
                .payload(serde_json::json!({
                    "amount": 1000 + (i % 9999) as i64,
                    "index": i,
                    "currency": "usd",
                    "plan": match i % 3 { 0 => "starter", 1 => "pro", _ => "enterprise" },
                }))
                .event_time(now - Duration::hours((count - i) as i64))
                .build()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Raw SQL queries (the "competitor")
// ---------------------------------------------------------------------------

async fn raw_query_by_source(pool: &PgPool) -> usize {
    let rows = sqlx::query(
        "SELECT e.event_id, e.org_id, e.source, e.topic, e.event_type, \
         e.event_time, e.ingestion_time, e.payload, e.media_type, e.media_ref, \
         e.media_blob, e.media_size_bytes, e.raw_body \
         FROM events e WHERE e.org_id = $1 AND e.source = $2 \
         ORDER BY e.event_time DESC LIMIT $3",
    )
    .bind(ORG)
    .bind("stripe")
    .bind(QUERY_LIMIT as i64)
    .fetch_all(pool)
    .await
    .unwrap();
    rows.len()
}

async fn raw_query_by_entity(pool: &PgPool) -> usize {
    let rows = sqlx::query(
        "SELECT e.event_id, e.org_id, e.source, e.topic, e.event_type, \
         e.event_time, e.ingestion_time, e.payload, e.media_type, e.media_ref, \
         e.media_blob, e.media_size_bytes, e.raw_body \
         FROM events e \
         JOIN entity_refs er ON e.event_id = er.event_id \
         WHERE e.org_id = $1 AND er.entity_type = $2 AND er.entity_id = $3 \
         ORDER BY e.event_time ASC LIMIT $4",
    )
    .bind(ORG)
    .bind("customer")
    .bind("cust_0001")
    .bind(QUERY_LIMIT as i64)
    .fetch_all(pool)
    .await
    .unwrap();
    rows.len()
}

async fn raw_count_by_source(pool: &PgPool) -> i64 {
    let row = sqlx::query(
        "SELECT COUNT(*) as cnt FROM events e WHERE e.org_id = $1 AND e.source = $2",
    )
    .bind(ORG)
    .bind("stripe")
    .fetch_one(pool)
    .await
    .unwrap();
    row.get::<i64, _>("cnt")
}

async fn raw_count_all(pool: &PgPool) -> i64 {
    let row = sqlx::query("SELECT COUNT(*) as cnt FROM events e WHERE e.org_id = $1")
        .bind(ORG)
        .fetch_one(pool)
        .await
        .unwrap();
    row.get::<i64, _>("cnt")
}

async fn raw_query_by_source_and_type(pool: &PgPool) -> usize {
    let rows = sqlx::query(
        "SELECT e.event_id, e.org_id, e.source, e.topic, e.event_type, \
         e.event_time, e.ingestion_time, e.payload, e.media_type, e.media_ref, \
         e.media_blob, e.media_size_bytes, e.raw_body \
         FROM events e \
         WHERE e.org_id = $1 AND e.source = $2 AND e.event_type = $3 \
         ORDER BY e.event_time DESC LIMIT $4",
    )
    .bind(ORG)
    .bind("stripe")
    .bind("payment_intent.succeeded")
    .bind(QUERY_LIMIT as i64)
    .fetch_all(pool)
    .await
    .unwrap();
    rows.len()
}

// ---------------------------------------------------------------------------
// Chronicle queries (through EventStore trait)
// ---------------------------------------------------------------------------

async fn chronicle_query_by_source(store: &dyn EventStore) -> usize {
    store
        .query_structured(&StructuredQuery {
            org_id: OrgId::new(ORG),
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
        .unwrap()
        .len()
}

async fn chronicle_query_by_entity(store: &dyn EventStore) -> usize {
    store
        .query_timeline(&TimelineQuery {
            org_id: OrgId::new(ORG),
            entity_type: EntityType::new("customer"),
            entity_id: EntityId::new("cust_0001"),
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

async fn chronicle_count_by_source(store: &dyn EventStore) -> u64 {
    store
        .count(&StructuredQuery {
            org_id: OrgId::new(ORG),
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
        .unwrap()
}

async fn chronicle_count_all(store: &dyn EventStore) -> u64 {
    store
        .count(&StructuredQuery {
            org_id: OrgId::new(ORG),
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
        .unwrap()
}

async fn chronicle_query_by_source_and_type(store: &dyn EventStore) -> usize {
    store
        .query_structured(&StructuredQuery {
            org_id: OrgId::new(ORG),
            source: Some(Source::new("stripe")),
            entity: None,
            topic: None,
            event_type: Some(EventType::new("payment_intent.succeeded")),
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit: QUERY_LIMIT,
            offset: 0,
        })
        .await
        .unwrap()
        .len()
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct QueryResult {
    name: String,
    raw_us: u128,
    chronicle_us: u128,
    raw_count: usize,
    chronicle_count: usize,
}

impl QueryResult {
    fn print(&self) {
        let ratio = if self.chronicle_us > 0 {
            self.raw_us as f64 / self.chronicle_us as f64
        } else {
            0.0
        };
        let winner = if self.chronicle_us <= self.raw_us {
            "Chronicle"
        } else {
            "Raw"
        };
        eprintln!(
            "  {:28} │ {:>8}µs │ {:>8}µs │ {:>5.2}x │ {} │ {}/{}",
            self.name,
            self.raw_us,
            self.chronicle_us,
            ratio,
            winner,
            self.raw_count,
            self.chronicle_count,
        );
    }
}

async fn bench_query<F1, F2, Fut1, Fut2>(
    name: &str,
    raw_fn: F1,
    chronicle_fn: F2,
) -> QueryResult
where
    F1: Fn() -> Fut1,
    F2: Fn() -> Fut2,
    Fut1: std::future::Future<Output = usize>,
    Fut2: std::future::Future<Output = usize>,
{
    // Warmup
    let _ = raw_fn().await;
    let _ = chronicle_fn().await;

    let mut raw_total = 0u128;
    let mut chr_total = 0u128;
    let mut raw_count = 0;
    let mut chr_count = 0;

    for _ in 0..ITERATIONS {
        let t = Instant::now();
        raw_count = raw_fn().await;
        raw_total += t.elapsed().as_micros();

        let t = Instant::now();
        chr_count = chronicle_fn().await;
        chr_total += t.elapsed().as_micros();
    }

    QueryResult {
        name: name.to_owned(),
        raw_us: raw_total / ITERATIONS as u128,
        chronicle_us: chr_total / ITERATIONS as u128,
        raw_count,
        chronicle_count: chr_count,
    }
}

// ---------------------------------------------------------------------------
// The benchmark
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bench_read_performance() {
    let pg = PostgresBackend::new(PG_URL).await.unwrap();
    pg.run_migrations().await.ok();
    let pool = pg.pg_pool().clone();

    // Clean + seed
    sqlx::raw_sql("TRUNCATE events CASCADE")
        .execute(&pool)
        .await
        .ok();

    eprintln!("\n  Seeding {SEED_SIZE} events for {NUM_CUSTOMERS} customers…");
    let events = generate_events(SEED_SIZE);
    let batch_size = 5000;
    for chunk in events.chunks(batch_size) {
        pg.insert_events(chunk).await.unwrap();
    }
    // Wait for async ref backfill to complete.
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let total = pg
        .count(&StructuredQuery {
            org_id: OrgId::new(ORG),
            source: None,
            entity: None,
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::default(),
            limit: 1,
            offset: 0,
        })
        .await
        .unwrap();
    eprintln!("  Seeded: {total} events. Running queries ({ITERATIONS} iterations each)…\n");

    eprintln!(
        "  {:28} │ {:>10} │ {:>10} │ {:>5} │ {:>10} │ rows",
        "Query", "Raw SQL", "Chronicle", "Ratio", "Winner"
    );
    eprintln!("  {}", "─".repeat(90));

    // 1. Query by source
    let r1 = bench_query(
        "Filter by source",
        || raw_query_by_source(&pool),
        || chronicle_query_by_source(&pg),
    )
    .await;
    r1.print();

    // 2. Query by source + event_type
    let r2 = bench_query(
        "Filter by source + type",
        || raw_query_by_source_and_type(&pool),
        || chronicle_query_by_source_and_type(&pg),
    )
    .await;
    r2.print();

    // 3. Entity timeline (JOIN entity_refs)
    let r3 = bench_query(
        "Entity timeline (JOIN refs)",
        || raw_query_by_entity(&pool),
        || chronicle_query_by_entity(&pg),
    )
    .await;
    r3.print();

    // 4. Count by source
    let r4 = bench_query(
        "Count by source",
        || async { raw_count_by_source(&pool).await as usize },
        || async { chronicle_count_by_source(&pg).await as usize },
    )
    .await;
    r4.print();

    // 5. Count all
    let r5 = bench_query(
        "Count all events",
        || async { raw_count_all(&pool).await as usize },
        || async { chronicle_count_all(&pg).await as usize },
    )
    .await;
    r5.print();

    eprintln!();
    eprintln!("  Chronicle overhead: trait dispatch + query builder + row_to_event conversion.");
    eprintln!("  If ratio >= 1.0, Chronicle matches or beats hand-written SQL.");
    eprintln!();

    // Sanity checks
    assert_eq!(r1.raw_count, r1.chronicle_count);
    assert_eq!(r2.raw_count, r2.chronicle_count);
    assert_eq!(r3.raw_count, r3.chronicle_count);
    assert_eq!(r4.raw_count, r4.chronicle_count);
    assert_eq!(r5.raw_count, r5.chronicle_count);
}
