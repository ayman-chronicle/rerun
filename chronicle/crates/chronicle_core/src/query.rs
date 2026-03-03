//! Query types for the five query modes.
//!
//! These types define *what* to query, not *how* to execute it.
//! The storage backend translates them into SQL, DataFusion plans,
//! or whatever the backend requires.

use crate::ids::{EntityId, EntityType, EventId, EventType, OrgId, Source, Topic};
use crate::link::LinkDirection;
use crate::time_range::TimeRange;
use serde::{Deserialize, Serialize};

/// Filter on envelope fields. Used by [`StructuredQuery`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredQuery {
    pub org_id: OrgId,
    pub entity: Option<(EntityType, EntityId)>,
    pub source: Option<Source>,
    pub topic: Option<Topic>,
    pub event_type: Option<EventType>,
    pub time_range: Option<TimeRange>,
    pub payload_filters: Vec<PayloadFilter>,
    pub group_by: Option<GroupBy>,
    pub order_by: OrderBy,
    pub limit: usize,
    pub offset: usize,
}

/// All events for a specific entity, chronologically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineQuery {
    pub org_id: OrgId,
    pub entity_type: EntityType,
    pub entity_id: EntityId,
    pub time_range: Option<TimeRange>,
    pub sources: Option<Vec<Source>>,
    pub include_linked: bool,
    pub include_entity_refs: bool,
    pub link_depth: u32,
    pub min_link_confidence: f32,
}

/// Natural language search over embedded event content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticQuery {
    pub org_id: OrgId,
    pub query_text: String,
    pub entity: Option<(EntityType, EntityId)>,
    pub source: Option<Source>,
    pub time_range: Option<TimeRange>,
    pub limit: usize,
}

/// Traverse event links (causal chains).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQuery {
    pub org_id: OrgId,
    pub start_event_id: EventId,
    pub direction: LinkDirection,
    pub link_types: Option<Vec<String>>,
    pub max_depth: u32,
    pub min_confidence: f32,
}

/// A filter on a JSON payload field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadFilter {
    /// JSON path (e.g., "amount", "metadata.campaign_id").
    pub path: String,

    /// The comparison operator and value.
    pub op: FilterOp,
}

/// Comparison operators for payload filters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq(serde_json::Value),
    Ne(serde_json::Value),
    Gt(serde_json::Value),
    Gte(serde_json::Value),
    Lt(serde_json::Value),
    Lte(serde_json::Value),
    In(Vec<serde_json::Value>),
    IsNull,
    IsNotNull,
}

/// Grouping for aggregation queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    EntityType(EntityType),
    Source,
    EventType,
    PayloadField(String),
}

/// Ordering for result sets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderBy {
    EventTimeAsc,
    EventTimeDesc,
    IngestionTimeAsc,
    IngestionTimeDesc,
}

impl Default for OrderBy {
    fn default() -> Self {
        Self::EventTimeDesc
    }
}

/// The result of a query -- a single event with optional enrichments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventResult {
    /// The event data.
    pub event: crate::event::Event,

    /// Entity refs attached to this event (if requested).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_refs: Vec<crate::entity_ref::EntityRef>,

    /// Semantic search distance (if this came from a semantic query).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_distance: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_query_defaults() {
        let q = StructuredQuery {
            org_id: OrgId::new("org_1"),
            entity: None,
            source: Some(Source::new("stripe")),
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::default(),
            limit: 50,
            offset: 0,
        };
        assert!(matches!(q.order_by, OrderBy::EventTimeDesc));
    }

    #[test]
    fn payload_filter_serde() {
        let filter = PayloadFilter {
            path: "amount".to_string(),
            op: FilterOp::Gt(serde_json::json!(5000)),
        };
        let json = serde_json::to_string(&filter).unwrap();
        let parsed: PayloadFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.path, "amount");
    }

    #[test]
    fn timeline_query_construction() {
        let q = TimelineQuery {
            org_id: OrgId::new("org_1"),
            entity_type: EntityType::new("customer"),
            entity_id: EntityId::new("cust_123"),
            time_range: Some(TimeRange::last_days(30)),
            sources: None,
            include_linked: true,
            include_entity_refs: true,
            link_depth: 2,
            min_link_confidence: 0.7,
        };
        assert!(q.include_linked);
        assert_eq!(q.link_depth, 2);
    }
}
