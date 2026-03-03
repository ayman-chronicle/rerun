//! `EventLinkStore` implementation for the in-memory backend.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventId, LinkId};
use chronicle_core::link::EventLink;
use chronicle_core::query::{EventResult, GraphQuery};
use chronicle_core::link::LinkDirection;

use crate::traits::EventLinkStore;
use super::state::InMemoryBackend;

#[async_trait]
impl EventLinkStore for InMemoryBackend {
    async fn create_link(&self, link: &EventLink) -> Result<LinkId, StoreError> {
        link.validate().map_err(|e| StoreError::Query(e.to_string()))?;
        let mut store = self.links.write();
        store.insert(link.link_id, link.clone());
        Ok(link.link_id)
    }

    async fn get_links_for_event(&self, event_id: &EventId) -> Result<Vec<EventLink>, StoreError> {
        let store = self.links.read();
        Ok(store
            .values()
            .filter(|l| l.source_event_id == *event_id || l.target_event_id == *event_id)
            .cloned()
            .collect())
    }

    async fn traverse(&self, query: &GraphQuery) -> Result<Vec<EventResult>, StoreError> {
        let links = self.links.read();
        let events = self.events.read();
        let mut visited: std::collections::HashSet<EventId> = std::collections::HashSet::new();
        let mut queue = vec![(query.start_event_id, 0u32)];
        let mut results = Vec::new();

        while let Some((current_id, depth)) = queue.pop() {
            if depth > query.max_depth || !visited.insert(current_id) {
                continue;
            }

            if let Some(event) = events.get(&current_id) {
                if event.org_id == query.org_id {
                    results.push(EventResult {
                        event: event.clone(),
                        entity_refs: vec![],
                        search_distance: None,
                    });
                }
            }

            for link in links.values() {
                if link.confidence.value() < query.min_confidence {
                    continue;
                }
                if let Some(ref types) = query.link_types {
                    if !types.contains(&link.link_type) {
                        continue;
                    }
                }

                let next = match query.direction {
                    LinkDirection::Outgoing if link.source_event_id == current_id => {
                        Some(link.target_event_id)
                    }
                    LinkDirection::Incoming if link.target_event_id == current_id => {
                        Some(link.source_event_id)
                    }
                    LinkDirection::Both => {
                        if link.source_event_id == current_id {
                            Some(link.target_event_id)
                        } else if link.target_event_id == current_id {
                            Some(link.source_event_id)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };

                if let Some(next_id) = next {
                    queue.push((next_id, depth + 1));
                }
            }
        }

        results.sort_by_key(|r| r.event.event_time);
        Ok(results)
    }
}
