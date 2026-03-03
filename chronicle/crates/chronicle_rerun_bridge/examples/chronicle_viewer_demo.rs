//! Chronicle Viewer Demo
//!
//! Seeds 500 events across 5 sources for 10 customers over 90 days,
//! creates causal links between related events, and visualizes
//! everything in the Rerun viewer.
//!
//! ```sh
//! cargo run -p chronicle_rerun_bridge --example chronicle_viewer_demo
//! ```

use std::sync::Arc;

use chrono::{Duration, Utc};

use chronicle_core::event::{Event, EventBuilder};
use chronicle_core::ids::{EventId, LinkId, Confidence, OrgId};
use chronicle_core::link::EventLink;
use chronicle_core::query::{OrderBy, StructuredQuery};
use chronicle_rerun_bridge::ChronicleBridge;
use chronicle_store::memory::InMemoryBackend;
use chronicle_store::traits::{EventLinkStore, EventStore};
use chronicle_store::StorageEngine;

const CUSTOMERS: u32 = 10;
const PAGES: [&str; 5] = ["pricing", "features", "docs", "integrations", "changelog"];
const FEATURES: [&str; 4] = ["dashboard", "reports", "api", "webhooks"];
const CAMPAIGNS: [&str; 4] = ["spring_promo", "webinar_q1", "product_launch", "referral"];
const PLANS: [&str; 3] = ["starter", "pro", "enterprise"];
const TICKET_SUBJECTS: [&str; 3] = [
    "Integration help needed",
    "Billing question",
    "Feature request",
];

fn day(offset: i64) -> i64 {
    offset.max(1)
}

fn base_amount(plan: &str) -> i64 {
    match plan {
        "starter" => 1900,
        "pro" => 4900,
        _ => 9900,
    }
}

/// Per-customer tracking of key event IDs used for causal linking.
#[derive(Default)]
struct LinkableIds {
    email_clicked: Option<EventId>,
    first_page_view: Option<EventId>,
    failed_payment: Option<EventId>,
    first_ticket: Option<EventId>,
    second_ticket: Option<EventId>,
    escalated_ticket: Option<EventId>,
    cancellation: Option<EventId>,
    overdue_invoice: Option<EventId>,
}

fn seed_marketing(ci: u32, cust: &str, events: &mut Vec<Event>, ids: &mut LinkableIds) {
    let now = Utc::now();
    let plan = PLANS[ci as usize % PLANS.len()];

    for j in 0..3u32 {
        let d = day(88 - i64::from(ci) * 2 - i64::from(j) * 8);
        events.push(
            EventBuilder::new("demo_org", "marketing", "campaigns", "email.sent")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "campaign": CAMPAIGNS[j as usize % CAMPAIGNS.len()],
                    "subject": format!("Check out our {plan} plan"),
                    "channel": "email"
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    for j in 0..3u32 {
        let d = day(86 - i64::from(ci) * 2 - i64::from(j) * 8);
        events.push(
            EventBuilder::new("demo_org", "marketing", "campaigns", "email.opened")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "campaign": CAMPAIGNS[j as usize % CAMPAIGNS.len()],
                    "open_count": j + 1
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    for j in 0..2u32 {
        let d = day(84 - i64::from(ci) * 2 - i64::from(j) * 10);
        let page = PAGES[j as usize % PAGES.len()];
        let evt = EventBuilder::new("demo_org", "marketing", "campaigns", "email.clicked")
            .entity("customer", cust)
            .payload(serde_json::json!({
                "campaign": CAMPAIGNS[j as usize % CAMPAIGNS.len()],
                "link": format!("https://app.example.com/{page}")
            }))
            .event_time(now - Duration::days(d))
            .build();
        if j == 0 {
            ids.email_clicked = Some(evt.event_id);
        }
        events.push(evt);
    }

    for j in 0..2u32 {
        let d = day(82 - i64::from(ci) * 2 - i64::from(j) * 12);
        let amt = base_amount(plan);
        events.push(
            EventBuilder::new("demo_org", "marketing", "campaigns", "campaign.attributed")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "campaign": CAMPAIGNS[j as usize % CAMPAIGNS.len()],
                    "attribution": "first_touch",
                    "revenue": amt
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }
}

fn seed_product(ci: u32, cust: &str, events: &mut Vec<Event>, ids: &mut LinkableIds) {
    let now = Utc::now();

    for j in 0..5u32 {
        let d = day(80 - i64::from(ci) * 2 - i64::from(j) * 5);
        let page = PAGES[j as usize % PAGES.len()];
        let referrer = if j == 0 { "email" } else { "direct" };
        let evt = EventBuilder::new("demo_org", "product", "usage", "page.viewed")
            .entity("customer", cust)
            .payload(serde_json::json!({
                "page": page,
                "duration_ms": 3000 + j * 1500,
                "referrer": referrer
            }))
            .event_time(now - Duration::days(d))
            .build();
        if j == 0 {
            ids.first_page_view = Some(evt.event_id);
        }
        events.push(evt);
    }

    for j in 0..4u32 {
        let d = day(70 - i64::from(ci) * 2 - i64::from(j) * 6);
        events.push(
            EventBuilder::new("demo_org", "product", "usage", "feature.used")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "feature": FEATURES[j as usize % FEATURES.len()],
                    "duration_ms": 5000 + j * 2000
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    for j in 0..3u32 {
        let d = day(75 - i64::from(ci) * 2 - i64::from(j) * 10);
        events.push(
            EventBuilder::new("demo_org", "product", "sessions", "session.started")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "user_agent": "Mozilla/5.0",
                    "ip_country": "US"
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    for j in 0..3u32 {
        let d = day(74 - i64::from(ci) * 2 - i64::from(j) * 10);
        events.push(
            EventBuilder::new("demo_org", "product", "sessions", "session.ended")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "duration_s": 120 + j * 60,
                    "pages_viewed": 3 + j
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }
}

fn seed_stripe(
    ci: u32,
    cust: &str,
    is_churner: bool,
    events: &mut Vec<Event>,
    ids: &mut LinkableIds,
) {
    let now = Utc::now();
    let plan = PLANS[ci as usize % PLANS.len()];
    let amt = base_amount(plan);

    // subscription.created
    let d = day(78 - i64::from(ci) * 2);
    events.push(
        EventBuilder::new("demo_org", "stripe", "subscriptions", "subscription.created")
            .entity("customer", cust)
            .payload(serde_json::json!({
                "plan": plan,
                "amount": amt,
                "currency": "usd",
                "interval": "monthly"
            }))
            .event_time(now - Duration::days(d))
            .build(),
    );

    // 7 payment cycles — last one fails for churners
    for j in 0..7u32 {
        let d = day(70 - i64::from(ci) * 2 - i64::from(j) * 8);
        if is_churner && j == 6 {
            let evt =
                EventBuilder::new("demo_org", "stripe", "payments", "payment_intent.failed")
                    .entity("customer", cust)
                    .payload(serde_json::json!({
                        "amount": amt,
                        "currency": "usd",
                        "failure_code": "card_declined",
                        "failure_message": "Your card was declined"
                    }))
                    .event_time(now - Duration::days(d))
                    .build();
            ids.failed_payment = Some(evt.event_id);
            events.push(evt);
        } else {
            events.push(
                EventBuilder::new(
                    "demo_org",
                    "stripe",
                    "payments",
                    "payment_intent.succeeded",
                )
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "amount": amt + (i64::from(j) * 100),
                    "currency": "usd",
                    "payment_method": "card"
                }))
                .event_time(now - Duration::days(d))
                .build(),
            );
        }
    }

    // Churners cancel; others get a refund
    if is_churner {
        let d = day(5 - i64::from(ci) / 4);
        let evt = EventBuilder::new(
            "demo_org",
            "stripe",
            "subscriptions",
            "subscription.cancelled",
        )
        .entity("customer", cust)
        .payload(serde_json::json!({
            "reason": "payment_failure",
            "at_period_end": true
        }))
        .event_time(now - Duration::days(d))
        .build();
        ids.cancellation = Some(evt.event_id);
        events.push(evt);
    } else {
        let d = day(25 - i64::from(ci) / 2);
        events.push(
            EventBuilder::new("demo_org", "stripe", "payments", "charge.refunded")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "amount": 1500,
                    "currency": "usd",
                    "reason": "requested_by_customer"
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    // One extra payment to reach 10 stripe events per customer
    let d = day(30 - i64::from(ci));
    events.push(
        EventBuilder::new(
            "demo_org",
            "stripe",
            "payments",
            "payment_intent.succeeded",
        )
        .entity("customer", cust)
        .payload(serde_json::json!({
            "amount": amt,
            "currency": "usd",
            "payment_method": "card"
        }))
        .event_time(now - Duration::days(d))
        .build(),
    );
}

fn seed_billing(ci: u32, cust: &str, events: &mut Vec<Event>, ids: &mut LinkableIds) {
    let now = Utc::now();
    let plan = PLANS[ci as usize % PLANS.len()];
    let amt = base_amount(plan);

    for j in 0..3u32 {
        let d = day(75 - i64::from(ci) * 2 - i64::from(j) * 10);
        events.push(
            EventBuilder::new("demo_org", "billing", "invoices", "invoice.created")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "amount": amt,
                    "currency": "usd",
                    "period": format!("2025-{:02}", 10u32.saturating_sub(j))
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    for j in 0..2u32 {
        let d = day(72 - i64::from(ci) * 2 - i64::from(j) * 10);
        events.push(
            EventBuilder::new("demo_org", "billing", "invoices", "invoice.paid")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "amount": amt,
                    "currency": "usd",
                    "payment_method": "card"
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    // Overdue invoice
    let d = day(20 - i64::from(ci) / 2);
    let evt = EventBuilder::new("demo_org", "billing", "invoices", "invoice.overdue")
        .entity("customer", cust)
        .payload(serde_json::json!({
            "amount": amt,
            "currency": "usd",
            "days_overdue": 7 + ci
        }))
        .event_time(now - Duration::days(d))
        .build();
    ids.overdue_invoice = Some(evt.event_id);
    events.push(evt);

    // Plan change
    let d = day(40 - i64::from(ci));
    events.push(
        EventBuilder::new("demo_org", "billing", "plans", "plan.changed")
            .entity("customer", cust)
            .payload(serde_json::json!({
                "from_plan": "starter",
                "to_plan": plan,
                "amount_change": amt - 1900
            }))
            .event_time(now - Duration::days(d))
            .build(),
    );
}

fn seed_support(
    ci: u32,
    cust: &str,
    is_churner: bool,
    events: &mut Vec<Event>,
    ids: &mut LinkableIds,
) {
    let now = Utc::now();

    for j in 0..3u32 {
        let d = day(60 - i64::from(ci) * 2 - i64::from(j) * 12);
        let priority = if j == 0 && is_churner {
            "high"
        } else {
            "normal"
        };
        let evt = EventBuilder::new("demo_org", "support", "tickets", "ticket.created")
            .entity("customer", cust)
            .payload(serde_json::json!({
                "subject": TICKET_SUBJECTS[j as usize],
                "priority": priority
            }))
            .event_time(now - Duration::days(d))
            .build();
        match j {
            0 => ids.first_ticket = Some(evt.event_id),
            1 => ids.second_ticket = Some(evt.event_id),
            _ => {}
        }
        events.push(evt);
    }

    for j in 0..2u32 {
        let d = day(55 - i64::from(ci) * 2 - i64::from(j) * 12);
        let assignee = format!("agent_{}", (ci + j) % 3 + 1);
        events.push(
            EventBuilder::new("demo_org", "support", "tickets", "ticket.updated")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "status": "in_progress",
                    "assignee": assignee
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    for j in 0..2u32 {
        let d = day(50 - i64::from(ci) * 2 - i64::from(j) * 12);
        let (resolution, satisfaction) = if j == 0 {
            ("resolved", 4)
        } else {
            ("wont_fix", 2)
        };
        events.push(
            EventBuilder::new("demo_org", "support", "tickets", "ticket.closed")
                .entity("customer", cust)
                .payload(serde_json::json!({
                    "resolution": resolution,
                    "satisfaction": satisfaction
                }))
                .event_time(now - Duration::days(d))
                .build(),
        );
    }

    // Escalation
    let d = day(10 - i64::from(ci) / 3);
    let reason = if is_churner {
        "payment_issue"
    } else {
        "response_time"
    };
    let evt = EventBuilder::new("demo_org", "support", "tickets", "ticket.escalated")
        .entity("customer", cust)
        .payload(serde_json::json!({
            "reason": reason,
            "escalated_to": "team_lead"
        }))
        .event_time(now - Duration::days(d))
        .build();
    ids.escalated_ticket = Some(evt.event_id);
    events.push(evt);
}

fn build_links(ids: &LinkableIds, is_churner: bool) -> Vec<EventLink> {
    let now = Utc::now();
    let mut links = Vec::new();

    // Marketing click → first product page view
    if let (Some(click), Some(view)) = (ids.email_clicked, ids.first_page_view) {
        links.push(EventLink {
            link_id: LinkId::new(),
            source_event_id: click,
            target_event_id: view,
            link_type: "campaign_conversion".to_string(),
            confidence: Confidence::new(0.75).unwrap(),
            reasoning: Some("Marketing email click led to product page view".to_string()),
            created_by: "demo".to_string(),
            created_at: now,
        });
    }

    // Churner chain: payment failure → ticket → escalation → cancellation
    if is_churner {
        if let (Some(fail), Some(ticket)) = (ids.failed_payment, ids.first_ticket) {
            links.push(EventLink {
                link_id: LinkId::new(),
                source_event_id: fail,
                target_event_id: ticket,
                link_type: "caused_by".to_string(),
                confidence: Confidence::new(0.85).unwrap(),
                reasoning: Some("Payment failure led to support ticket".to_string()),
                created_by: "demo".to_string(),
                created_at: now,
            });
        }
        if let (Some(esc), Some(cancel)) = (ids.escalated_ticket, ids.cancellation) {
            links.push(EventLink {
                link_id: LinkId::new(),
                source_event_id: esc,
                target_event_id: cancel,
                link_type: "led_to".to_string(),
                confidence: Confidence::new(0.90).unwrap(),
                reasoning: Some(
                    "Escalated ticket preceded subscription cancellation".to_string(),
                ),
                created_by: "demo".to_string(),
                created_at: now,
            });
        }
    }

    // Overdue invoice → billing support ticket
    if let (Some(inv), Some(ticket)) = (ids.overdue_invoice, ids.second_ticket) {
        links.push(EventLink {
            link_id: LinkId::new(),
            source_event_id: inv,
            target_event_id: ticket,
            link_type: "triggered".to_string(),
            confidence: Confidence::new(0.70).unwrap(),
            reasoning: Some("Overdue invoice triggered billing support ticket".to_string()),
            created_by: "demo".to_string(),
            created_at: now,
        });
    }

    links
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = Arc::new(InMemoryBackend::new());
    let engine = StorageEngine {
        events: backend.clone(),
        entity_refs: backend.clone(),
        links: backend.clone(),
        embeddings: backend.clone(),
        schemas: backend.clone(),
    };

    // -- Seed events & collect link candidates ----------------------------------

    let mut all_events = Vec::with_capacity(550);
    let mut all_links = Vec::new();

    for ci in 0..CUSTOMERS {
        let cust = format!("cust_{:03}", ci + 1);
        let is_churner = ci % 4 == 0;
        let mut ids = LinkableIds::default();

        seed_marketing(ci, &cust, &mut all_events, &mut ids);
        seed_product(ci, &cust, &mut all_events, &mut ids);
        seed_stripe(ci, &cust, is_churner, &mut all_events, &mut ids);
        seed_billing(ci, &cust, &mut all_events, &mut ids);
        seed_support(ci, &cust, is_churner, &mut all_events, &mut ids);

        all_links.extend(build_links(&ids, is_churner));
    }

    let event_count = all_events.len();
    engine.events.insert_events(&all_events).await?;

    // -- Persist links ----------------------------------------------------------

    let link_count = all_links.len();
    for link in &all_links {
        engine.links.create_link(link).await?;
    }

    // -- Open Rerun viewer & send data ------------------------------------------

    let bridge = ChronicleBridge::new(engine.clone(), "Chronicle Demo")?;

    let query = StructuredQuery {
        org_id: OrgId::new("demo_org"),
        source: None,
        entity: None,
        topic: None,
        event_type: None,
        time_range: None,
        payload_filters: vec![],
        group_by: None,
        order_by: OrderBy::EventTimeAsc,
        limit: 100_000,
        offset: 0,
    };

    let results = engine.events.query_structured(&query).await?;
    let (text_count, logged_links) = bridge.log_events_with_links(&results, &all_links)?;

    let scalar_count = bridge.load_scalars(&query, "amount").await?;

    // -- Summary ----------------------------------------------------------------

    println!();
    println!("Chronicle Viewer Demo");
    println!("=====================");
    println!("Seeded: {event_count} events across 5 sources for {CUSTOMERS} customers");
    println!("Created: {link_count} causal links");
    println!("Logged: {text_count} TextLog + ChronicleEvent entries to Rerun");
    println!("Logged: {logged_links} ChronicleLink entries to Rerun");
    println!("Logged: {scalar_count} Scalar data points to Rerun");
    println!();
    println!("The Rerun viewer should be open. Try:");
    println!("  - Scrub the timeline to see events across 90 days");
    println!("  - Look at the TextLog view for event summaries");
    println!("  - Check the TimeSeries view for payment amounts");
    println!("  - Browse the entity tree in the left panel");
    println!();

    println!("Press Ctrl+C to exit…");
    tokio::signal::ctrl_c().await?;

    Ok(())
}
