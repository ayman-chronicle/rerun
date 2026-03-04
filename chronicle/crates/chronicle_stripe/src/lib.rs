//! First-class Stripe connector for Chronicle.
//!
//! Converts raw Stripe webhook JSON into Chronicle [`Event`] objects with
//! automatic entity ref extraction, topic derivation, and full payload
//! preservation.
//!
//! # Usage
//!
//! ```ignore
//! use chronicle_stripe::convert_webhook;
//!
//! let event = convert_webhook(webhook_json, "my_org")?;
//! // event.source == "stripe"
//! // event.topic == "payments"
//! // event.event_type == "payment_intent.succeeded"
//! // event.entity_refs contains ("customer", "cus_xxx")
//! ```
//!
//! # Design
//!
//! Uses plain `serde_json` for parsing — no heavy Stripe SDK dependency.
//! Forward-compatible with any Stripe API version: unknown event types
//! still produce valid Chronicle events (entity extraction returns empty
//! refs for unrecognized object shapes).

use chronicle_core::event::{Event, EventBuilder};

/// Errors from Stripe webhook conversion.
#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    #[error("Invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("Missing field: {0}")]
    MissingField(&'static str),

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(i64),
}

/// Convert a raw Stripe webhook JSON body into a Chronicle [`Event`].
///
/// Extracts entity refs (customer, subscription, invoice, charge IDs),
/// derives the topic from the event type, and preserves the full payload.
pub fn convert_webhook(json: &str, org_id: &str) -> Result<Event, StripeError> {
    let raw: serde_json::Value = serde_json::from_str(json)?;

    let event_type = raw["type"]
        .as_str()
        .ok_or(StripeError::MissingField("type"))?;

    let created = raw["created"]
        .as_i64()
        .ok_or(StripeError::MissingField("created"))?;

    let timestamp = chrono::DateTime::from_timestamp(created, 0)
        .ok_or(StripeError::InvalidTimestamp(created))?;

    let data_object = &raw["data"]["object"];

    let topic = derive_topic(event_type);
    let entities = extract_entities(data_object);

    let mut builder = EventBuilder::new(org_id, "stripe", topic, event_type)
        .event_time(timestamp)
        .raw_body(json.to_owned());

    if !data_object.is_null() {
        builder = builder.payload(data_object.clone());
    }

    for (etype, eid) in entities {
        builder = builder.entity(etype.as_str(), eid);
    }

    Ok(builder.build())
}

/// Batch-convert multiple Stripe webhook JSON bodies.
pub fn convert_webhooks(jsons: &[&str], org_id: &str) -> Vec<Result<Event, StripeError>> {
    jsons.iter().map(|json| convert_webhook(json, org_id)).collect()
}

// ---------------------------------------------------------------------------
// Topic derivation
// ---------------------------------------------------------------------------

/// Map a Stripe event type string to a Chronicle topic.
///
/// Uses prefix matching on the Stripe event type hierarchy:
/// - `charge.dispute.*` -> `disputes`
/// - `charge.*`, `payment_intent.*`, `refund.*` -> `payments`
/// - `customer.subscription.*` -> `subscriptions`
/// - `invoice.*` -> `invoices`
/// - `customer.*` -> `customers`
/// - everything else -> `other`
pub fn derive_topic(event_type: &str) -> &'static str {
    if event_type.starts_with("charge.dispute") {
        return "disputes";
    }
    if event_type.starts_with("charge")
        || event_type.starts_with("payment_intent")
        || event_type.starts_with("refund")
        || event_type.starts_with("checkout")
    {
        return "payments";
    }
    if event_type.starts_with("customer.subscription") {
        return "subscriptions";
    }
    if event_type.starts_with("invoice") {
        return "invoices";
    }
    if event_type.starts_with("customer") {
        return "customers";
    }
    if event_type.starts_with("product") || event_type.starts_with("price") {
        return "catalog";
    }
    "other"
}

// ---------------------------------------------------------------------------
// Entity extraction
// ---------------------------------------------------------------------------

/// Extract entity references from a Stripe `data.object` JSON value.
///
/// Pulls well-known ID fields (`customer`, `subscription`, `invoice`,
/// `charge`, `payment_intent`) and returns them as `(entity_type, entity_id)`
/// pairs for Chronicle entity refs.
pub fn extract_entities(data_object: &serde_json::Value) -> Vec<(String, String)> {
    let mut refs = Vec::new();

    extract_id(data_object, "customer", "customer", &mut refs);
    extract_id(data_object, "subscription", "subscription", &mut refs);
    extract_id(data_object, "invoice", "invoice", &mut refs);
    extract_id(data_object, "charge", "charge", &mut refs);
    extract_id(data_object, "payment_intent", "payment_intent", &mut refs);

    // The object itself has an ID — use the object type as the entity type.
    if let (Some(id), Some(object_type)) = (
        data_object["id"].as_str(),
        data_object["object"].as_str(),
    ) {
        let entity_type = match object_type {
            "charge" => "charge",
            "payment_intent" => "payment_intent",
            "subscription" => "subscription",
            "invoice" => "invoice",
            "dispute" => "dispute",
            "refund" => "refund",
            "customer" => "customer",
            _ => object_type,
        };
        if !refs.iter().any(|(t, i)| t == entity_type && i == id) {
            refs.push((entity_type.to_owned(), id.to_owned()));
        }
    }

    refs
}

/// Extract a string ID field from a JSON object. Handles both string
/// values and objects with an `id` sub-field (Stripe's expandable pattern).
fn extract_id(
    obj: &serde_json::Value,
    field: &str,
    entity_type: &str,
    refs: &mut Vec<(String, String)>,
) {
    let val = &obj[field];
    let id = if let Some(s) = val.as_str() {
        Some(s.to_owned())
    } else if val.is_object() {
        val["id"].as_str().map(str::to_owned)
    } else {
        None
    };

    if let Some(id) = id {
        if !id.is_empty() {
            refs.push((entity_type.to_owned(), id));
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const PAYMENT_INTENT_SUCCEEDED: &str = r#"{
        "id": "evt_1234",
        "type": "payment_intent.succeeded",
        "created": 1709481600,
        "data": {
            "object": {
                "id": "pi_abc123",
                "object": "payment_intent",
                "amount": 4999,
                "currency": "usd",
                "status": "succeeded",
                "customer": "cus_042",
                "payment_method": "pm_card_visa",
                "metadata": {}
            }
        }
    }"#;

    const CHARGE_FAILED: &str = r#"{
        "id": "evt_5678",
        "type": "charge.failed",
        "created": 1709568000,
        "data": {
            "object": {
                "id": "ch_fail1",
                "object": "charge",
                "amount": 2999,
                "currency": "usd",
                "status": "failed",
                "customer": "cus_042",
                "payment_intent": "pi_abc123",
                "failure_code": "card_declined",
                "failure_message": "Your card was declined"
            }
        }
    }"#;

    const SUBSCRIPTION_CREATED: &str = r#"{
        "id": "evt_sub1",
        "type": "customer.subscription.created",
        "created": 1709654400,
        "data": {
            "object": {
                "id": "sub_pro_monthly",
                "object": "subscription",
                "status": "active",
                "customer": "cus_042",
                "current_period_start": 1709654400,
                "current_period_end": 1712332800,
                "cancel_at_period_end": false
            }
        }
    }"#;

    const INVOICE_PAID: &str = r#"{
        "id": "evt_inv1",
        "type": "invoice.paid",
        "created": 1709740800,
        "data": {
            "object": {
                "id": "in_monthly_001",
                "object": "invoice",
                "amount_due": 4999,
                "amount_paid": 4999,
                "currency": "usd",
                "status": "paid",
                "customer": "cus_042",
                "subscription": "sub_pro_monthly",
                "billing_reason": "subscription_cycle"
            }
        }
    }"#;

    const DISPUTE_CREATED: &str = r#"{
        "id": "evt_disp1",
        "type": "charge.dispute.created",
        "created": 1709827200,
        "data": {
            "object": {
                "id": "dp_fraud1",
                "object": "dispute",
                "amount": 4999,
                "currency": "usd",
                "status": "needs_response",
                "reason": "fraudulent",
                "charge": "ch_original"
            }
        }
    }"#;

    const UNKNOWN_EVENT: &str = r#"{
        "id": "evt_unk1",
        "type": "some.future.event_type",
        "created": 1709913600,
        "data": {
            "object": {
                "id": "obj_future",
                "object": "future_thing",
                "some_field": "some_value"
            }
        }
    }"#;

    // --- Topic derivation ---

    #[test]
    fn topic_payments() {
        assert_eq!(derive_topic("charge.succeeded"), "payments");
        assert_eq!(derive_topic("charge.failed"), "payments");
        assert_eq!(derive_topic("charge.refunded"), "payments");
        assert_eq!(derive_topic("payment_intent.succeeded"), "payments");
        assert_eq!(derive_topic("payment_intent.payment_failed"), "payments");
        assert_eq!(derive_topic("refund.created"), "payments");
        assert_eq!(derive_topic("checkout.session.completed"), "payments");
    }

    #[test]
    fn topic_subscriptions() {
        assert_eq!(derive_topic("customer.subscription.created"), "subscriptions");
        assert_eq!(derive_topic("customer.subscription.updated"), "subscriptions");
        assert_eq!(derive_topic("customer.subscription.deleted"), "subscriptions");
    }

    #[test]
    fn topic_invoices() {
        assert_eq!(derive_topic("invoice.created"), "invoices");
        assert_eq!(derive_topic("invoice.paid"), "invoices");
        assert_eq!(derive_topic("invoice.payment_failed"), "invoices");
    }

    #[test]
    fn topic_disputes() {
        assert_eq!(derive_topic("charge.dispute.created"), "disputes");
        assert_eq!(derive_topic("charge.dispute.closed"), "disputes");
    }

    #[test]
    fn topic_customers() {
        assert_eq!(derive_topic("customer.created"), "customers");
        assert_eq!(derive_topic("customer.updated"), "customers");
    }

    #[test]
    fn topic_unknown() {
        assert_eq!(derive_topic("some.future.event"), "other");
    }

    // --- Entity extraction ---

    #[test]
    fn extract_payment_intent_entities() {
        let obj: serde_json::Value = serde_json::from_str(PAYMENT_INTENT_SUCCEEDED).unwrap();
        let entities = extract_entities(&obj["data"]["object"]);

        assert!(entities.iter().any(|(t, i)| t == "customer" && i == "cus_042"));
        assert!(entities.iter().any(|(t, i)| t == "payment_intent" && i == "pi_abc123"));
    }

    #[test]
    fn extract_charge_entities() {
        let obj: serde_json::Value = serde_json::from_str(CHARGE_FAILED).unwrap();
        let entities = extract_entities(&obj["data"]["object"]);

        assert!(entities.iter().any(|(t, i)| t == "customer" && i == "cus_042"));
        assert!(entities.iter().any(|(t, i)| t == "payment_intent" && i == "pi_abc123"));
        assert!(entities.iter().any(|(t, i)| t == "charge" && i == "ch_fail1"));
    }

    #[test]
    fn extract_subscription_entities() {
        let obj: serde_json::Value = serde_json::from_str(SUBSCRIPTION_CREATED).unwrap();
        let entities = extract_entities(&obj["data"]["object"]);

        assert!(entities.iter().any(|(t, i)| t == "customer" && i == "cus_042"));
        assert!(entities.iter().any(|(t, i)| t == "subscription" && i == "sub_pro_monthly"));
    }

    #[test]
    fn extract_invoice_entities() {
        let obj: serde_json::Value = serde_json::from_str(INVOICE_PAID).unwrap();
        let entities = extract_entities(&obj["data"]["object"]);

        assert!(entities.iter().any(|(t, i)| t == "customer" && i == "cus_042"));
        assert!(entities.iter().any(|(t, i)| t == "subscription" && i == "sub_pro_monthly"));
        assert!(entities.iter().any(|(t, i)| t == "invoice" && i == "in_monthly_001"));
    }

    #[test]
    fn extract_dispute_entities() {
        let obj: serde_json::Value = serde_json::from_str(DISPUTE_CREATED).unwrap();
        let entities = extract_entities(&obj["data"]["object"]);

        assert!(entities.iter().any(|(t, i)| t == "charge" && i == "ch_original"));
        assert!(entities.iter().any(|(t, i)| t == "dispute" && i == "dp_fraud1"));
    }

    // --- Full webhook conversion ---

    #[test]
    fn convert_payment_intent() {
        let event = convert_webhook(PAYMENT_INTENT_SUCCEEDED, "org_1").unwrap();

        assert_eq!(event.source.as_str(), "stripe");
        assert_eq!(event.topic.as_str(), "payments");
        assert_eq!(event.event_type.as_str(), "payment_intent.succeeded");
        assert_eq!(event.org_id.as_str(), "org_1");
        assert!(event.payload.is_some());
        assert_eq!(event.payload.as_ref().unwrap()["amount"], 4999);
        assert!(event.entity_refs.iter().any(|r| r.entity_type.as_str() == "customer" && r.entity_id.as_str() == "cus_042"));
        assert!(event.raw_body.is_some());
    }

    #[test]
    fn convert_charge_failed() {
        let event = convert_webhook(CHARGE_FAILED, "org_1").unwrap();

        assert_eq!(event.topic.as_str(), "payments");
        assert_eq!(event.event_type.as_str(), "charge.failed");
        assert_eq!(event.payload.as_ref().unwrap()["failure_code"], "card_declined");
    }

    #[test]
    fn convert_subscription() {
        let event = convert_webhook(SUBSCRIPTION_CREATED, "org_1").unwrap();

        assert_eq!(event.topic.as_str(), "subscriptions");
        assert_eq!(event.event_type.as_str(), "customer.subscription.created");
        assert!(event.entity_refs.iter().any(|r| r.entity_type.as_str() == "subscription"));
    }

    #[test]
    fn convert_invoice() {
        let event = convert_webhook(INVOICE_PAID, "org_1").unwrap();

        assert_eq!(event.topic.as_str(), "invoices");
        assert_eq!(event.event_type.as_str(), "invoice.paid");
        assert_eq!(event.payload.as_ref().unwrap()["billing_reason"], "subscription_cycle");
    }

    #[test]
    fn convert_dispute() {
        let event = convert_webhook(DISPUTE_CREATED, "org_1").unwrap();

        assert_eq!(event.topic.as_str(), "disputes");
        assert_eq!(event.event_type.as_str(), "charge.dispute.created");
    }

    #[test]
    fn convert_unknown_event_type() {
        let event = convert_webhook(UNKNOWN_EVENT, "org_1").unwrap();

        assert_eq!(event.topic.as_str(), "other");
        assert_eq!(event.event_type.as_str(), "some.future.event_type");
        assert!(event.payload.is_some());
    }

    #[test]
    fn convert_batch() {
        let results = convert_webhooks(
            &[PAYMENT_INTENT_SUCCEEDED, CHARGE_FAILED, SUBSCRIPTION_CREATED],
            "org_1",
        );

        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn convert_invalid_json() {
        let result = convert_webhook("not json", "org_1");
        assert!(result.is_err());
    }

    #[test]
    fn convert_missing_type_field() {
        let result = convert_webhook(r#"{"created": 123, "data": {"object": {}}}"#, "org_1");
        assert!(result.is_err());
    }

    #[test]
    fn event_time_is_from_created() {
        let event = convert_webhook(PAYMENT_INTENT_SUCCEEDED, "org_1").unwrap();
        assert_eq!(event.event_time.timestamp(), 1709481600);
    }

    #[test]
    fn expandable_customer_field() {
        let json = r#"{
            "id": "evt_exp",
            "type": "charge.succeeded",
            "created": 1709481600,
            "data": {
                "object": {
                    "id": "ch_exp1",
                    "object": "charge",
                    "customer": {"id": "cus_expanded", "object": "customer", "name": "Jane"}
                }
            }
        }"#;

        let event = convert_webhook(json, "org_1").unwrap();
        assert!(event.entity_refs.iter().any(|r| r.entity_type.as_str() == "customer" && r.entity_id.as_str() == "cus_expanded"));
    }
}
