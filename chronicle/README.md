# Chronicle

A Rust-native SaaS event store designed for AI agent workloads.

Chronicle organizes events from external SaaS sources (Stripe, Intercom,
product telemetry, support systems) with dynamic entity references, causal
event linking, semantic search, and multi-modal media support.

## Architecture

Chronicle is built in phases behind swappable trait abstractions:

- **Phase 1**: Postgres + pgvector — single system, ACID, vector search
- **Phase 2**: Adds DataFusion + Parquet for columnar analytics on cold data
- **Phase 3**: Adds KurrentDB for real-time subscriptions and event replay

All phases share the same trait interfaces. Code above the storage layer
never changes when switching backends.

## Building

```bash
cd chronicle
cargo build
cargo nextest run --all-features --no-fail-fast
```

## Crate Organization

```
crates/
  chronicle_tuid/        — Time-ordered unique IDs (forked from re_tuid)
  chronicle_interner/    — String interning for O(1) comparison
  chronicle_core/        — Domain types, ID newtypes, error types, traits
```

More crates are added as the implementation progresses. See the plan
document for the full roadmap.

## Principles

- **DRY**: Shared traits, macros for repetitive patterns, one source of truth
- **Test always**: Every public function has tests. Phase gates block progress.
- **Readable code**: Files stay small. Doc comments explain *why*, not *what*.
- **Swappable backends**: Storage traits decouple business logic from infrastructure.
