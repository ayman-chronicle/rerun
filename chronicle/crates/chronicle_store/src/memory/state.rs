//! Shared state for the in-memory backend.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use chronicle_core::entity_ref::EntityRef;
use chronicle_core::event::Event;
use chronicle_core::ids::{EventId, LinkId};
use chronicle_core::link::EventLink;

use crate::traits::{EventEmbedding, SourceSchema};

/// In-memory storage backend for testing and development.
///
/// All data lives in `Arc<RwLock<...>>` collections. Thread-safe,
/// but not persistent. Create via [`InMemoryBackend::new`].
#[derive(Clone, Default)]
pub struct InMemoryBackend {
    pub(crate) events: Arc<RwLock<HashMap<EventId, Event>>>,
    pub(crate) entity_refs: Arc<RwLock<Vec<EntityRef>>>,
    pub(crate) links: Arc<RwLock<HashMap<LinkId, EventLink>>>,
    pub(crate) embeddings: Arc<RwLock<HashMap<EventId, EventEmbedding>>>,
    pub(crate) schemas: Arc<RwLock<Vec<SourceSchema>>>,
}

impl InMemoryBackend {
    /// Create a new empty in-memory backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of events stored (for testing assertions).
    pub fn event_count(&self) -> usize {
        self.events.read().len()
    }

    /// Number of entity refs stored (for testing assertions).
    pub fn entity_ref_count(&self) -> usize {
        self.entity_refs.read().len()
    }

    /// Number of links stored (for testing assertions).
    pub fn link_count(&self) -> usize {
        self.links.read().len()
    }

    /// Number of embeddings stored (for testing assertions).
    pub fn embedding_count(&self) -> usize {
        self.embeddings.read().len()
    }
}
