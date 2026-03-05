//! Shared state for the in-memory backend.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use chronicle_core::entity_ref::EntityRef;
use chronicle_core::event::Event;
use chronicle_core::ids::{EventId, LinkId, OrgId};
use chronicle_core::link::EventLink;

use crate::subscriptions::{EventHandler, SubFilter, matches_filter};
use crate::traits::{EventEmbedding, SourceSchema};

/// An active subscription with its filter and handler.
pub(crate) struct ActiveSubscription {
    pub id: u64,
    pub filter: SubFilter,
    pub handler: Arc<dyn EventHandler>,
}

/// In-memory storage backend for testing and development.
///
/// All data lives in `Arc<RwLock<...>>` collections. Thread-safe,
/// but not persistent. Create via [`InMemoryBackend::new`].
///
/// Supports push subscriptions: call `subscribe()` to register a
/// handler that fires on every matching `insert_events` call.
#[derive(Clone, Default)]
pub struct InMemoryBackend {
    pub(crate) events: Arc<RwLock<HashMap<EventId, Event>>>,
    pub(crate) entity_refs: Arc<RwLock<Vec<(OrgId, EntityRef)>>>,
    pub(crate) links: Arc<RwLock<HashMap<LinkId, (OrgId, EventLink)>>>,
    pub(crate) embeddings: Arc<RwLock<HashMap<EventId, EventEmbedding>>>,
    pub(crate) schemas: Arc<RwLock<Vec<SourceSchema>>>,
    pub(crate) subscriptions: Arc<RwLock<Vec<ActiveSubscription>>>,
    next_sub_id: Arc<AtomicU64>,
}

impl InMemoryBackend {
    /// Create a new empty in-memory backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// Dispatch events to all matching subscriptions.
    ///
    /// Collects matching handlers while holding the read lock, then drops
    /// the lock before calling handlers (avoids holding RwLock across await).
    pub(crate) async fn dispatch_to_subscribers(&self, events: &[Event]) {
        let handlers: Vec<(Arc<dyn EventHandler>, Vec<Event>)> = {
            let subs = self.subscriptions.read();
            subs.iter()
                .map(|sub| {
                    let matching: Vec<Event> = events
                        .iter()
                        .filter(|e| matches_filter(e, &sub.filter))
                        .cloned()
                        .collect();
                    (Arc::clone(&sub.handler), matching)
                })
                .filter(|(_, evts)| !evts.is_empty())
                .collect()
        };

        for (handler, matched_events) in handlers {
            for event in &matched_events {
                if let Err(e) = handler.handle(event).await {
                    tracing::warn!("subscription handler error: {e}");
                }
            }
        }
    }

    /// Allocate a unique subscription ID.
    pub(crate) fn next_subscription_id(&self) -> u64 {
        self.next_sub_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Remove a subscription by ID.
    pub(crate) fn remove_subscription(&self, id: u64) {
        self.subscriptions.write().retain(|s| s.id != id);
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

    /// Number of active subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.read().len()
    }
}
