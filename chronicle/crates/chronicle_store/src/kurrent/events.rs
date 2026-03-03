//! `EventStore` for the Kurrent backend.
//!
//! Writes go to BOTH Kurrent (for subscriptions and global ordering)
//! and Postgres (for queries). Reads always go through Postgres.

use async_trait::async_trait;
use kurrentdb::EventData;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::{EventId, OrgId};
use chronicle_core::query::{EventResult, StructuredQuery, TimelineQuery};

use crate::traits::EventStore;
use super::KurrentBackend;

#[async_trait]
impl EventStore for KurrentBackend {
    async fn insert_events(&self, events: &[Event]) -> Result<Vec<EventId>, StoreError> {
        for event in events {
            let stream_name = format!("{}-{}", event.source.as_str(), event.event_type.as_str());

            let payload = serde_json::json!({
                "event_id": event.event_id.to_string(),
                "org_id": event.org_id.as_str(),
                "source": event.source.as_str(),
                "topic": event.topic.as_str(),
                "event_type": event.event_type.as_str(),
                "event_time": event.event_time.to_rfc3339(),
                "payload": event.payload,
            });

            let event_data = EventData::json(event.event_type.as_str(), &payload)
                .map_err(|e| StoreError::Internal(format!("serialize event for Kurrent: {e}")))?;

            self.kurrent
                .append_to_stream(stream_name, &Default::default(), event_data)
                .await
                .map_err(|e| StoreError::Internal(format!("Kurrent append: {e}")))?;
        }

        // Also write to Postgres for query support.
        self.pg.insert_events(events).await
    }

    async fn get_event(
        &self,
        org_id: &OrgId,
        id: &EventId,
    ) -> Result<Option<EventResult>, StoreError> {
        self.pg.get_event(org_id, id).await
    }

    async fn query_structured(
        &self,
        query: &StructuredQuery,
    ) -> Result<Vec<EventResult>, StoreError> {
        self.pg.query_structured(query).await
    }

    async fn query_timeline(
        &self,
        query: &TimelineQuery,
    ) -> Result<Vec<EventResult>, StoreError> {
        self.pg.query_timeline(query).await
    }

    async fn query_sql(
        &self,
        org_id: &OrgId,
        sql: &str,
    ) -> Result<Vec<EventResult>, StoreError> {
        self.pg.query_sql(org_id, sql).await
    }

    async fn count(&self, query: &StructuredQuery) -> Result<u64, StoreError> {
        self.pg.count(query).await
    }
}
