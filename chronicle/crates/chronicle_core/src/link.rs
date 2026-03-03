//! Causal event links.
//!
//! An [`EventLink`] represents a directed edge between two events with
//! a type (e.g., "caused_by"), a confidence score, and optional reasoning.
//! Links form a graph that AI agents traverse to understand causal chains.

use crate::error::ValidationError;
use crate::ids::{Confidence, EventId, LinkId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A directed link between two events.
///
/// Links are created by AI agents during investigation, by automated
/// rules during enrichment, or manually by users. Each link has a
/// confidence score and optional reasoning explaining why the link exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventLink {
    pub link_id: LinkId,
    pub source_event_id: EventId,
    pub target_event_id: EventId,
    pub link_type: String,
    pub confidence: Confidence,
    pub reasoning: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

impl EventLink {
    /// Validate that a link is well-formed.
    ///
    /// Rules:
    /// - Source and target must be different events (no self-links)
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.source_event_id == self.target_event_id {
            return Err(ValidationError::SelfLink);
        }
        Ok(())
    }
}

/// Direction for graph traversal queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkDirection {
    /// Follow links where the start event is the source.
    Outgoing,
    /// Follow links where the start event is the target.
    Incoming,
    /// Follow links in both directions.
    Both,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_link(source: EventId, target: EventId) -> EventLink {
        EventLink {
            link_id: LinkId::new(),
            source_event_id: source,
            target_event_id: target,
            link_type: "caused_by".to_string(),
            confidence: Confidence::new(0.9).unwrap(),
            reasoning: Some("test".to_string()),
            created_by: "test".to_string(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn valid_link() {
        let link = make_link(EventId::new(), EventId::new());
        assert!(link.validate().is_ok());
    }

    #[test]
    fn self_link_rejected() {
        let id = EventId::new();
        let link = make_link(id, id);
        assert!(matches!(link.validate(), Err(ValidationError::SelfLink)));
    }

    #[test]
    fn link_direction_serde() {
        let json = serde_json::to_string(&LinkDirection::Incoming).unwrap();
        assert_eq!(json, "\"incoming\"");
        let parsed: LinkDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, LinkDirection::Incoming);
    }
}
