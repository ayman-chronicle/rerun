//! `EventLinkStore` for the hybrid backend -- delegates entirely to Postgres.
//!
//! Event links (graph edges) always live in Postgres. The recursive
//! CTE traversal needs ACID and relational joins that Parquet can't provide.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventId, LinkId};
use chronicle_core::link::EventLink;
use chronicle_core::query::{EventResult, GraphQuery};

use crate::traits::EventLinkStore;
use super::HybridBackend;

#[async_trait]
impl EventLinkStore for HybridBackend {
    async fn create_link(&self, link: &EventLink) -> Result<LinkId, StoreError> {
        self.pg.create_link(link).await
    }

    async fn get_links_for_event(&self, event_id: &EventId) -> Result<Vec<EventLink>, StoreError> {
        self.pg.get_links_for_event(event_id).await
    }

    async fn traverse(&self, query: &GraphQuery) -> Result<Vec<EventResult>, StoreError> {
        // The Postgres CTE traversal finds linked event_ids, then fetches
        // events from Postgres. For archived events, the CTE will find the
        // link chain but the event data needs to come from Parquet.
        //
        // For now, delegate to Postgres. Events that have been archived
        // will be missing from the traversal results. A future improvement
        // would federate: get event_ids from the CTE, then look up missing
        // events in Parquet.
        self.pg.traverse(query).await
    }
}
