//! Gorgias helpdesk connector for Chronicle.
//!
//! Implements [`SaasConnector`] for Gorgias webhook events. Converts
//! raw webhook JSON into Chronicle [`Event`] objects with automatic
//! entity ref extraction (ticket, customer, agent) and topic derivation.
//!
//! # Gorgias event structure
//!
//! ```json
//! {
//!   "id": 1234,
//!   "type": "ticket-created",
//!   "object_id": 123,
//!   "object_type": "Ticket",
//!   "created_datetime": "2019-11-16T15:59:41.966927",
//!   "user_id": 123,
//!   "context": "uuid4",
//!   "data": {}
//! }
//! ```

use chronicle_core::connector::{ConnectorError, SaasConnector};
use chronicle_core::event::{Event, EventBuilder};

/// The Gorgias webhook connector.
pub struct GorgiasConnector;

impl SaasConnector for GorgiasConnector {
    fn source_name(&self) -> &'static str {
        "gorgias"
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

/// Convert a raw Gorgias webhook JSON body into a Chronicle [`Event`].
pub fn convert_webhook(json: &str, org_id: &str) -> Result<Event, ConnectorError> {
    let raw: serde_json::Value = serde_json::from_str(json)?;

    let event_type = raw["type"]
        .as_str()
        .ok_or_else(|| ConnectorError::MissingField("type".into()))?;

    let created_str = raw["created_datetime"]
        .as_str()
        .ok_or_else(|| ConnectorError::MissingField("created_datetime".into()))?;

    let timestamp = chrono::NaiveDateTime::parse_from_str(created_str, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(created_str, "%Y-%m-%dT%H:%M:%S"))
        .map(|naive| naive.and_utc())
        .or_else(|_| {
            chrono::DateTime::parse_from_rfc3339(created_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
        })
        .map_err(|_| ConnectorError::InvalidTimestamp(created_str.into()))?;

    let topic = derive_topic(event_type);
    let entities = extract_entities(&raw);

    let mut builder = EventBuilder::new(org_id, "gorgias", topic, event_type)
        .event_time(timestamp)
        .raw_body(json.to_owned());

    let data = &raw["data"];
    if !data.is_null() {
        builder = builder.payload(data.clone());
    }

    for (etype, eid) in entities {
        builder = builder.entity(etype.as_str(), eid);
    }

    Ok(builder.build())
}

/// Batch-convert multiple Gorgias webhook JSON bodies.
pub fn convert_webhooks(jsons: &[&str], org_id: &str) -> Vec<Result<Event, ConnectorError>> {
    jsons
        .iter()
        .map(|json| convert_webhook(json, org_id))
        .collect()
}

// ---------------------------------------------------------------------------
// Topic derivation
// ---------------------------------------------------------------------------

/// Map a Gorgias event type to a Chronicle topic.
pub fn derive_topic(event_type: &str) -> &'static str {
    if event_type.starts_with("ticket-message") {
        return "messages";
    }
    if event_type.starts_with("ticket") {
        return "tickets";
    }
    if event_type.starts_with("customer") {
        return "customers";
    }
    if event_type.starts_with("user") {
        return "agents";
    }
    if event_type.starts_with("satisfaction-survey") {
        return "csat";
    }
    if event_type.starts_with("tag") {
        return "tags";
    }
    if event_type.starts_with("rule") || event_type.starts_with("macro") {
        return "automation";
    }
    if event_type.starts_with("facebook") || event_type.starts_with("instagram") {
        return "social";
    }
    if event_type.starts_with("integration") {
        return "integrations";
    }
    "other"
}

// ---------------------------------------------------------------------------
// Entity extraction
// ---------------------------------------------------------------------------

/// Extract entity references from a Gorgias webhook event.
///
/// Uses `object_type` + `object_id` for the primary entity, and
/// scans `data` for nested IDs (`customer_id`, `ticket_id`, `user_id`).
pub fn extract_entities(event: &serde_json::Value) -> Vec<(String, String)> {
    let mut refs = Vec::new();

    if let (Some(object_type), Some(object_id)) = (
        event["object_type"].as_str(),
        event["object_id"].as_i64(),
    ) {
        let entity_type = match object_type {
            "Ticket" => "ticket",
            "TicketMessage" => "message",
            "Customer" => "customer",
            "User" => "agent",
            "Tag" => "tag",
            "SatisfactionSurvey" => "survey",
            "Rule" => "rule",
            "Macro" => "macro",
            _ => object_type,
        };
        refs.push((entity_type.to_owned(), object_id.to_string()));
    }

    let data = &event["data"];
    if let Some(customer_id) = data["customer_id"].as_i64() {
        if !refs.iter().any(|(t, _)| t == "customer") {
            refs.push(("customer".to_owned(), customer_id.to_string()));
        }
    }
    if let Some(ticket_id) = data["ticket_id"].as_i64() {
        if !refs.iter().any(|(t, _)| t == "ticket") {
            refs.push(("ticket".to_owned(), ticket_id.to_string()));
        }
    }
    if let Some(user_id) = event["user_id"].as_i64() {
        refs.push(("agent".to_owned(), user_id.to_string()));
    }

    refs
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TICKET_CREATED: &str = r#"{
        "id": 100, "type": "ticket-created", "object_id": 500, "object_type": "Ticket",
        "created_datetime": "2024-03-15T10:30:00.000000", "user_id": 42,
        "context": "dd4ff312-69df-494a-be96-1a58b3d8b8e0",
        "data": {"customer_id": 777}
    }"#;

    const TICKET_CLOSED: &str = r#"{
        "id": 101, "type": "ticket-closed", "object_id": 500, "object_type": "Ticket",
        "created_datetime": "2024-03-15T11:00:00.000000", "user_id": 42,
        "data": {}
    }"#;

    const MESSAGE_CREATED: &str = r#"{
        "id": 200, "type": "ticket-message-created", "object_id": 8000, "object_type": "TicketMessage",
        "created_datetime": "2024-03-15T10:35:00.000000", "user_id": null,
        "data": {"ticket_id": 500, "customer_id": 777}
    }"#;

    const CUSTOMER_CREATED: &str = r#"{
        "id": 300, "type": "customer-created", "object_id": 777, "object_type": "Customer",
        "created_datetime": "2024-03-14T09:00:00.000000", "user_id": null,
        "data": {}
    }"#;

    const CSAT_RESPONDED: &str = r#"{
        "id": 400, "type": "satisfaction-survey-responded", "object_id": 50, "object_type": "SatisfactionSurvey",
        "created_datetime": "2024-03-16T14:00:00.000000", "user_id": null,
        "data": {"ticket_id": 500, "customer_id": 777}
    }"#;

    const UNKNOWN_EVENT: &str = r#"{
        "id": 999, "type": "some-future-event", "object_id": 1, "object_type": "NewObject",
        "created_datetime": "2024-03-20T12:00:00.000000", "user_id": null,
        "data": {}
    }"#;

    // --- Topic derivation ---

    #[test]
    fn topic_tickets() {
        assert_eq!(derive_topic("ticket-created"), "tickets");
        assert_eq!(derive_topic("ticket-closed"), "tickets");
        assert_eq!(derive_topic("ticket-assigned"), "tickets");
        assert_eq!(derive_topic("ticket-reopened"), "tickets");
        assert_eq!(derive_topic("ticket-merged"), "tickets");
    }

    #[test]
    fn topic_messages() {
        assert_eq!(derive_topic("ticket-message-created"), "messages");
        assert_eq!(derive_topic("ticket-message-deleted"), "messages");
        assert_eq!(derive_topic("ticket-message-updated"), "messages");
    }

    #[test]
    fn topic_customers() {
        assert_eq!(derive_topic("customer-created"), "customers");
        assert_eq!(derive_topic("customer-updated"), "customers");
    }

    #[test]
    fn topic_agents() {
        assert_eq!(derive_topic("user-created"), "agents");
        assert_eq!(derive_topic("user-logged-in"), "agents");
    }

    #[test]
    fn topic_csat() {
        assert_eq!(derive_topic("satisfaction-survey-sent"), "csat");
        assert_eq!(derive_topic("satisfaction-survey-responded"), "csat");
    }

    #[test]
    fn topic_automation() {
        assert_eq!(derive_topic("rule-executed"), "automation");
        assert_eq!(derive_topic("macro-applied"), "automation");
    }

    #[test]
    fn topic_social() {
        assert_eq!(derive_topic("facebook-comment-created"), "social");
        assert_eq!(derive_topic("instagram-direct-message-created"), "social");
    }

    #[test]
    fn topic_unknown() {
        assert_eq!(derive_topic("some-future-event"), "other");
    }

    // --- Entity extraction ---

    #[test]
    fn extract_ticket_created_entities() {
        let raw: serde_json::Value = serde_json::from_str(TICKET_CREATED).unwrap();
        let entities = extract_entities(&raw);

        assert!(entities.iter().any(|(t, i)| t == "ticket" && i == "500"));
        assert!(entities.iter().any(|(t, i)| t == "customer" && i == "777"));
        assert!(entities.iter().any(|(t, i)| t == "agent" && i == "42"));
    }

    #[test]
    fn extract_message_created_entities() {
        let raw: serde_json::Value = serde_json::from_str(MESSAGE_CREATED).unwrap();
        let entities = extract_entities(&raw);

        assert!(entities.iter().any(|(t, i)| t == "message" && i == "8000"));
        assert!(entities.iter().any(|(t, i)| t == "ticket" && i == "500"));
        assert!(entities.iter().any(|(t, i)| t == "customer" && i == "777"));
    }

    #[test]
    fn extract_customer_created_entities() {
        let raw: serde_json::Value = serde_json::from_str(CUSTOMER_CREATED).unwrap();
        let entities = extract_entities(&raw);

        assert!(entities.iter().any(|(t, i)| t == "customer" && i == "777"));
    }

    #[test]
    fn extract_csat_entities() {
        let raw: serde_json::Value = serde_json::from_str(CSAT_RESPONDED).unwrap();
        let entities = extract_entities(&raw);

        assert!(entities.iter().any(|(t, i)| t == "survey" && i == "50"));
        assert!(entities.iter().any(|(t, i)| t == "ticket" && i == "500"));
        assert!(entities.iter().any(|(t, i)| t == "customer" && i == "777"));
    }

    // --- Full conversion ---

    #[test]
    fn convert_ticket_created() {
        let event = convert_webhook(TICKET_CREATED, "org_1").unwrap();

        assert_eq!(event.source.as_str(), "gorgias");
        assert_eq!(event.topic.as_str(), "tickets");
        assert_eq!(event.event_type.as_str(), "ticket-created");
        assert!(event.entity_refs.iter().any(|r| r.entity_type.as_str() == "ticket"));
        assert!(event.entity_refs.iter().any(|r| r.entity_type.as_str() == "customer"));
        assert!(event.raw_body.is_some());
    }

    #[test]
    fn convert_message_created() {
        let event = convert_webhook(MESSAGE_CREATED, "org_1").unwrap();

        assert_eq!(event.topic.as_str(), "messages");
        assert_eq!(event.event_type.as_str(), "ticket-message-created");
    }

    #[test]
    fn convert_csat_responded() {
        let event = convert_webhook(CSAT_RESPONDED, "org_1").unwrap();

        assert_eq!(event.topic.as_str(), "csat");
    }

    #[test]
    fn convert_unknown_event() {
        let event = convert_webhook(UNKNOWN_EVENT, "org_1").unwrap();

        assert_eq!(event.topic.as_str(), "other");
        assert_eq!(event.source.as_str(), "gorgias");
    }

    #[test]
    fn convert_batch() {
        let results = convert_webhooks(&[TICKET_CREATED, MESSAGE_CREATED, CUSTOMER_CREATED], "org_1");
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn convert_invalid_json() {
        assert!(convert_webhook("not json", "org_1").is_err());
    }

    #[test]
    fn trait_convert_works() {
        let connector = GorgiasConnector;
        let event = connector.convert(TICKET_CREATED, "org_1").unwrap();
        assert_eq!(event.source.as_str(), "gorgias");
        assert_eq!(connector.source_name(), "gorgias");
    }

    #[test]
    fn timestamp_is_parsed_correctly() {
        let event = convert_webhook(TICKET_CREATED, "org_1").unwrap();
        assert_eq!(event.event_time.format("%Y-%m-%d").to_string(), "2024-03-15");
    }
}
