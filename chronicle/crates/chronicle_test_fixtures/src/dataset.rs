//! A seeded mini-dataset for deterministic test assertions.
//!
//! [`TestDataset`] contains 10 customers across 5 sources with ~50 events,
//! links, and entity refs. Every query integration test uses this same seed
//! so assertions are stable.

use chronicle_core::event::Event;
use chronicle_core::ids::EventId;
use chronicle_core::link::EventLink;

use crate::factories;

/// A pre-built test dataset with known events, entities, and links.
///
/// Use [`TestDataset::build`] to create the dataset. Then insert it
/// into your backend under test and run assertions against it.
pub struct TestDataset {
    pub org_id: String,
    pub events: Vec<Event>,
    pub links: Vec<EventLink>,

    /// Customer IDs that received a campaign AND later cancelled.
    pub churned_after_campaign: Vec<String>,

    /// Customer IDs that received a campaign but did NOT cancel.
    pub retained_after_campaign: Vec<String>,

    /// The anonymous session ID (not yet linked to a customer).
    pub anonymous_session_id: String,

    /// The customer ID the anonymous session will eventually be linked to.
    pub session_customer_id: String,
}

impl TestDataset {
    /// Build the test dataset. Does not write to any store -- call
    /// [`seed_store`] for that.
    pub fn build() -> Self {
        let org = "test_org";
        let mut events = Vec::new();
        let mut links = Vec::new();

        let campaign_id = "camp_BF";
        let mut churned = Vec::new();
        let mut retained = Vec::new();

        // 10 customers: 3 will churn after campaign, 7 will not
        for i in 0..10 {
            let cust = format!("cust_{i:03}");

            // Everyone gets a payment and a page view
            events.push(factories::stripe_payment(org, &cust, 4999 + i * 100));
            events.push(factories::product_page_view(org, &cust, "/dashboard"));

            // Everyone receives the campaign
            events.push(factories::marketing_campaign_sent(org, &cust, campaign_id));

            if i < 3 {
                // First 3 churn: failed payment → support ticket → cancellation
                let failed = factories::stripe_payment_failed(org, &cust, 4999);
                let ticket = factories::support_ticket(org, &cust, "Billing issue");
                let cancelled = factories::stripe_subscription_cancelled(org, &cust);

                links.push(factories::causal_link(failed.event_id, ticket.event_id, 0.85));
                links.push(factories::causal_link(ticket.event_id, cancelled.event_id, 0.9));

                events.push(failed);
                events.push(ticket);
                events.push(cancelled);

                churned.push(cust);
            } else {
                // Rest are retained -- add extra engagement
                events.push(factories::product_page_view(org, &cust, "/features"));
                retained.push(cust);
            }
        }

        // One voice call for the first customer
        events.push(factories::voice_call(org, "cust_000", 180_000));

        // Anonymous session events (5 page views, no customer ref)
        let session_id = "sess_anon_001";
        for url in &["/", "/pricing", "/features", "/signup", "/checkout"] {
            events.push(factories::anonymous_page_view(org, session_id, url));
        }

        Self {
            org_id: org.to_string(),
            events,
            links,
            churned_after_campaign: churned,
            retained_after_campaign: retained,
            anonymous_session_id: session_id.to_string(),
            session_customer_id: "cust_010".to_string(),
        }
    }

    /// Total number of events in the dataset.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Find events by source.
    pub fn events_by_source(&self, source: &str) -> Vec<&Event> {
        self.events.iter().filter(|e| e.source == source).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_is_consistent() {
        let ds = TestDataset::build();

        assert_eq!(ds.churned_after_campaign.len(), 3);
        assert_eq!(ds.retained_after_campaign.len(), 7);

        // 10 customers × 3 events (payment, page_view, campaign) = 30
        // + 3 churned × 3 extra events (failed, ticket, cancelled) = 9
        // + 7 retained × 1 extra page_view = 7
        // + 1 voice call = 1
        // + 5 anonymous page views = 5
        // Total = 52
        assert_eq!(ds.event_count(), 52);

        assert_eq!(ds.links.len(), 6); // 3 churned × 2 links each

        let stripe_events = ds.events_by_source("stripe");
        assert!(!stripe_events.is_empty());
    }
}
