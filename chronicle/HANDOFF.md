# Chronicle Project — Context Handoff

> **Purpose**: Capture the full state of the Chronicle project so a new chat session can pick up exactly where we left off.

---

## 1. Project Overview

Chronicle is a Rust-native SaaS event store built inside the Rerun repository at `chronicle/`. It has its own Cargo workspace, separate from Rerun's.

**14 crates:**
- `chronicle_core` — core types (EventEnvelope, EntityRef, Link, Schema, etc.)
- `chronicle_store` — trait-based storage layer + 4 backends
- `chronicle_ingest` — ingestion pipeline
- `chronicle_query` — query engine
- `chronicle_link` — event linking / correlation
- `chronicle_embed` — embedding support
- `chronicle_server` — HTTP/gRPC server
- `chronicle_sdk` — Rust client SDK
- `chronicle_py` — Python bindings (PyO3)
- `chronicle_test_fixtures` — shared test data
- `chronicle_tuid` — time-based unique IDs
- `chronicle_interner` — string interning
- `chronicle_rerun_bridge` — maps Chronicle events to Rerun viewer
- `chronicle_viewer_common` — egui widgets for Chronicle data

---

## 2. What's Built and Working

### Storage Backends (all passing tests)

| Backend | Tests | Notes |
|---------|-------|-------|
| **InMemoryBackend** | all trait suites pass | Default, no external deps |
| **PostgresBackend** | 6 tests, 55K evt/sec write | UNNEST + concurrent pipeline. Port 5433, `docker start chronicle_pg` |
| **HybridBackend** | 8 tests | Hot/cold routing, DataFusion over Parquet |
| **KurrentBackend** | 6 tests | Dual-write to Kurrent + Postgres. Port 2113, `docker start kurrentdb` |

### Performance Benchmarks

- `tests/benchmark_all_backends.rs` — 10K events across all 4 backends
- `tests/benchmark_batch_scaling.rs` — batch size 1 through 10K, throughput curve
- Postgres peak: **68K evt/sec** at batch=5000, **55K** at 50K events

### Viewer Integration

- **chronicle_rerun_bridge** — logs ChronicleEvent (native archetype) + TextDocument payloads to Rerun viewer
- **chronicle_viewer_common** — egui widgets (EventEnvelopeWidget, EntityRefChips, LinksList, PayloadJsonWidget)
- **Standalone demo**: `cargo run -p chronicle_viewer_common --example chronicle_detail_demo` — works, shows swimlanes + arcs + detail panel
- **Rerun bridge demo**: `cargo run -p chronicle_rerun_bridge --example chronicle_viewer_demo` — seeds 500 events + 26 links, sends to Rerun viewer

### Native Rerun Types (Phase A complete)

- **3 archetypes**: `ChronicleEvent`, `ChronicleLink`, `ChronicleEntityRef`
- **11 components**: ChronicleSource, ChronicleEventType, ChronicleTopic, ChroniclePayload, ChronicleLinkType, ChronicleConfidence, ChronicleReasoning, ChronicleSourceEvent, ChronicleTargetEvent, ChronicleEntityType, ChronicleEntityId
- Defined in `.fbs` files, codegen run successfully (Rust + Python + C++)
- Located at: `crates/store/re_sdk_types/definitions/rerun/archetypes/chronicle_*.fbs` and `components/chronicle_*.fbs`

---

## 3. Current Issue: Link Arcs Not Showing

The link overlay arcs in the Rerun viewer time panel are not appearing when clicking entity rows.

### What Exists

- `crates/viewer/re_time_panel/src/link_overlay.rs` — LinkOverlayState, ResolvedLink, `paint()` with Bezier arcs (unit tested, 4 tests pass)
- `crates/viewer/re_time_panel/src/time_panel.rs` — link_overlay field, row_positions HashMap, time_area_x_range, `x_from_time()` method, paint call in `expanded_ui()`
- `crates/viewer/re_viewer/src/app_state.rs` — `populate_chronicle_link_overlay()` function called on every frame, scans `_links/` entity paths

### How Links Are Logged

Bridge logs links at entity paths like:

```
_links/{src_source}/{src_type}/{src_epoch}/to/{tgt_source}/{tgt_type}/{tgt_epoch}/{link_type}
```

Example: `_links/stripe/payment_intent.failed/1741020000/to/support/ticket.created/1741106400/caused_by`

### Likely Causes

1. **Dots in entity paths** — this is likely THE issue. Rerun treats `.` as a path separator. So `stripe/payment_intent.failed` becomes 3 path segments (`stripe`, `payment_intent`, `failed`) instead of 2. The parsed paths won't match.
2. **Path matching**: `populate_chronicle_link_overlay` in `app_state.rs` (~line 975) parses entity paths from `recording.sorted_entity_paths()`. The path format needs 8 parts after `_links/`, with `"to"` at index 3.
3. **row_positions may be empty**: If `row_positions` isn't populated during `show_entity()`, no matches will be found.
4. **time_area_x_range may be (0,0)**: If the time area hasn't been laid out yet.
5. **x_from_time may return None**: If the timestamp is outside the visible time range.

### Most Likely Fix

Replace `.` with `_` (or another safe character) in entity paths to avoid Rerun's `.` path separator interpretation. Or use `EntityPath::parse()` with proper escaping. The bridge should use `payment_intent_succeeded` instead of `payment_intent.succeeded`.

---

## 4. Key Commands

```bash
# Start Postgres
docker start chronicle_pg

# Start KurrentDB
docker start kurrentdb

# Run Chronicle tests (all except Python)
cd chronicle && cargo test --all --exclude chronicle_py

# Run Postgres-specific tests
cargo test -p chronicle_store --features postgres --test postgres_backend -- --test-threads=1

# Run hybrid tests
cargo test -p chronicle_store --features hybrid --test hybrid_backend -- --test-threads=1

# Run Kurrent tests
cargo test -p chronicle_store --features kurrent --test kurrent_backend -- --test-threads=1

# Run benchmarks
cargo test -p chronicle_store --features "hybrid,kurrent" --test benchmark_all_backends -- --test-threads=1 --nocapture
cargo test -p chronicle_store --features "hybrid" --test benchmark_batch_scaling -- --test-threads=1 --nocapture

# Run standalone detail demo (works immediately, no viewer needed)
cd chronicle && cargo run -p chronicle_viewer_common --example chronicle_detail_demo

# Build and run Rerun viewer
cargo run -p rerun-cli --no-default-features --features release_no_web_viewer

# Send Chronicle demo data to viewer (separate terminal)
cd chronicle && cargo run -p chronicle_rerun_bridge --example chronicle_viewer_demo

# Run codegen (after modifying .fbs files)
cargo run --package re_types_builder
```

---

## 5. Plan Files

| Plan | Description |
|------|-------------|
| `~/.cursor/plans/chronicle_event_store_4c7a21b9.plan.md` | Original architecture plan (Phase 1–3 storage backends) |
| `~/.cursor/plans/postgres_write_perf_b88312bd.plan.md` | Postgres write performance optimization |
| `~/.cursor/plans/chronicle_viewer_d9265c7d.plan.md` | Viewer integration plan (DataFrame, Bridge, Link overlay, Detail panel) |
| `~/.cursor/plans/chronicle_native_types_e0297b4f.plan.md` | Native Rerun type definitions plan |

---

## 6. Git Log (recent commits)

```
e6c83eaf5c Fix link arcs to connect to actual event positions
d4b70a5638 Fix TextLog view crash: log only ChronicleEvent, not dual TextLog+ChronicleEvent
d27c06c7fb Bridge native ChronicleEvent/Link types + real link overlay wiring
9bd6259d40 Phase A: Chronicle native types -- ChronicleEvent, ChronicleLink, ChronicleEntityRef
6e6694a90b Fix link overlay: populate row_positions, fix path matching, fix X coords
6f06578f4c Wire Chronicle link overlay into viewer selection handler
08df3dbf52 Add chronicle_detail_demo: interactive viewer with timeline arcs + detail widgets
da6f3f09ce Phase 3: Chronicle viewer widgets -- event detail panel components
ebe36f160b Phase 2: Link overlay on time panel for Chronicle event links
9f8361b8e3 Bridge: log full JSON payload as TextDocument on child entity
11687381fa Add chronicle_viewer_demo: runnable example seeding 500 events for Rerun viewer
cc86a117d1 Phase 1: chronicle_rerun_bridge -- maps Chronicle events to Rerun viewer
4e1af4911f Phase 0: DataFrame integration with shared Arrow export
90b3746166 Add batch scaling benchmark: single-event through bulk throughput curve
9e5a189bb8 UNNEST + concurrent pipeline: 55K evt/sec Postgres write throughput
```

---

## 7. Next Steps

1. **Fix the link arc visibility** — likely the dot-in-path issue. Entity paths like `stripe/payment_intent.succeeded` may be interpreted as 3 segments instead of 2 by Rerun's path parser.
2. **Gate C**: Verify link arcs work end-to-end with user visual confirmation.
3. **Phase D**: Selection panel integration — show Chronicle detail widgets when ChronicleEvent is selected.
4. **Phase 4** (from viewer plan): Entity browser + semantic search.

---

## 8. Architecture Notes

- Chronicle workspace at `chronicle/` uses a LOCAL path dep to the rerun crate (`path = "../crates/top/rerun"`) for native archetype access.
- The link overlay paint happens between `tree_ui()` and `time_marker_ui()` in `expanded_ui()`.
- `populate_chronicle_link_overlay()` runs every frame in `app_state.rs`, reading from `recording.sorted_entity_paths()`.
- The bridge's `log_events_with_links()` builds an `event_id -> (entity_path, timestamp)` lookup, then encodes link metadata in the `_links/` entity path convention.
