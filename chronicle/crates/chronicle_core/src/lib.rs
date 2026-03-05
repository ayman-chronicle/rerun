//! Core domain types for Chronicle.
//!
//! This crate defines the foundational types used throughout Chronicle:
//! events, entity references, event links, media attachments, query types,
//! newtype IDs, and error types.
//!
//! Everything above the storage layer depends on these types. No crate in
//! Chronicle depends on a concrete storage backend -- only on the types
//! and traits defined here and in `chronicle_store`.
//!
//! # Modules
//!
//! - [`ids`]: Newtype ID wrappers (`EventId`, `OrgId`, `LinkId`, etc.)
//! - [`event`]: The `Event` type and builder
//! - [`entity_ref`]: Dynamic entity references
//! - [`link`]: Causal event links with confidence scores
//! - [`media`]: Multi-modal media attachments
//! - [`query`]: Query types for structured, timeline, semantic, and graph queries
//! - [`error`]: Shared error types
//! - [`time_range`]: Time range arithmetic

pub mod connector;
pub mod entity_ref;
pub mod error;
pub mod event;
pub mod ids;
pub mod link;
pub mod media;
pub mod query;
pub mod time_range;

pub use connector::{ConnectorError, SaasConnector};
pub use entity_ref::EntityRef;
pub use error::ChronicleError;
pub use event::Event;
pub use ids::*;
pub use link::EventLink;
pub use media::MediaAttachment;
pub use query::*;
pub use time_range::TimeRange;
