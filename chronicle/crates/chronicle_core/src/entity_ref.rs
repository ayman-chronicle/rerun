//! Dynamic entity references.
//!
//! An [`EntityRef`] associates an event with a typed entity (e.g.,
//! customer, account, ticket). A single event can have many entity refs,
//! and refs can be added after event creation (JIT linking).

use crate::ids::{EntityId, EntityType, EventId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A reference from an event to a typed entity.
///
/// Entity refs are the primary mechanism for organizing events by
/// business entities. They are stored in a separate table and can
/// be added at any time -- during ingestion, by an enrichment agent,
/// or by an AI agent during investigation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRef {
    /// The event this ref belongs to.
    pub event_id: EventId,

    /// The type of entity (e.g., "customer", "account").
    pub entity_type: EntityType,

    /// The entity identifier (e.g., "cust_123").
    pub entity_id: EntityId,

    /// Who created this ref (e.g., "ingestion", "linker_agent_v2").
    pub created_by: String,

    /// When this ref was created.
    pub created_at: DateTime<Utc>,
}

impl EntityRef {
    /// Create a new entity ref with the current timestamp.
    pub fn new(
        event_id: EventId,
        entity_type: impl Into<EntityType>,
        entity_id: impl Into<EntityId>,
        created_by: impl Into<String>,
    ) -> Self {
        Self {
            event_id,
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
            created_by: created_by.into(),
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_ref_construction() {
        let event_id = EventId::new();
        let r = EntityRef::new(event_id, "customer", "cust_123", "ingestion");
        assert_eq!(r.event_id, event_id);
        assert_eq!(r.entity_type, "customer");
        assert_eq!(r.entity_id.as_str(), "cust_123");
        assert_eq!(r.created_by, "ingestion");
    }

    #[test]
    fn entity_ref_serde_round_trip() {
        let r = EntityRef::new(EventId::new(), "account", "acc_456", "test");
        let json = serde_json::to_string(&r).unwrap();
        let parsed: EntityRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r.entity_type, parsed.entity_type);
        assert_eq!(r.entity_id, parsed.entity_id);
    }
}
