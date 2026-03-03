//! `SchemaRegistry` implementation for the in-memory backend.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventType, OrgId, Source};

use crate::traits::{SchemaRegistry, SourceInfo, SourceSchema};
use super::state::InMemoryBackend;

#[async_trait]
impl SchemaRegistry for InMemoryBackend {
    async fn register_schema(&self, schema: &SourceSchema) -> Result<(), StoreError> {
        let mut store = self.schemas.write();
        let existing = store.iter().position(|s| {
            s.org_id == schema.org_id
                && s.source == schema.source
                && s.event_type == schema.event_type
        });

        if let Some(idx) = existing {
            if store[idx].version < schema.version {
                store[idx] = schema.clone();
            }
        } else {
            store.push(schema.clone());
        }

        Ok(())
    }

    async fn get_schema(
        &self,
        org_id: &OrgId,
        source: &Source,
        event_type: &EventType,
    ) -> Result<Option<SourceSchema>, StoreError> {
        let store = self.schemas.read();
        Ok(store
            .iter()
            .find(|s| s.org_id == *org_id && s.source == *source && s.event_type == *event_type)
            .cloned())
    }

    async fn describe_sources(&self, org_id: &OrgId) -> Result<Vec<SourceInfo>, StoreError> {
        let store = self.schemas.read();
        let events = self.events.read();

        let mut source_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for schema in store.iter().filter(|s| s.org_id == *org_id) {
            source_map
                .entry(schema.source.as_str().to_string())
                .or_default()
                .push(schema.event_type.as_str().to_string());
        }

        let results: Vec<SourceInfo> = source_map
            .into_iter()
            .map(|(source, event_types)| {
                let count = events
                    .values()
                    .filter(|e: &&chronicle_core::event::Event| e.org_id == *org_id && e.source.as_str() == source)
                    .count() as u64;

                SourceInfo {
                    source: Source::new(&source),
                    event_types: event_types.into_iter().map(|t| EventType::new(&t)).collect(),
                    event_count: count,
                    first_seen: None,
                    last_seen: None,
                }
            })
            .collect();

        Ok(results)
    }
}
