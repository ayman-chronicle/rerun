//! In-memory storage backend.
//!
//! Implements all storage traits using `parking_lot::RwLock` and `HashMap`/`Vec`.
//! Useful for:
//!
//! - Running trait test suites without a database
//! - Unit testing service crates without I/O
//! - Prototyping and development
//!
//! **Not for production use** -- all data is lost on drop.

mod events;
mod entity_refs;
mod links;
mod embeddings;
mod schemas;
mod state;
mod subscriptions;

pub use state::InMemoryBackend;
