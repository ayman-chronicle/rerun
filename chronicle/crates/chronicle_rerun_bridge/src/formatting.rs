//! Shared event formatting utilities.
//!
//! Used by the bridge, future detail panel, and search results.
//! Defined once here (DRY) -- every consumer imports from this module.

use chronicle_core::event::Event;

/// Format a concise one-line summary of an event for display.
///
/// Shows: `[source] event_type | key payload fields`
pub fn format_event_summary(event: &Event) -> String {
    let mut parts = vec![format!(
        "[{}] {}",
        event.source.as_str(),
        event.event_type.as_str()
    )];

    if let Some(ref payload) = event.payload {
        if let Some(obj) = payload.as_object() {
            let fields: Vec<String> = obj
                .iter()
                .take(4)
                .map(|(k, v)| {
                    let val = match v {
                        serde_json::Value::String(s) => {
                            if s.len() > 30 {
                                format!("\"{}…\"", &s[..27])
                            } else {
                                format!("\"{s}\"")
                            }
                        }
                        other => other.to_string(),
                    };
                    format!("{k}={val}")
                })
                .collect();

            if !fields.is_empty() {
                parts.push(fields.join(", "));
            }
        }
    }

    parts.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_core::event::EventBuilder;

    #[test]
    fn summary_with_payload() {
        let event = EventBuilder::new("o", "stripe", "t", "payment_intent.succeeded")
            .payload(serde_json::json!({"amount": 4999, "currency": "usd"}))
            .build();
        let s = format_event_summary(&event);
        assert!(s.starts_with("[stripe] payment_intent.succeeded"));
        assert!(s.contains("amount=4999"));
    }

    #[test]
    fn summary_without_payload() {
        let event = EventBuilder::new("o", "product", "t", "page.viewed").build();
        let s = format_event_summary(&event);
        assert_eq!(s, "[product] page.viewed");
    }

    #[test]
    fn summary_truncates_long_strings() {
        let event = EventBuilder::new("o", "s", "t", "e")
            .payload(serde_json::json!({"desc": "a".repeat(100)}))
            .build();
        let s = format_event_summary(&event);
        assert!(s.contains('…'));
    }
}
