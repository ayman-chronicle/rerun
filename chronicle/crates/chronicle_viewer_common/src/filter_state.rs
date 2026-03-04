//! Chronicle filter state for the viewer.
//!
//! Manages available and active filter values across eight dimensions:
//! entity, source, event_type, topic, time_range, payload_text,
//! payload_field, and link_type. Supports client-side matching against
//! entity paths and JSON payloads, and can build a [`StructuredQuery`]
//! for backend execution.

use std::collections::BTreeSet;

use chronicle_core::ids::{EntityId, EntityType, EventType, OrgId, Source, Topic};
use chronicle_core::query::{FilterOp, OrderBy, PayloadFilter, StructuredQuery};

/// A single payload field filter (JSON path + operator).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldFilter {
    pub path: String,
    pub op: FilterOp,
}

/// Filter state for Chronicle data in the viewer.
///
/// All `active_*` sets use "empty = no filter = show all" semantics.
/// When a set is non-empty, only items in the set pass the filter.
#[derive(Debug, Clone, Default)]
pub struct ChronicleFilterState {
    // --- Discovered values (populated by `discover`) ---
    pub available_sources: BTreeSet<String>,
    pub available_event_types: BTreeSet<String>,
    pub available_topics: BTreeSet<String>,
    pub available_link_types: BTreeSet<String>,
    pub available_entity_types: BTreeSet<String>,
    pub available_entity_ids: BTreeSet<String>,

    // --- User-selected filters ---
    pub active_sources: BTreeSet<String>,
    pub active_event_types: BTreeSet<String>,
    pub active_topics: BTreeSet<String>,
    pub active_link_types: BTreeSet<String>,
    pub entity_type_filter: String,
    pub entity_id_filter: String,
    pub payload_text: String,
    pub field_filters: Vec<FieldFilter>,
}

impl ChronicleFilterState {
    /// Populate available values from entity paths and link paths.
    ///
    /// Entity paths follow `{source}/{event_type}` (e.g., `stripe/charge.created`).
    /// Link paths follow `_links/…/{link_type}` (last segment is the link type).
    /// Entity types/IDs are extracted from `_entity_refs` in payload JSON strings.
    pub fn discover<'a>(
        &mut self,
        entity_paths: impl Iterator<Item = &'a str>,
        payload_jsons: impl Iterator<Item = &'a str>,
    ) {
        self.available_sources.clear();
        self.available_event_types.clear();
        self.available_link_types.clear();
        self.available_entity_types.clear();
        self.available_entity_ids.clear();

        for path in entity_paths {
            let clean = path.strip_prefix('/').unwrap_or(path);

            if let Some(remainder) = clean.strip_prefix("_links/") {
                if let Some(link_type) = remainder.rsplit('/').next() {
                    self.available_link_types.insert(link_type.to_owned());
                }
                continue;
            }

            if clean.contains("payload") || clean.is_empty() {
                continue;
            }

            let mut parts = clean.splitn(2, '/');
            if let Some(source) = parts.next() {
                self.available_sources.insert(source.to_owned());
                if let Some(event_type) = parts.next() {
                    self.available_event_types.insert(event_type.to_owned());
                }
            }
        }

        for json_str in payload_jsons {
            Self::extract_entity_refs(
                json_str,
                &mut self.available_entity_types,
                &mut self.available_entity_ids,
            );
        }
    }

    /// Populate available topics from an iterator of topic strings.
    pub fn discover_topics<'a>(&mut self, topics: impl Iterator<Item = &'a str>) {
        self.available_topics.clear();
        for topic in topics {
            if !topic.is_empty() {
                self.available_topics.insert(topic.to_owned());
            }
        }
    }

    fn extract_entity_refs(
        json_str: &str,
        entity_types: &mut BTreeSet<String>,
        entity_ids: &mut BTreeSet<String>,
    ) {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) else {
            return;
        };
        let Some(refs) = val.get("_entity_refs").and_then(|v| v.as_array()) else {
            return;
        };
        for r in refs {
            if let Some(t) = r.get("type").and_then(|v| v.as_str()) {
                entity_types.insert(t.to_owned());
            }
            if let Some(id) = r.get("id").and_then(|v| v.as_str()) {
                entity_ids.insert(id.to_owned());
            }
        }
    }

    /// Returns `true` when any filter is active.
    pub fn is_active(&self) -> bool {
        !self.active_sources.is_empty()
            || !self.active_event_types.is_empty()
            || !self.active_topics.is_empty()
            || !self.active_link_types.is_empty()
            || !self.entity_type_filter.is_empty()
            || !self.entity_id_filter.is_empty()
            || !self.payload_text.is_empty()
            || !self.field_filters.is_empty()
    }

    /// Reset all filters to "show all".
    pub fn clear(&mut self) {
        self.active_sources.clear();
        self.active_event_types.clear();
        self.active_topics.clear();
        self.active_link_types.clear();
        self.entity_type_filter.clear();
        self.entity_id_filter.clear();
        self.payload_text.clear();
        self.field_filters.clear();
    }

    /// Check whether an entity path passes the source + event_type filters.
    ///
    /// Path format: `{source}/{event_type}` (leading `/` is stripped).
    pub fn matches_path(&self, path: &str) -> bool {
        let clean = path.strip_prefix('/').unwrap_or(path);
        let mut parts = clean.splitn(2, '/');
        let source = parts.next().unwrap_or("");
        let event_type = parts.next().unwrap_or("");

        if !self.active_sources.is_empty() && !self.active_sources.contains(source) {
            return false;
        }
        if !self.active_event_types.is_empty() && !self.active_event_types.contains(event_type) {
            return false;
        }
        true
    }

    /// Check whether a link type passes the link_type filter.
    pub fn matches_link_type(&self, link_type: &str) -> bool {
        self.active_link_types.is_empty() || self.active_link_types.contains(link_type)
    }

    /// Check whether a payload JSON string passes the text search
    /// and entity filters.
    pub fn matches_payload(&self, payload_json: &str) -> bool {
        if !self.payload_text.is_empty() {
            let lower = payload_json.to_ascii_lowercase();
            if !lower.contains(&self.payload_text.to_ascii_lowercase()) {
                return false;
            }
        }

        if !self.entity_type_filter.is_empty() || !self.entity_id_filter.is_empty() {
            if !self.payload_has_entity(payload_json) {
                return false;
            }
        }

        true
    }

    fn payload_has_entity(&self, json_str: &str) -> bool {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) else {
            return false;
        };
        let Some(refs) = val.get("_entity_refs").and_then(|v| v.as_array()) else {
            return false;
        };
        refs.iter().any(|r| {
            let type_ok = self.entity_type_filter.is_empty()
                || r.get("type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t == self.entity_type_filter);
            let id_ok = self.entity_id_filter.is_empty()
                || r.get("id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id == self.entity_id_filter);
            type_ok && id_ok
        })
    }

    /// Build a [`StructuredQuery`] from the current filter state.
    ///
    /// Useful for future backend query execution. The `org_id` must be
    /// provided since it is not part of the viewer filter UI.
    pub fn build_query(&self, org_id: &str) -> StructuredQuery {
        let source = self
            .active_sources
            .iter()
            .next()
            .map(|s| Source::new(s.as_str()));

        let event_type = self
            .active_event_types
            .iter()
            .next()
            .map(|t| EventType::new(t.as_str()));

        let topic = self
            .active_topics
            .iter()
            .next()
            .map(|t| Topic::new(t.as_str()));

        let entity = if !self.entity_type_filter.is_empty() && !self.entity_id_filter.is_empty() {
            Some((
                EntityType::new(&self.entity_type_filter),
                EntityId::new(&self.entity_id_filter),
            ))
        } else {
            None
        };

        let payload_filters: Vec<PayloadFilter> = self
            .field_filters
            .iter()
            .map(|f| PayloadFilter {
                path: f.path.clone(),
                op: f.op.clone(),
            })
            .collect();

        StructuredQuery {
            org_id: OrgId::new(org_id),
            entity,
            source,
            topic,
            event_type,
            time_range: None,
            payload_filters,
            group_by: None,
            order_by: OrderBy::EventTimeAsc,
            limit: 100_000,
            offset: 0,
        }
    }

    /// Toggle a source in the active set.
    pub fn toggle_source(&mut self, source: &str) {
        if !self.active_sources.remove(source) {
            self.active_sources.insert(source.to_owned());
        }
    }

    /// Toggle an event type in the active set.
    pub fn toggle_event_type(&mut self, event_type: &str) {
        if !self.active_event_types.remove(event_type) {
            self.active_event_types.insert(event_type.to_owned());
        }
    }

    /// Toggle a link type in the active set.
    pub fn toggle_link_type(&mut self, link_type: &str) {
        if !self.active_link_types.remove(link_type) {
            self.active_link_types.insert(link_type.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_paths() -> Vec<&'static str> {
        vec![
            "/stripe/charge.created",
            "/stripe/payment_intent.succeeded",
            "/stripe/payment_intent.failed",
            "/support/ticket.created",
            "/support/ticket.closed",
            "/billing/invoice.created",
            "/billing/invoice.overdue",
            "/marketing/email.clicked",
            "/product/page.viewed",
            "/stripe/charge.created/payload",
            "/_links/stripe/payment_intent.failed/123/to/support/ticket.created/456/caused_by",
            "/_links/support/ticket.escalated/789/to/stripe/subscription.cancelled/101/led_to",
        ]
    }

    fn sample_payloads() -> Vec<&'static str> {
        vec![
            r#"{"amount":4999,"_entity_refs":[{"type":"customer","id":"cust_001"},{"type":"account","id":"acc_1"}]}"#,
            r#"{"subject":"Help","_entity_refs":[{"type":"customer","id":"cust_001"}]}"#,
            r#"{"amount":100,"_entity_refs":[{"type":"customer","id":"cust_002"}]}"#,
            r#"{"amount":200}"#,
        ]
    }

    #[test]
    fn discover_extracts_sources_and_types() {
        let mut state = ChronicleFilterState::default();
        state.discover(sample_paths().into_iter(), std::iter::empty());

        assert!(state.available_sources.contains("stripe"));
        assert!(state.available_sources.contains("support"));
        assert!(state.available_sources.contains("billing"));
        assert!(state.available_sources.contains("marketing"));
        assert!(state.available_sources.contains("product"));
        assert_eq!(state.available_sources.len(), 5);

        assert!(state.available_event_types.contains("charge.created"));
        assert!(state.available_event_types.contains("ticket.created"));
        assert!(state.available_event_types.contains("invoice.overdue"));
    }

    #[test]
    fn discover_extracts_link_types() {
        let mut state = ChronicleFilterState::default();
        state.discover(sample_paths().into_iter(), std::iter::empty());

        assert!(state.available_link_types.contains("caused_by"));
        assert!(state.available_link_types.contains("led_to"));
        assert_eq!(state.available_link_types.len(), 2);
    }

    #[test]
    fn discover_extracts_entity_refs_from_payloads() {
        let mut state = ChronicleFilterState::default();
        state.discover(std::iter::empty(), sample_payloads().into_iter());

        assert!(state.available_entity_types.contains("customer"));
        assert!(state.available_entity_types.contains("account"));
        assert!(state.available_entity_ids.contains("cust_001"));
        assert!(state.available_entity_ids.contains("cust_002"));
        assert!(state.available_entity_ids.contains("acc_1"));
    }

    #[test]
    fn discover_skips_payload_and_empty_paths() {
        let mut state = ChronicleFilterState::default();
        state.discover(
            vec!["/stripe/charge.created/payload", "", "/"].into_iter(),
            std::iter::empty(),
        );

        assert!(state.available_sources.is_empty());
    }

    #[test]
    fn matches_path_no_filters() {
        let state = ChronicleFilterState::default();
        assert!(state.matches_path("/stripe/charge.created"));
        assert!(state.matches_path("support/ticket.created"));
    }

    #[test]
    fn matches_path_source_filter() {
        let mut state = ChronicleFilterState::default();
        state.active_sources.insert("stripe".to_owned());

        assert!(state.matches_path("/stripe/charge.created"));
        assert!(state.matches_path("stripe/payment_intent.failed"));
        assert!(!state.matches_path("/support/ticket.created"));
        assert!(!state.matches_path("billing/invoice.overdue"));
    }

    #[test]
    fn matches_path_event_type_filter() {
        let mut state = ChronicleFilterState::default();
        state
            .active_event_types
            .insert("charge.created".to_owned());

        assert!(state.matches_path("/stripe/charge.created"));
        assert!(!state.matches_path("/stripe/payment_intent.failed"));
    }

    #[test]
    fn matches_path_combined_source_and_type() {
        let mut state = ChronicleFilterState::default();
        state.active_sources.insert("stripe".to_owned());
        state
            .active_event_types
            .insert("charge.created".to_owned());

        assert!(state.matches_path("/stripe/charge.created"));
        assert!(!state.matches_path("/stripe/payment_intent.failed"));
        assert!(!state.matches_path("/support/charge.created"));
    }

    #[test]
    fn matches_link_type_no_filter() {
        let state = ChronicleFilterState::default();
        assert!(state.matches_link_type("caused_by"));
        assert!(state.matches_link_type("anything"));
    }

    #[test]
    fn matches_link_type_with_filter() {
        let mut state = ChronicleFilterState::default();
        state.active_link_types.insert("caused_by".to_owned());

        assert!(state.matches_link_type("caused_by"));
        assert!(!state.matches_link_type("led_to"));
    }

    #[test]
    fn matches_payload_text_search() {
        let mut state = ChronicleFilterState::default();
        state.payload_text = "card_declined".to_owned();

        assert!(state.matches_payload(r#"{"failure_code":"card_declined"}"#));
        assert!(state.matches_payload(r#"{"msg":"CARD_DECLINED"}"#));
        assert!(!state.matches_payload(r#"{"failure_code":"insufficient_funds"}"#));
    }

    #[test]
    fn matches_payload_entity_filter() {
        let mut state = ChronicleFilterState::default();
        state.entity_type_filter = "customer".to_owned();
        state.entity_id_filter = "cust_001".to_owned();

        assert!(state.matches_payload(
            r#"{"amount":100,"_entity_refs":[{"type":"customer","id":"cust_001"}]}"#
        ));
        assert!(!state.matches_payload(
            r#"{"amount":100,"_entity_refs":[{"type":"customer","id":"cust_002"}]}"#
        ));
        assert!(!state.matches_payload(r#"{"amount":100}"#));
    }

    #[test]
    fn matches_payload_entity_type_only() {
        let mut state = ChronicleFilterState::default();
        state.entity_type_filter = "customer".to_owned();

        assert!(state.matches_payload(
            r#"{"_entity_refs":[{"type":"customer","id":"cust_001"}]}"#
        ));
        assert!(state.matches_payload(
            r#"{"_entity_refs":[{"type":"customer","id":"cust_999"}]}"#
        ));
        assert!(!state.matches_payload(
            r#"{"_entity_refs":[{"type":"account","id":"acc_1"}]}"#
        ));
    }

    #[test]
    fn toggle_source() {
        let mut state = ChronicleFilterState::default();
        state.toggle_source("stripe");
        assert!(state.active_sources.contains("stripe"));
        state.toggle_source("stripe");
        assert!(!state.active_sources.contains("stripe"));
    }

    #[test]
    fn is_active_and_clear() {
        let mut state = ChronicleFilterState::default();
        assert!(!state.is_active());

        state.active_sources.insert("stripe".to_owned());
        assert!(state.is_active());

        state.clear();
        assert!(!state.is_active());
    }

    #[test]
    fn build_query_produces_valid_structured_query() {
        let mut state = ChronicleFilterState::default();
        state.active_sources.insert("stripe".to_owned());
        state.entity_type_filter = "customer".to_owned();
        state.entity_id_filter = "cust_001".to_owned();
        state.field_filters.push(FieldFilter {
            path: "amount".to_owned(),
            op: FilterOp::Gt(serde_json::json!(5000)),
        });

        let q = state.build_query("org_1");

        assert_eq!(q.org_id.as_str(), "org_1");
        assert_eq!(q.source.as_ref().unwrap().as_str(), "stripe");
        assert!(q.entity.is_some());
        let (et, eid) = q.entity.unwrap();
        assert_eq!(et.as_str(), "customer");
        assert_eq!(eid.as_str(), "cust_001");
        assert_eq!(q.payload_filters.len(), 1);
        assert_eq!(q.payload_filters[0].path, "amount");
    }

    #[test]
    fn build_query_empty_filters() {
        let state = ChronicleFilterState::default();
        let q = state.build_query("org_1");

        assert!(q.source.is_none());
        assert!(q.entity.is_none());
        assert!(q.event_type.is_none());
        assert!(q.topic.is_none());
        assert!(q.payload_filters.is_empty());
    }
}
