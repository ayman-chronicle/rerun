//! The core Event type and its builder.
//!
//! An [`Event`] represents a single occurrence from a SaaS source.
//! Events carry an envelope (source, type, time) and an optional payload
//! with media. Entity references are stored separately but can be
//! specified during construction via the builder.

use crate::entity_ref::EntityRef;
use crate::ids::{EntityId, EntityType, EventId, EventType, OrgId, Source, Topic};
use crate::media::MediaAttachment;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A SaaS event with envelope metadata and optional payload/media.
///
/// Events are immutable after creation. Entity references and links
/// are stored separately and can be added at any time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    pub org_id: OrgId,
    pub source: Source,
    pub topic: Topic,
    pub event_type: EventType,
    pub event_time: DateTime<Utc>,
    pub ingestion_time: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaAttachment>,

    /// Entity refs to create alongside this event.
    /// These are written to `entity_refs` during ingestion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_refs: Vec<PendingEntityRef>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_body: Option<String>,
}

/// An entity ref that hasn't been persisted yet.
///
/// Unlike [`EntityRef`], this doesn't have an `event_id` or
/// `created_at` -- those are filled in during ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingEntityRef {
    pub entity_type: EntityType,
    pub entity_id: EntityId,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for constructing events with a fluent API.
///
/// ```
/// use chronicle_core::event::EventBuilder;
///
/// let event = EventBuilder::new("org_1", "stripe", "payments", "payment_intent.succeeded")
///     .entity("customer", "cust_123")
///     .entity("account", "acc_456")
///     .payload(serde_json::json!({"amount": 4999}))
///     .build();
/// ```
pub struct EventBuilder {
    org_id: OrgId,
    source: Source,
    topic: Topic,
    event_type: EventType,
    event_time: Option<DateTime<Utc>>,
    payload: Option<serde_json::Value>,
    media: Option<MediaAttachment>,
    entity_refs: Vec<PendingEntityRef>,
    raw_body: Option<String>,
}

impl EventBuilder {
    /// Start building an event. All four envelope fields are required.
    pub fn new(
        org_id: impl Into<OrgId>,
        source: impl Into<Source>,
        topic: impl Into<Topic>,
        event_type: impl Into<EventType>,
    ) -> Self {
        Self {
            org_id: org_id.into(),
            source: source.into(),
            topic: topic.into(),
            event_type: event_type.into(),
            event_time: None,
            payload: None,
            media: None,
            entity_refs: Vec::new(),
            raw_body: None,
        }
    }

    /// Set the event timestamp. Defaults to now if not specified.
    pub fn event_time(mut self, time: DateTime<Utc>) -> Self {
        self.event_time = Some(time);
        self
    }

    /// Attach a JSON payload.
    pub fn payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Attach media (image, audio, video).
    pub fn media(mut self, media: MediaAttachment) -> Self {
        self.media = Some(media);
        self
    }

    /// Add a dynamic entity reference.
    pub fn entity(mut self, entity_type: impl Into<EntityType>, entity_id: impl Into<EntityId>) -> Self {
        self.entity_refs.push(PendingEntityRef {
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
        });
        self
    }

    /// Preserve the original webhook body for debugging.
    pub fn raw_body(mut self, body: impl Into<String>) -> Self {
        self.raw_body = Some(body.into());
        self
    }

    /// Build the event, generating a new ID and setting timestamps.
    pub fn build(self) -> Event {
        let now = Utc::now();
        Event {
            event_id: EventId::new(),
            org_id: self.org_id,
            source: self.source,
            topic: self.topic,
            event_type: self.event_type,
            event_time: self.event_time.unwrap_or(now),
            ingestion_time: now,
            payload: self.payload,
            media: self.media,
            entity_refs: self.entity_refs,
            raw_body: self.raw_body,
        }
    }
}

impl Event {
    /// Convenience: convert pending entity refs into full [`EntityRef`]s
    /// with the event's ID and a `created_by` attribution.
    pub fn materialize_entity_refs(&self, created_by: &str) -> Vec<EntityRef> {
        self.entity_refs
            .iter()
            .map(|pending| {
                EntityRef::new(
                    self.event_id,
                    pending.entity_type,
                    pending.entity_id.clone(),
                    created_by,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_minimal() {
        let event = EventBuilder::new("org_1", "stripe", "payments", "charge.created").build();
        assert_eq!(event.source, "stripe");
        assert_eq!(event.topic, "payments");
        assert_eq!(event.event_type, "charge.created");
        assert!(event.payload.is_none());
        assert!(event.entity_refs.is_empty());
    }

    #[test]
    fn builder_full() {
        let event = EventBuilder::new("org_1", "stripe", "payments", "payment_intent.succeeded")
            .entity("customer", "cust_123")
            .entity("account", "acc_456")
            .payload(serde_json::json!({"amount": 4999, "currency": "usd"}))
            .media(MediaAttachment::jpeg(vec![0xFF, 0xD8]))
            .raw_body(r#"{"id": "pi_xxx"}"#)
            .build();

        assert_eq!(event.entity_refs.len(), 2);
        assert_eq!(event.entity_refs[0].entity_type, "customer");
        assert_eq!(event.entity_refs[1].entity_id.as_str(), "acc_456");
        assert!(event.payload.is_some());
        assert!(event.media.is_some());
        assert!(event.raw_body.is_some());
    }

    #[test]
    fn materialize_entity_refs() {
        let event = EventBuilder::new("org_1", "support", "tickets", "ticket.created")
            .entity("customer", "cust_1")
            .entity("ticket", "tkt_42")
            .build();

        let refs = event.materialize_entity_refs("ingestion");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].event_id, event.event_id);
        assert_eq!(refs[0].entity_type, "customer");
        assert_eq!(refs[0].created_by, "ingestion");
        assert_eq!(refs[1].entity_type, "ticket");
    }

    #[test]
    fn event_ids_are_unique() {
        let a = EventBuilder::new("o", "s", "t", "e").build();
        let b = EventBuilder::new("o", "s", "t", "e").build();
        assert_ne!(a.event_id, b.event_id);
    }

    #[test]
    fn default_event_time_is_now() {
        let before = Utc::now();
        let event = EventBuilder::new("o", "s", "t", "e").build();
        let after = Utc::now();
        assert!(event.event_time >= before && event.event_time <= after);
    }
}
