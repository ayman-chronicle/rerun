//! `EventLinkStore` for Kurrent -- delegates to Postgres sidecar.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventId, LinkId, OrgId};
use chronicle_core::link::EventLink;
use chronicle_core::query::{EventResult, GraphQuery};

use crate::traits::EventLinkStore;
use super::KurrentBackend;

#[async_trait]
impl EventLinkStore for KurrentBackend {
    async fn create_link(&self, org_id: &OrgId, link: &EventLink) -> Result<LinkId, StoreError> {
        self.pg.create_link(org_id, link).await
    }

    async fn get_links_for_event(&self, org_id: &OrgId, event_id: &EventId) -> Result<Vec<EventLink>, StoreError> {
        self.pg.get_links_for_event(org_id, event_id).await
    }

    async fn traverse(&self, query: &GraphQuery) -> Result<Vec<EventResult>, StoreError> {
        self.pg.traverse(query).await
    }
}
