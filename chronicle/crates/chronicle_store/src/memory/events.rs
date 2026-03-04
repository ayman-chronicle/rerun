//! `EventStore` implementation for the in-memory backend.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventId, OrgId};
use chronicle_core::query::{EventResult, OrderBy, StructuredQuery, TimelineQuery};

use crate::traits::EventStore;
use super::state::InMemoryBackend;

#[async_trait]
impl EventStore for InMemoryBackend {
    async fn insert_events(&self, events: &[chronicle_core::event::Event]) -> Result<Vec<EventId>, StoreError> {
        let ids = {
            let mut store = self.events.write();
            let mut refs_store = self.entity_refs.write();
            let mut ids = Vec::with_capacity(events.len());

            for event in events {
                ids.push(event.event_id);
                let materialized = event.materialize_entity_refs("ingestion");
                refs_store.extend(materialized);
                store.insert(event.event_id, event.clone());
            }

            ids
        };

        self.dispatch_to_subscribers(events).await;

        Ok(ids)
    }

    async fn get_event(&self, org_id: &OrgId, id: &EventId) -> Result<Option<EventResult>, StoreError> {
        let store = self.events.read();
        let event = store.get(id).filter(|e| e.org_id == *org_id).cloned();
        Ok(event.map(|e| EventResult {
            event: e,
            entity_refs: vec![],
            search_distance: None,
        }))
    }

    async fn query_structured(&self, query: &StructuredQuery) -> Result<Vec<EventResult>, StoreError> {
        let store = self.events.read();
        let refs_store = self.entity_refs.read();

        let mut results: Vec<EventResult> = store
            .values()
            .filter(|e| e.org_id == query.org_id)
            .filter(|e| query.source.as_ref().map_or(true, |s| e.source == *s))
            .filter(|e| query.topic.as_ref().map_or(true, |t| e.topic == *t))
            .filter(|e| query.event_type.as_ref().map_or(true, |t| e.event_type == *t))
            .filter(|e| query.time_range.as_ref().map_or(true, |r| r.contains(e.event_time)))
            .filter(|e| {
                query.entity.as_ref().map_or(true, |(etype, eid)| {
                    refs_store.iter().any(|r| {
                        r.event_id == e.event_id
                            && r.entity_type == *etype
                            && r.entity_id == *eid
                    })
                })
            })
            .map(|e| EventResult {
                event: e.clone(),
                entity_refs: vec![],
                search_distance: None,
            })
            .collect();

        sort_results(&mut results, &query.order_by);
        results.truncate(query.limit);
        Ok(results)
    }

    async fn query_timeline(&self, query: &TimelineQuery) -> Result<Vec<EventResult>, StoreError> {
        let store = self.events.read();
        let refs_store = self.entity_refs.read();

        let matching_event_ids: Vec<EventId> = refs_store
            .iter()
            .filter(|r| r.entity_type == query.entity_type && r.entity_id == query.entity_id)
            .map(|r| r.event_id)
            .collect();

        let mut results: Vec<EventResult> = store
            .values()
            .filter(|e| e.org_id == query.org_id)
            .filter(|e| matching_event_ids.contains(&e.event_id))
            .filter(|e| query.sources.as_ref().map_or(true, |srcs| srcs.iter().any(|s| e.source == *s)))
            .filter(|e| query.time_range.as_ref().map_or(true, |r| r.contains(e.event_time)))
            .map(|e| EventResult {
                event: e.clone(),
                entity_refs: vec![],
                search_distance: None,
            })
            .collect();

        sort_results(&mut results, &OrderBy::EventTimeAsc);
        Ok(results)
    }

    async fn query_sql(&self, _org_id: &OrgId, _sql: &str) -> Result<Vec<EventResult>, StoreError> {
        Err(StoreError::Query("SQL not supported in in-memory backend".to_string()))
    }

    async fn count(&self, query: &StructuredQuery) -> Result<u64, StoreError> {
        let results = self.query_structured(query).await?;
        Ok(results.len() as u64)
    }
}

/// Shared sort logic -- DRY: used by all query methods.
fn sort_results(results: &mut [EventResult], order: &OrderBy) {
    match order {
        OrderBy::EventTimeAsc => results.sort_by_key(|r| r.event.event_time),
        OrderBy::EventTimeDesc => results.sort_by(|a, b| b.event.event_time.cmp(&a.event.event_time)),
        OrderBy::IngestionTimeAsc => results.sort_by_key(|r| r.event.ingestion_time),
        OrderBy::IngestionTimeDesc => results.sort_by(|a, b| b.event.ingestion_time.cmp(&a.event.ingestion_time)),
    }
}
