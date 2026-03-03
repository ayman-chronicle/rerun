//! Sync entity extraction from event payloads.
//!
//! Inspects JSON payloads for known field patterns and extracts
//! entity references. For example, a Stripe webhook with a
//! `"customer": "cus_abc123"` field gets an entity ref of type
//! "customer" with id "cus_abc123".
//!
//! Extraction rules are configurable per (source, field_path).

use chronicle_core::event::PendingEntityRef;
use chronicle_core::ids::{EntityId, EntityType};

/// A rule that maps a JSON field to an entity type.
///
/// When the extractor finds a non-null value at `field_path` in an
/// event payload, it creates a `PendingEntityRef` with the given
/// `entity_type` and the field's string value as the entity ID.
#[derive(Debug, Clone)]
pub struct ExtractionRule {
    /// JSON field path (top-level only for now, e.g., "customer").
    pub field_path: String,

    /// The entity type to assign (e.g., "customer").
    pub entity_type: String,
}

/// Extract entity refs from a JSON payload using the given rules.
///
/// Returns refs for every rule that matches a non-null string value
/// in the payload.
pub fn extract_entities(
    payload: &serde_json::Value,
    rules: &[ExtractionRule],
) -> Vec<PendingEntityRef> {
    let obj = match payload.as_object() {
        Some(obj) => obj,
        None => return vec![],
    };

    rules
        .iter()
        .filter_map(|rule| {
            obj.get(&rule.field_path)
                .and_then(|v| v.as_str())
                .map(|id| PendingEntityRef {
                    entity_type: EntityType::new(&rule.entity_type),
                    entity_id: EntityId::new(id),
                })
        })
        .collect()
}

/// Default extraction rules for common SaaS sources.
pub fn default_stripe_rules() -> Vec<ExtractionRule> {
    vec![
        ExtractionRule {
            field_path: "customer".to_string(),
            entity_type: "customer".to_string(),
        },
        ExtractionRule {
            field_path: "subscription".to_string(),
            entity_type: "subscription".to_string(),
        },
        ExtractionRule {
            field_path: "invoice".to_string(),
            entity_type: "invoice".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_matching_fields() {
        let payload = serde_json::json!({
            "customer": "cus_123",
            "amount": 4999,
            "subscription": "sub_456",
        });

        let rules = default_stripe_rules();
        let refs = extract_entities(&payload, &rules);

        assert_eq!(refs.len(), 2);

        let customer = refs.iter().find(|r| r.entity_type == "customer").unwrap();
        assert_eq!(customer.entity_id.as_str(), "cus_123");

        let sub = refs.iter().find(|r| r.entity_type == "subscription").unwrap();
        assert_eq!(sub.entity_id.as_str(), "sub_456");
    }

    #[test]
    fn skip_missing_fields() {
        let payload = serde_json::json!({
            "amount": 4999,
        });

        let rules = default_stripe_rules();
        let refs = extract_entities(&payload, &rules);
        assert!(refs.is_empty());
    }

    #[test]
    fn skip_null_values() {
        let payload = serde_json::json!({
            "customer": null,
        });

        let rules = default_stripe_rules();
        let refs = extract_entities(&payload, &rules);
        assert!(refs.is_empty());
    }

    #[test]
    fn skip_non_string_values() {
        let payload = serde_json::json!({
            "customer": 12345,
        });

        let rules = default_stripe_rules();
        let refs = extract_entities(&payload, &rules);
        assert!(refs.is_empty(), "Non-string values should be skipped");
    }

    #[test]
    fn non_object_payload() {
        let payload = serde_json::json!("just a string");
        let refs = extract_entities(&payload, &default_stripe_rules());
        assert!(refs.is_empty());
    }
}
