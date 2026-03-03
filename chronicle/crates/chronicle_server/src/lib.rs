//! Chronicle REST + gRPC server.
//!
//! Exposes all Chronicle capabilities over HTTP:
//!
//! - `POST /v1/events` -- ingest events
//! - `POST /v1/events/batch` -- batch ingest
//! - `GET  /v1/events` -- structured query
//! - `GET  /v1/timeline/{entity_type}/{entity_id}` -- entity timeline
//! - `POST /v1/search` -- semantic search
//! - `GET  /v1/links/{event_id}` -- graph traversal
//! - `POST /v1/entity-refs` -- add entity ref
//! - `POST /v1/event-links` -- create event link
//! - `POST /v1/link-entity` -- JIT entity linking
//! - `GET  /v1/discover/sources` -- list sources
//! - `GET  /v1/discover/entity-types` -- list entity types
//!
//! # Architecture
//!
//! The server holds a [`ServerState`] containing all services. Each
//! route handler extracts the state and calls the appropriate service.
//! axum's `State` extractor makes this clean and testable.

pub mod rest;
pub mod state;

pub use state::ServerState;
