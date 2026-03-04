# Chronicle

A Rust-native SaaS event store designed for AI agent workloads, with
real-time visualization in the [Rerun](https://rerun.io) viewer.

Chronicle organizes events from external SaaS sources (Stripe, Intercom,
product telemetry, support systems) with dynamic entity references, causal
event linking, semantic search, and multi-modal media support.

## Quick Start

```bash
# Build everything
cd chronicle && cargo build

# Run tests (no external services needed for InMemory backend)
cargo test --all --exclude chronicle_py

# Run the standalone demo (swimlane timeline + detail panel)
cargo run -p chronicle_viewer_common --example chronicle_detail_demo

# Run the Rerun viewer demo (Terminal 1: start viewer)
cargo run -p rerun-cli --no-default-features --features release_no_web_viewer

# Terminal 2: send 500 demo events + 26 causal links
cd chronicle && cargo run -p chronicle_rerun_bridge --example chronicle_viewer_demo
```

In the Rerun viewer: switch to the **event_time** timeline, then click any
entity row to see causal link arcs. Use the **+ Filter** button above the
Streams tree to filter by source, event type, entity type, or entity ID.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  SDK / Ingestion                                        │
│  chronicle_sdk · chronicle_ingest · chronicle_py        │
└──────────────────┬──────────────────────────────────────┘
                   │ StorageEngine traits
┌──────────────────▼──────────────────────────────────────┐
│  Storage Backends                                       │
│  InMemory · Postgres · Hybrid (DataFusion) · Kurrent    │
│                              chronicle_store            │
└──────────────────┬──────────────────────────────────────┘
                   │ Arrow / gRPC
┌──────────────────▼──────────────────────────────────────┐
│  Viewer                                                 │
│  chronicle_rerun_bridge → Rerun viewer                  │
│  chronicle_viewer_common (egui widgets + filter state)  │
│  re_time_panel (link overlay + filter bar)              │
└─────────────────────────────────────────────────────────┘
```

### Storage Backends

| Backend | Tests | Throughput | Notes |
|---------|-------|------------|-------|
| **InMemoryBackend** | all trait suites pass | — | Default, no external deps |
| **PostgresBackend** | 6 tests | 55K–68K evt/sec | UNNEST + concurrent pipeline. `docker start chronicle_pg` (port 5433) |
| **HybridBackend** | 8 tests | — | Hot/cold routing, DataFusion over Parquet |
| **KurrentBackend** | 6 tests | — | Dual-write to Kurrent + Postgres. `docker start kurrentdb` (port 2113) |

All backends implement the same `EventStore`, `EntityRefStore`, `EventLinkStore`,
`EmbeddingStore`, and `SchemaStore` traits. Code above the trait boundary never
changes when switching backends.

### Viewer Integration

The bridge logs Chronicle data as native Rerun archetypes:

- **ChronicleEvent** — source, event_type, topic, payload, label
- **ChronicleLink** — source_event, target_event, link_type, confidence
- **ChronicleEntityRef** — entity_type, entity_id

The time panel shows a **tokenized filter bar** (source, event type, entity
type, entity ID, payload text) and **causal link arcs** (Bezier curves
connecting linked events across swimlane rows).

## Crate Organization

```
chronicle/crates/
  chronicle_core/          — Domain types, ID newtypes, error types, query models
  chronicle_store/         — Trait-based storage layer + 4 backends
  chronicle_ingest/        — Ingestion pipeline with micro-batching
  chronicle_query/         — Composite query engine
  chronicle_link/          — Entity extraction, JIT linking
  chronicle_embed/         — Embedding pipeline (text → vector)
  chronicle_server/        — REST + gRPC server
  chronicle_sdk/           — Rust client SDK
  chronicle_py/            — Python bindings (PyO3)
  chronicle_test_fixtures/ — Shared test data and trait test suites
  chronicle_tuid/          — Time-ordered unique IDs
  chronicle_interner/      — Global string interning
  chronicle_rerun_bridge/  — Maps Chronicle events to Rerun viewer
  chronicle_viewer_common/ — egui widgets + filter state for viewer
```

## Data Model

### Events

An event is an immutable record with an envelope (source, type, time) and
optional JSON payload:

```
EventId · OrgId · Source · Topic · EventType · event_time · ingestion_time
payload: Option<JSON> · entity_refs: Vec<(EntityType, EntityId)>
```

Events are logged to Rerun at entity path `{source}/{event_type}`, timestamped
on the `event_time` timeline.

### Entity References

Entity refs associate events with business entities (customer, account, ticket).
A single event can have many refs. Refs are stored separately and can be added
post-hoc (JIT linking by AI agents). The bridge embeds refs in the payload JSON
under `_entity_refs` and logs static `ChronicleEntityRef` archetypes at
`_entities/{type}/{id}` so the viewer can discover and filter by entity.

### Causal Links

Links connect pairs of events (e.g., payment failure → support ticket).
Each link has a type (`caused_by`, `led_to`, `triggered`, `campaign_conversion`),
confidence score (0.0–1.0), and optional reasoning text. The bridge encodes
links in entity paths at `_links/{src}/{epoch}/to/{tgt}/{epoch}/{type}` and
logs `ChronicleLink` archetypes for component queries.

## Key Commands

```bash
# Storage backends
docker start chronicle_pg                    # Postgres on port 5433
docker start kurrentdb                       # KurrentDB on port 2113

# Tests
cargo test --all --exclude chronicle_py      # All (no Python)
cargo test -p chronicle_store --features postgres --test postgres_backend -- --test-threads=1
cargo test -p chronicle_store --features hybrid --test hybrid_backend -- --test-threads=1
cargo test -p chronicle_store --features kurrent --test kurrent_backend -- --test-threads=1

# Benchmarks
cargo test -p chronicle_store --features "hybrid,kurrent" --test benchmark_all_backends -- --test-threads=1 --nocapture

# Viewer
cargo run -p chronicle_viewer_common --example chronicle_detail_demo
cargo run -p rerun-cli --no-default-features --features release_no_web_viewer
cd chronicle && cargo run -p chronicle_rerun_bridge --example chronicle_viewer_demo
```

## Principles

- **DRY**: Shared traits, one schema, one formatter, one query path
- **Test always**: Every public function has tests. Phase gates block progress.
- **Readable code**: Files stay small. Doc comments explain *why*, not *what*.
- **Swappable backends**: Storage traits decouple business logic from infrastructure.
