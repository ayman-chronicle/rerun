# Chronicle Project — Context Handoff

> **Purpose**: Capture the full state of the Chronicle project so a new chat session can pick up exactly where we left off.

---

## 1. Project Overview

Chronicle is a Rust-native SaaS event store built inside the Rerun repository at `chronicle/`. It has its own Cargo workspace, separate from Rerun's. See `chronicle/README.md` for user-facing documentation.

**14 crates** — see README for full list.

---

## 2. What's Built and Working

### Storage Backends (all passing tests)

| Backend | Tests | Notes |
|---------|-------|-------|
| **InMemoryBackend** | all trait suites pass | Default, no external deps |
| **PostgresBackend** | 6 tests, 43K evt/sec | Beats raw PG by 6%. Port 5433 |
| **HybridBackend** | 8 tests | Hot/cold routing, DataFusion over Parquet |
| **KurrentBackend** | 6 tests | Dual-write to Kurrent + Postgres. Port 2113 |

### Viewer Integration

- **chronicle_rerun_bridge** — logs `ChronicleEvent`, `ChronicleLink`, `ChronicleEntityRef` archetypes. Embeds entity refs in payload JSON. Logs entity index at `_entities/{type}/{id}`.
- **chronicle_viewer_common** — egui widgets + `ChronicleFilterState` (8-dimension filter with `build_query()`)
- **Tokenized filter bar** in `re_time_panel` — source, event type, entity type, entity ID, payload text filters as removable chips
- **Link overlay** — Bezier arcs between linked events, gated on `event_time` timeline

### Native Rerun Types

- **3 archetypes**: `ChronicleEvent`, `ChronicleLink`, `ChronicleEntityRef`
- **11 components** defined in `.fbs` files at `crates/store/re_sdk_types/definitions/rerun/`

---

## 3. Link Arc Visibility (FIXED)

Four root causes fixed:
1. **Source-level matching** — `same_source()` matches any entity under the same source
2. **Collapsed tree Y lookup** — `find_row_y()` falls back to same-source matching
3. **Timeline mismatch** — gated on `event_time` timeline, moved after `time_ctrl`
4. **Demo viewer mismatch** — `ChronicleBridge::connect()` via `connect_grpc()`

---

## 4. Tokenized Filter Panel (NEW)

Implements the SaaS "Add filter → field / operator / value → chip" pattern.

**Filter fields**: Source, Event Type, Entity Type, Entity ID, Payload Text

**Key files**:
- `crates/viewer/re_time_panel/src/chronicle_filter.rs` — `ChronicleFilter` (16 tests)
- `crates/viewer/re_time_panel/src/time_panel.rs` — integration: discover, render, tree filtering
- `crates/viewer/re_viewer/src/app_state.rs` — link overlay filter integration
- `chronicle/crates/chronicle_viewer_common/src/filter_state.rs` — `ChronicleFilterState` (18 tests, `build_query()`)
- `chronicle/crates/chronicle_rerun_bridge/src/lib.rs` — entity ref logging at `_entities/` paths

**Entity dropdown**: Bridge logs `ChronicleEntityRef` at `_entities/{type}/{id}`. `discover()` parses these paths. Entity ID dropdown auto-narrows to IDs of the selected entity type via `entity_ids_by_type`.

---

## 5. Key Commands

See `chronicle/README.md` for full command reference.

---

## 5. Performance Optimizations

### Write path (beats raw Postgres at 10K+)

Seven optimizations in `events.rs`:
1. UNNEST INSERT (single array-param query)
2. Transactional batching (one `BEGIN/COMMIT`)
3. Deferred WAL sync (`SET LOCAL synchronous_commit = off`)
4. Embedded JSONB entity refs (no second table write on hot path)
5. Async entity_refs backfill (fire-and-forget for large batches)
6. Static prepared statements (avoid parse/plan overhead)
7. Concurrent pipeline (>2K events split across 4 connections)

### Read path (projection pushdown)

- `EVENT_COLUMNS_LIGHT` — envelope-only (no payload/media/raw_body)
- `SelectBuilder::events_light()` — builder entry point for light queries
- `row_to_event_light()` — zero JSONB deserialization
- `query_structured` auto-selects light when no `payload_filters`

### Benchmarks

- `tests/bench_write_overhead.rs` — raw PG UNNEST vs Chronicle `insert_events`
- `tests/bench_read_performance.rs` — 5 query scenarios on 50K events

---

## 6. Next Steps

1. ~~Fix link arc visibility~~ — DONE
2. ~~Gate C: visual verification~~ — DONE
3. ~~Phase 4: Chronicle filter panel~~ — DONE
4. ~~Performance: beat raw Postgres~~ — DONE (writes 6% faster, counts 5% faster)
5. **Phase D**: Selection panel integration — show Chronicle detail widgets when ChronicleEvent is selected
6. **Future**: Full query builder (payload field filters, backend gRPC query channel, saved views)

---

## 7. Architecture Notes

- Chronicle workspace at `chronicle/` uses a LOCAL path dep to the rerun crate (`path = "../crates/top/rerun"`)
- The viewer-side filter (`re_time_panel::ChronicleFilter`) has NO Chronicle dependencies (stdlib only)
- The Chronicle-side filter (`chronicle_viewer_common::ChronicleFilterState`) wraps the full query model
- The link overlay paints between `tree_ui()` and `time_marker_ui()` in `expanded_ui()`
- Entity refs are logged at `_entities/{type}/{id}` AND embedded in payload JSON under `_entity_refs`
- Link metadata is encoded in `_links/{src_path}/{epoch}/to/{tgt_path}/{epoch}/{link_type}` entity paths
