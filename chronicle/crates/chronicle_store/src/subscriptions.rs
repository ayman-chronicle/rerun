//! Push subscription service for real-time event delivery.
//!
//! Agents subscribe with a [`SubFilter`] and receive events via an
//! [`EventHandler`] callback as they are inserted. Supports filtering
//! by source, event type, entity, and payload content.
//!
//! Implemented by: `InMemoryBackend`, `KurrentBackend`.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::{EntityId, EntityType, EventType, OrgId, Source};

/// Position from which to start a subscription.
#[derive(Debug, Clone)]
pub enum SubscriptionPosition {
    /// From the very beginning of the stream/log.
    Beginning,
    /// From the current end (only new events).
    End,
}

/// Filter for which events a subscription should receive.
///
/// All fields are optional. `None` = no restriction on that dimension.
/// Multiple active fields are AND-combined.
#[derive(Debug, Clone, Default)]
pub struct SubFilter {
    pub org_id: Option<OrgId>,
    pub sources: Option<Vec<Source>>,
    pub event_types: Option<Vec<EventType>>,
    /// Subscribe to events for a specific entity (cross-source).
    pub entity: Option<(EntityType, EntityId)>,
    /// Case-insensitive substring match on serialized payload JSON.
    pub payload_contains: Option<String>,
}

/// A handle to a running subscription. Drop or call `cancel()` to unsubscribe.
pub struct SubscriptionHandle {
    _cancel: broadcast::Sender<()>,
}

impl SubscriptionHandle {
    pub fn new(cancel: broadcast::Sender<()>) -> Self {
        Self { _cancel: cancel }
    }

    pub fn cancel(self) {
        drop(self._cancel);
    }
}

/// Callback trait for receiving events from a subscription.
#[async_trait]
pub trait EventHandler: Send + Sync + 'static {
    async fn handle(&self, event: &Event) -> Result<(), StoreError>;
}

/// Push-based event subscription service.
#[async_trait]
pub trait SubscriptionService: Send + Sync + 'static {
    async fn subscribe(
        &self,
        filter: SubFilter,
        position: SubscriptionPosition,
        handler: Arc<dyn EventHandler>,
    ) -> Result<SubscriptionHandle, StoreError>;
}

/// Check whether an event matches a subscription filter.
pub fn matches_filter(event: &Event, filter: &SubFilter) -> bool {
    if let Some(ref org) = filter.org_id {
        if event.org_id != *org {
            return false;
        }
    }
    if let Some(ref sources) = filter.sources {
        if !sources.iter().any(|s| event.source == *s) {
            return false;
        }
    }
    if let Some(ref types) = filter.event_types {
        if !types.iter().any(|t| event.event_type == *t) {
            return false;
        }
    }
    if let Some((ref etype, ref eid)) = filter.entity {
        let has_match = event.entity_refs.iter().any(|er| {
            er.entity_type.as_str() == etype.as_str() && er.entity_id.as_str() == eid.as_str()
        });
        if !has_match {
            return false;
        }
    }
    if let Some(ref needle) = filter.payload_contains {
        let payload_str = event
            .payload
            .as_ref()
            .map(|p| p.to_string())
            .unwrap_or_default();
        if !payload_str.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_core::event::EventBuilder;

    fn test_event() -> Event {
        EventBuilder::new("org_1", "stripe", "payments", "charge.created")
            .entity("customer", "cust_042")
            .payload(serde_json::json!({"amount": 4999, "currency": "usd"}))
            .build()
    }

    #[test]
    fn empty_filter_matches_everything() {
        assert!(matches_filter(&test_event(), &SubFilter::default()));
    }

    #[test]
    fn source_filter() {
        let f = SubFilter { sources: Some(vec![Source::new("stripe")]), ..Default::default() };
        assert!(matches_filter(&test_event(), &f));

        let f = SubFilter { sources: Some(vec![Source::new("support")]), ..Default::default() };
        assert!(!matches_filter(&test_event(), &f));
    }

    #[test]
    fn event_type_filter() {
        let f = SubFilter {
            event_types: Some(vec![EventType::new("charge.created")]),
            ..Default::default()
        };
        assert!(matches_filter(&test_event(), &f));

        let f = SubFilter {
            event_types: Some(vec![EventType::new("ticket.created")]),
            ..Default::default()
        };
        assert!(!matches_filter(&test_event(), &f));
    }

    #[test]
    fn entity_filter() {
        let f = SubFilter {
            entity: Some((EntityType::new("customer"), EntityId::new("cust_042"))),
            ..Default::default()
        };
        assert!(matches_filter(&test_event(), &f));

        let f = SubFilter {
            entity: Some((EntityType::new("customer"), EntityId::new("cust_999"))),
            ..Default::default()
        };
        assert!(!matches_filter(&test_event(), &f));
    }

    #[test]
    fn payload_contains_filter() {
        let f = SubFilter { payload_contains: Some("4999".into()), ..Default::default() };
        assert!(matches_filter(&test_event(), &f));

        let f = SubFilter { payload_contains: Some("USD".into()), ..Default::default() };
        assert!(matches_filter(&test_event(), &f));

        let f = SubFilter { payload_contains: Some("eur".into()), ..Default::default() };
        assert!(!matches_filter(&test_event(), &f));
    }

    #[test]
    fn combined_filters_are_and() {
        let f = SubFilter {
            sources: Some(vec![Source::new("stripe")]),
            entity: Some((EntityType::new("customer"), EntityId::new("cust_042"))),
            payload_contains: Some("4999".into()),
            ..Default::default()
        };
        assert!(matches_filter(&test_event(), &f));

        let f = SubFilter {
            sources: Some(vec![Source::new("support")]),
            entity: Some((EntityType::new("customer"), EntityId::new("cust_042"))),
            ..Default::default()
        };
        assert!(!matches_filter(&test_event(), &f));
    }
}
