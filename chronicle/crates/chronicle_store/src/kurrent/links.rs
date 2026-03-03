//! `EventLinkStore` for Kurrent -- delegates to Postgres sidecar.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventId, LinkId};
use chronicle_core::link::EventLink;
use chronicle_core::query::{EventResult, GraphQuery};

use crate::traits::EventLinkStore;
use super::KurrentBackend;

#[async_trait]
impl EventLinkStore for KurrentBackend {
    async fn create_link(&self, link: &EventLink) -> Result<LinkId, StoreError> {
        self.pg.create_link(link).await
    }

    async fn get_links_for_event(&self, event_id: &EventId) -> Result<Vec<EventLink>, StoreError> {
        self.pg.get_links_for_event(event_id).await
    }

    async fn traverse(&self, query: &GraphQuery) -> Result<Vec<EventResult>, StoreError> {
        self.pg.traverse(query).await
    }
}
