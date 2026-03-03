//! Factory functions for creating test data.
//!
//! Each factory produces a valid, fully-formed domain object with
//! sensible defaults. Override fields as needed in tests.

use chronicle_core::event::{EventBuilder, Event};
use chronicle_core::ids::{Confidence, EventId, LinkId};
use chronicle_core::link::{EventLink, LinkDirection};
use chronicle_core::media::MediaAttachment;
use chrono::{DateTime, Utc};

/// A Stripe payment event with a customer entity ref.
pub fn stripe_payment(org_id: &str, customer_id: &str, amount: i64) -> Event {
    EventBuilder::new(org_id, "stripe", "payments", "payment_intent.succeeded")
        .entity("customer", customer_id)
        .payload(serde_json::json!({
            "amount": amount,
            "currency": "usd",
            "status": "succeeded",
        }))
        .build()
}

/// A failed Stripe payment.
pub fn stripe_payment_failed(org_id: &str, customer_id: &str, amount: i64) -> Event {
    EventBuilder::new(org_id, "stripe", "payments", "payment_intent.payment_failed")
        .entity("customer", customer_id)
        .payload(serde_json::json!({
            "amount": amount,
            "currency": "usd",
            "status": "failed",
            "failure_reason": "insufficient_funds",
        }))
        .build()
}

/// A Stripe subscription cancellation.
pub fn stripe_subscription_cancelled(org_id: &str, customer_id: &str) -> Event {
    EventBuilder::new(org_id, "stripe", "subscriptions", "customer.subscription.deleted")
        .entity("customer", customer_id)
        .payload(serde_json::json!({
            "plan": "pro",
            "reason": "customer_request",
        }))
        .build()
}

/// A support ticket creation.
pub fn support_ticket(org_id: &str, customer_id: &str, subject: &str) -> Event {
    EventBuilder::new(org_id, "support", "tickets", "ticket.created")
        .entity("customer", customer_id)
        .entity("ticket", format!("tkt_{}", chronicle_tuid::Tuid::new().short_string()))
        .payload(serde_json::json!({
            "subject": subject,
            "priority": "high",
            "channel": "email",
        }))
        .build()
}

/// A support voice call with inline audio.
pub fn voice_call(org_id: &str, customer_id: &str, duration_ms: u64) -> Event {
    let fake_audio = vec![0x4F, 0x67, 0x67, 0x53]; // OGG magic bytes
    EventBuilder::new(org_id, "support", "voice_calls", "call.completed")
        .entity("customer", customer_id)
        .media(MediaAttachment::audio_ogg(fake_audio))
        .payload(serde_json::json!({
            "duration_ms": duration_ms,
            "agent_id": "agent_1",
            "disposition": "resolved",
        }))
        .build()
}

/// A marketing campaign send event.
pub fn marketing_campaign_sent(org_id: &str, customer_id: &str, campaign_id: &str) -> Event {
    EventBuilder::new(org_id, "marketing", "campaigns", "campaign.sent")
        .entity("customer", customer_id)
        .payload(serde_json::json!({
            "campaign_id": campaign_id,
            "channel": "email",
            "subject": "Check out our new features!",
        }))
        .build()
}

/// A product usage event (e.g., page view).
pub fn product_page_view(org_id: &str, customer_id: &str, url: &str) -> Event {
    EventBuilder::new(org_id, "product", "usage", "page.viewed")
        .entity("customer", customer_id)
        .payload(serde_json::json!({
            "url": url,
            "session_duration_ms": 45000,
        }))
        .build()
}

/// An anonymous product event (no customer yet, only session).
pub fn anonymous_page_view(org_id: &str, session_id: &str, url: &str) -> Event {
    EventBuilder::new(org_id, "product", "usage", "page.viewed")
        .entity("session", session_id)
        .payload(serde_json::json!({
            "url": url,
            "referrer": "google.com",
        }))
        .build()
}

/// Create a causal link between two events.
pub fn causal_link(
    source_event: EventId,
    target_event: EventId,
    confidence: f32,
) -> EventLink {
    EventLink {
        link_id: LinkId::new(),
        source_event_id: source_event,
        target_event_id: target_event,
        link_type: "caused_by".to_string(),
        confidence: Confidence::new(confidence).expect("test confidence should be valid"),
        reasoning: Some("test link".to_string()),
        created_by: "test".to_string(),
        created_at: Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factories_produce_valid_events() {
        let payment = stripe_payment("org_1", "cust_1", 4999);
        assert_eq!(payment.source, "stripe");
        assert_eq!(payment.event_type, "payment_intent.succeeded");
        assert_eq!(payment.entity_refs.len(), 1);
        assert_eq!(payment.entity_refs[0].entity_type, "customer");
    }

    #[test]
    fn voice_call_has_media() {
        let call = voice_call("org_1", "cust_1", 120_000);
        assert!(call.media.is_some());
        let media = call.media.unwrap();
        assert_eq!(media.media_type, "audio/ogg");
        assert!(media.is_inline());
    }

    #[test]
    fn anonymous_event_has_session_entity() {
        let view = anonymous_page_view("org_1", "sess_abc", "/pricing");
        assert_eq!(view.entity_refs.len(), 1);
        assert_eq!(view.entity_refs[0].entity_type, "session");
        assert_eq!(view.entity_refs[0].entity_id.as_str(), "sess_abc");
    }

    #[test]
    fn causal_link_is_valid() {
        let a = EventId::new();
        let b = EventId::new();
        let link = causal_link(a, b, 0.9);
        assert!(link.validate().is_ok());
        assert_eq!(link.link_type, "caused_by");
    }
}
