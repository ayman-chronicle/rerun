//! Extract embeddable text content from events.
//!
//! Not all events have meaningful text. A payment amount isn't worth
//! embedding, but a support ticket subject + body is. This module
//! decides what text to extract based on the event's source and payload.

use chronicle_core::event::Event;

/// Extract text content worth embedding from an event.
///
/// Returns `None` if the event has no meaningful text content.
/// The returned string is a concatenation of relevant text fields,
/// prefixed with source and type for context.
pub fn extract_text(event: &Event) -> Option<String> {
    let payload = event.payload.as_ref()?;
    let obj = payload.as_object()?;

    let mut parts: Vec<String> = Vec::new();

    // Always include source and type as context
    parts.push(format!("[{}:{}]", event.source, event.event_type));

    // Extract known text-bearing fields in priority order
    for field in TEXT_FIELDS {
        if let Some(value) = obj.get(*field) {
            if let Some(text) = value.as_str() {
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
        }
    }

    // If we only have the context prefix, try a JSON summary
    if parts.len() == 1 {
        let summary = summarize_payload(obj);
        if !summary.is_empty() {
            parts.push(summary);
        }
    }

    // Need more than just the context prefix to be worth embedding
    if parts.len() > 1 {
        Some(parts.join(" "))
    } else {
        None
    }
}

/// Fields commonly containing embeddable text, checked in order.
const TEXT_FIELDS: &[&str] = &[
    "subject",
    "body",
    "description",
    "summary",
    "transcript",
    "message",
    "content",
    "text",
    "reason",
    "note",
    "comment",
    "title",
    "name",
    "failure_reason",
    "disposition",
];

/// Create a brief textual summary of a payload's key-value pairs.
/// Used when no specific text field is found.
fn summarize_payload(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    obj.iter()
        .filter_map(|(k, v)| {
            match v {
                serde_json::Value::String(s) => Some(format!("{k}: {s}")),
                serde_json::Value::Number(n) => Some(format!("{k}: {n}")),
                serde_json::Value::Bool(b) => Some(format!("{k}: {b}")),
                _ => None,
            }
        })
        .take(5)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_core::event::EventBuilder;

    #[test]
    fn extracts_subject_from_ticket() {
        let event = EventBuilder::new("org", "support", "tickets", "ticket.created")
            .payload(serde_json::json!({
                "subject": "Can't access my account",
                "priority": "high",
            }))
            .build();

        let text = extract_text(&event).unwrap();
        assert!(text.contains("Can't access my account"));
        assert!(text.contains("[support:ticket.created]"));
    }

    #[test]
    fn extracts_body_and_subject() {
        let event = EventBuilder::new("org", "support", "email", "email.received")
            .payload(serde_json::json!({
                "subject": "Billing question",
                "body": "I was charged twice for my subscription.",
            }))
            .build();

        let text = extract_text(&event).unwrap();
        assert!(text.contains("Billing question"));
        assert!(text.contains("charged twice"));
    }

    #[test]
    fn falls_back_to_summary_for_structured_data() {
        let event = EventBuilder::new("org", "stripe", "payments", "payment.succeeded")
            .payload(serde_json::json!({
                "amount": 4999,
                "currency": "usd",
                "status": "succeeded",
            }))
            .build();

        let text = extract_text(&event).unwrap();
        assert!(text.contains("amount: 4999"));
        assert!(text.contains("currency: usd"));
    }

    #[test]
    fn returns_none_for_no_payload() {
        let event = EventBuilder::new("org", "sys", "health", "ping")
            .build();
        assert!(extract_text(&event).is_none());
    }

    #[test]
    fn returns_none_for_empty_object() {
        let event = EventBuilder::new("org", "sys", "health", "ping")
            .payload(serde_json::json!({}))
            .build();
        assert!(extract_text(&event).is_none());
    }
}
