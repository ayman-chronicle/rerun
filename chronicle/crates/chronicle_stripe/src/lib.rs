//! First-class Stripe connector for Chronicle.
//!
//! Implements [`SaasConnector`] for Stripe webhooks. Converts raw
//! webhook JSON into Chronicle [`Event`] objects with automatic entity
//! ref extraction, topic derivation, and full payload preservation.
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

use chronicle_core::connector::{ConnectorError, SaasConnector};
use chronicle_core::event::{Event, EventBuilder};

/// The Stripe webhook connector.
pub struct StripeConnector;

impl SaasConnector for StripeConnector {
    fn source_name(&self) -> &'static str {
        "stripe"
    }

    fn convert(&self, json: &str, org_id: &str) -> Result<Event, ConnectorError> {
        convert_webhook(json, org_id)
    }

    fn derive_topic(&self, event_type: &str) -> &'static str {
        derive_topic(event_type)
    }

    fn extract_entities(&self, data: &serde_json::Value) -> Vec<(String, String)> {
        extract_entities(data)
    }
}

/// Convenience: convert a raw Stripe webhook JSON body into a Chronicle [`Event`].
pub fn convert_webhook(json: &str, org_id: &str) -> Result<Event, ConnectorError> {
    let raw: serde_json::Value = serde_json::from_str(json)?;

    let event_type = raw["type"]
        .as_str()
        .ok_or_else(|| ConnectorError::MissingField("type".into()))?;

    let created = raw["created"]
        .as_i64()
        .ok_or_else(|| ConnectorError::MissingField("created".into()))?;

    let timestamp = chrono::DateTime::from_timestamp(created, 0)
        .ok_or_else(|| ConnectorError::InvalidTimestamp(created.to_string()))?;

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
pub fn convert_webhooks(jsons: &[&str], org_id: &str) -> Vec<Result<Event, ConnectorError>> {
    jsons
        .iter()
        .map(|json| convert_webhook(json, org_id))
        .collect()
}

// ---------------------------------------------------------------------------
// Topic derivation
// ---------------------------------------------------------------------------

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

pub fn extract_entities(data_object: &serde_json::Value) -> Vec<(String, String)> {
    let mut refs = Vec::new();

    extract_id(data_object, "customer", "customer", &mut refs);
    extract_id(data_object, "subscription", "subscription", &mut refs);
    extract_id(data_object, "invoice", "invoice", &mut refs);
    extract_id(data_object, "charge", "charge", &mut refs);
    extract_id(data_object, "payment_intent", "payment_intent", &mut refs);

    if let (Some(id), Some(object_type)) = (
        data_object["id"].as_str(),
        data_object["object"].as_str(),
    ) {
        let entity_type = match object_type {
            "charge" | "payment_intent" | "subscription" | "invoice" | "dispute" | "refund"
            | "customer" => object_type,
            _ => object_type,
        };
        if !refs.iter().any(|(t, i)| t == entity_type && i == id) {
            refs.push((entity_type.to_owned(), id.to_owned()));
        }
    }

    refs
}

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
        "id": "evt_1234", "type": "payment_intent.succeeded", "created": 1709481600,
        "data": {"object": {"id": "pi_abc123", "object": "payment_intent", "amount": 4999, "currency": "usd", "status": "succeeded", "customer": "cus_042", "payment_method": "pm_card_visa", "metadata": {}}}
    }"#;

    const CHARGE_FAILED: &str = r#"{
        "id": "evt_5678", "type": "charge.failed", "created": 1709568000,
        "data": {"object": {"id": "ch_fail1", "object": "charge", "amount": 2999, "currency": "usd", "status": "failed", "customer": "cus_042", "payment_intent": "pi_abc123", "failure_code": "card_declined"}}
    }"#;

    const SUBSCRIPTION_CREATED: &str = r#"{
        "id": "evt_sub1", "type": "customer.subscription.created", "created": 1709654400,
        "data": {"object": {"id": "sub_pro_monthly", "object": "subscription", "status": "active", "customer": "cus_042", "current_period_start": 1709654400, "current_period_end": 1712332800}}
    }"#;

    const INVOICE_PAID: &str = r#"{
        "id": "evt_inv1", "type": "invoice.paid", "created": 1709740800,
        "data": {"object": {"id": "in_monthly_001", "object": "invoice", "amount_due": 4999, "amount_paid": 4999, "currency": "usd", "status": "paid", "customer": "cus_042", "subscription": "sub_pro_monthly", "billing_reason": "subscription_cycle"}}
    }"#;

    const DISPUTE_CREATED: &str = r#"{
        "id": "evt_disp1", "type": "charge.dispute.created", "created": 1709827200,
        "data": {"object": {"id": "dp_fraud1", "object": "dispute", "amount": 4999, "currency": "usd", "status": "needs_response", "reason": "fraudulent", "charge": "ch_original"}}
    }"#;

    const UNKNOWN_EVENT: &str = r#"{
        "id": "evt_unk1", "type": "some.future.event_type", "created": 1709913600,
        "data": {"object": {"id": "obj_future", "object": "future_thing", "some_field": "some_value"}}
    }"#;

    #[test]
    fn topic_payments() {
        assert_eq!(derive_topic("charge.succeeded"), "payments");
        assert_eq!(derive_topic("payment_intent.succeeded"), "payments");
        assert_eq!(derive_topic("refund.created"), "payments");
        assert_eq!(derive_topic("checkout.session.completed"), "payments");
    }

    #[test]
    fn topic_subscriptions() {
        assert_eq!(derive_topic("customer.subscription.created"), "subscriptions");
    }

    #[test]
    fn topic_invoices() {
        assert_eq!(derive_topic("invoice.paid"), "invoices");
    }

    #[test]
    fn topic_disputes() {
        assert_eq!(derive_topic("charge.dispute.created"), "disputes");
    }

    #[test]
    fn topic_customers() {
        assert_eq!(derive_topic("customer.created"), "customers");
    }

    #[test]
    fn topic_unknown() {
        assert_eq!(derive_topic("some.future.event"), "other");
    }

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
    }

    #[test]
    fn extract_dispute_entities() {
        let obj: serde_json::Value = serde_json::from_str(DISPUTE_CREATED).unwrap();
        let entities = extract_entities(&obj["data"]["object"]);
        assert!(entities.iter().any(|(t, i)| t == "charge" && i == "ch_original"));
        assert!(entities.iter().any(|(t, i)| t == "dispute" && i == "dp_fraud1"));
    }

    #[test]
    fn convert_payment_intent() {
        let event = convert_webhook(PAYMENT_INTENT_SUCCEEDED, "org_1").unwrap();
        assert_eq!(event.source.as_str(), "stripe");
        assert_eq!(event.topic.as_str(), "payments");
        assert_eq!(event.event_type.as_str(), "payment_intent.succeeded");
        assert!(event.entity_refs.iter().any(|r| r.entity_type.as_str() == "customer" && r.entity_id.as_str() == "cus_042"));
    }

    #[test]
    fn convert_unknown_event_type() {
        let event = convert_webhook(UNKNOWN_EVENT, "org_1").unwrap();
        assert_eq!(event.topic.as_str(), "other");
    }

    #[test]
    fn convert_invalid_json() {
        assert!(convert_webhook("not json", "org_1").is_err());
    }

    #[test]
    fn convert_batch() {
        let results = convert_webhooks(&[PAYMENT_INTENT_SUCCEEDED, CHARGE_FAILED], "org_1");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn trait_convert_matches_free_function() {
        let connector = StripeConnector;
        let from_trait = connector.convert(PAYMENT_INTENT_SUCCEEDED, "org_1").unwrap();
        let from_fn = convert_webhook(PAYMENT_INTENT_SUCCEEDED, "org_1").unwrap();
        assert_eq!(from_trait.source.as_str(), from_fn.source.as_str());
        assert_eq!(from_trait.event_type.as_str(), from_fn.event_type.as_str());
        assert_eq!(from_trait.topic.as_str(), from_fn.topic.as_str());
    }

    #[test]
    fn expandable_customer_field() {
        let json = r#"{"id": "evt_exp", "type": "charge.succeeded", "created": 1709481600,
            "data": {"object": {"id": "ch_exp1", "object": "charge", "customer": {"id": "cus_expanded", "object": "customer"}}}}"#;
        let event = convert_webhook(json, "org_1").unwrap();
        assert!(event.entity_refs.iter().any(|r| r.entity_type.as_str() == "customer" && r.entity_id.as_str() == "cus_expanded"));
    }
}
