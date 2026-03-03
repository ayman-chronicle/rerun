//! Bridge between Chronicle and Rerun viewer.
//!
//! Maps Chronicle events to Rerun archetypes (`ChronicleEvent`, `Scalar`) and
//! logs them to a `RecordingStream` for visualization in the Rerun viewer.
//!
//! # Usage
//!
//! ```ignore
//! use chronicle_rerun_bridge::ChronicleBridge;
//!
//! let bridge = ChronicleBridge::new(engine, "chronicle_demo")?;
//! bridge.load_timeline(&timeline_query).await?;
//! // Events are now visible in the Rerun viewer.
//! ```
//!
//! # DRY
//!
//! - Arrow conversion reuses `chronicle_store::arrow_export` (one schema)
//! - All data flows through `StorageEngine` traits (one query path)
//! - Event formatting via shared `format_event_summary` (one formatter)

mod formatting;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::link::EventLink;
use chronicle_core::query::{EventResult, StructuredQuery, TimelineQuery};
use chronicle_store::StorageEngine;

pub use formatting::format_event_summary;

/// Bridge that sends Chronicle data to a Rerun `RecordingStream`.
pub struct ChronicleBridge {
    engine: StorageEngine,
    rec: rerun::RecordingStream,
}

impl ChronicleBridge {
    /// Create a bridge with a new Rerun recording.
    pub fn new(engine: StorageEngine, app_id: &str) -> Result<Self, StoreError> {
        let rec = rerun::RecordingStreamBuilder::new(app_id)
            .spawn()
            .map_err(|e| StoreError::Internal(format!("Rerun spawn: {e}")))?;

        Ok(Self { engine, rec })
    }

    /// Create a bridge with an existing `RecordingStream`.
    pub fn with_stream(engine: StorageEngine, rec: rerun::RecordingStream) -> Self {
        Self { engine, rec }
    }

    /// Load an entity's timeline into the Rerun viewer as `ChronicleEvent` entries.
    ///
    /// Each event becomes a `ChronicleEvent` at entity path `/{source}/{event_type}`,
    /// timestamped on the `event_time` timeline.
    pub async fn load_timeline(&self, query: &TimelineQuery) -> Result<usize, StoreError> {
        let results = self.engine.events.query_timeline(query).await?;
        self.log_events(&results)?;
        Ok(results.len())
    }

    /// Load structured query results into the Rerun viewer.
    pub async fn load_query(&self, query: &StructuredQuery) -> Result<usize, StoreError> {
        let results = self.engine.events.query_structured(query).await?;
        self.log_events(&results)?;
        Ok(results.len())
    }

    /// Load events as an Arrow dataframe into the Rerun viewer.
    ///
    /// Uses the shared `events_to_record_batch` from `arrow_export` (DRY).
    pub async fn load_as_dataframe(&self, query: &StructuredQuery) -> Result<usize, StoreError> {
        let results = self.engine.events.query_structured(query).await?;
        let events: Vec<Event> = results.iter().map(|r| r.event.clone()).collect();
        let _batch = chronicle_store::arrow_export::events_to_record_batch(&events)?;
        // RecordBatch is ready for send_dataframe when the Rerun API supports it.
        // For now, log as ChronicleEvent which works with the current viewer.
        self.log_events(&results)?;
        Ok(results.len())
    }

    /// Extract a numeric payload field and log as `Scalar` for time-series plots.
    pub async fn load_scalars(
        &self,
        query: &StructuredQuery,
        field_name: &str,
    ) -> Result<usize, StoreError> {
        let results = self.engine.events.query_structured(query).await?;
        let mut logged = 0;

        for r in &results {
            if let Some(ref payload) = r.event.payload {
                if let Some(value) = payload.get(field_name).and_then(serde_json::Value::as_f64) {
                    let path = format!(
                        "{}/{}/{}",
                        r.event.source.as_str(),
                        r.event.event_type.as_str(),
                        field_name
                    );

                    self.rec.set_timestamp_secs_since_epoch(
                        "event_time",
                        r.event.event_time.timestamp() as f64,
                    );

                    if let Err(e) = self.rec.log(path, &rerun::archetypes::Scalars::new([value])) {
                        tracing::warn!("Failed to log scalar: {e}");
                    } else {
                        logged += 1;
                    }
                }
            }
        }

        Ok(logged)
    }

    fn log_events(&self, results: &[EventResult]) -> Result<(), StoreError> {
        for r in results {
            let path = format!(
                "{}/{}",
                r.event.source.as_str(),
                r.event.event_type.as_str()
            );

            self.rec
                .set_timestamp_secs_since_epoch("event_time", r.event.event_time.timestamp() as f64);

            let mut chronicle_event = rerun::archetypes::ChronicleEvent::new(
                r.event.source.as_str(),
                r.event.event_type.as_str(),
            );

            chronicle_event = chronicle_event.with_topic(r.event.topic.as_str());

            if let Some(ref payload) = r.event.payload {
                let json_str = serde_json::to_string(payload).unwrap_or_default();
                chronicle_event = chronicle_event.with_payload(json_str.as_str());
            }

            let summary = format_event_summary(&r.event);
            chronicle_event = chronicle_event.with_label(summary);

            if let Err(e) = self.rec.log(path.as_str(), &chronicle_event) {
                tracing::warn!("Failed to log event: {e}");
            }

            if let Some(ref payload) = r.event.payload {
                let json = serde_json::to_string_pretty(payload).unwrap_or_default();
                let detail_path = format!("{path}/payload");
                if let Err(e) = self.rec.log(
                    detail_path.as_str(),
                    &rerun::archetypes::TextDocument::new(json)
                        .with_media_type("application/json"),
                ) {
                    tracing::warn!("Failed to log payload: {e}");
                }
            }
        }
        Ok(())
    }

    /// Log events and their links together into the Rerun viewer.
    ///
    /// Events are logged as `ChronicleEvent` at `{source}/{event_type}`.
    /// Links are logged as `ChronicleLink` at
    /// `_links/{src_source}/{src_type}/to/{tgt_source}/{tgt_type}/{link_type}`,
    /// encoding the source/target entity paths in the link entity path so the
    /// viewer can resolve arcs without component queries.
    pub fn log_events_with_links(
        &self,
        results: &[EventResult],
        links: &[EventLink],
    ) -> Result<(usize, usize), StoreError> {
        let mut id_to_path: std::collections::HashMap<String, (String, i64)> =
            std::collections::HashMap::new();

        for r in results {
            let path = format!(
                "{}/{}",
                r.event.source.as_str(),
                r.event.event_type.as_str()
            );
            let epoch = r.event.event_time.timestamp();
            id_to_path.insert(r.event.event_id.to_string(), (path, epoch));
        }

        self.log_events(results)?;
        let event_count = results.len();

        let mut link_count = 0;
        for link in links {
            let src_id = link.source_event_id.to_string();
            let tgt_id = link.target_event_id.to_string();

            if let (Some((src_path, src_epoch)), Some((tgt_path, tgt_epoch))) =
                (id_to_path.get(&src_id), id_to_path.get(&tgt_id))
            {
                let link_entity = format!(
                    "_links/{}/{}/to/{}/{}/{}",
                    src_path, src_epoch, tgt_path, tgt_epoch, link.link_type
                );

                let archetype = rerun::archetypes::ChronicleLink::new(
                    src_path.as_str(),
                    tgt_path.as_str(),
                    link.link_type.as_str(),
                )
                .with_confidence(
                    rerun::components::ChronicleConfidence::from(link.confidence.value()),
                )
                .with_reasoning(link.reasoning.as_deref().unwrap_or(""));

                if let Err(e) = self.rec.log_static(link_entity.as_str(), &archetype) {
                    tracing::warn!("Failed to log link: {e}");
                } else {
                    link_count += 1;
                }
            }
        }

        Ok((event_count, link_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use chronicle_core::event::EventBuilder;
    use chronicle_core::ids::*;
    use chronicle_core::query::OrderBy;
    use chronicle_store::memory::InMemoryBackend;
    use chronicle_store::traits::EventStore;

    fn test_engine() -> StorageEngine {
        let backend = Arc::new(InMemoryBackend::new());
        StorageEngine {
            events: backend.clone(),
            entity_refs: backend.clone(),
            links: backend.clone(),
            embeddings: backend.clone(),
            schemas: backend.clone(),
        }
    }

    #[tokio::test]
    async fn bridge_logs_timeline_events() {
        let engine = test_engine();

        let events = vec![
            EventBuilder::new("org_1", "stripe", "payments", "charge.created")
                .entity("customer", "cust_1")
                .payload(serde_json::json!({"amount": 4999}))
                .build(),
            EventBuilder::new("org_1", "support", "tickets", "ticket.created")
                .entity("customer", "cust_1")
                .payload(serde_json::json!({"subject": "Help"}))
                .build(),
        ];
        engine.events.insert_events(&events).await.unwrap();

        let rec = rerun::RecordingStreamBuilder::new("test")
            .buffered()
            .unwrap();
        let bridge = ChronicleBridge::with_stream(engine, rec);

        let query = TimelineQuery {
            org_id: OrgId::new("org_1"),
            entity_type: EntityType::new("customer"),
            entity_id: EntityId::new("cust_1"),
            time_range: None,
            sources: None,
            include_linked: false,
            include_entity_refs: false,
            link_depth: 0,
            min_link_confidence: 0.0,
        };

        let count = bridge.load_timeline(&query).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn bridge_logs_scalars() {
        let engine = test_engine();

        let events: Vec<_> = (0..5)
            .map(|i| {
                EventBuilder::new("org_1", "stripe", "payments", "charge.created")
                    .payload(serde_json::json!({"amount": 1000 + i * 500}))
                    .build()
            })
            .collect();
        engine.events.insert_events(&events).await.unwrap();

        let rec = rerun::RecordingStreamBuilder::new("test_scalar")
            .buffered()
            .unwrap();
        let bridge = ChronicleBridge::with_stream(engine, rec);

        let query = StructuredQuery {
            org_id: OrgId::new("org_1"),
            source: Some(Source::new("stripe")),
            entity: None,
            topic: None,
            event_type: None,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeAsc,
            limit: 100,
            offset: 0,
        };

        let count = bridge.load_scalars(&query, "amount").await.unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn format_event_summary_works() {
        let event = EventBuilder::new("org", "stripe", "pay", "charge.created")
            .payload(serde_json::json!({"amount": 4999, "currency": "usd"}))
            .build();
        let summary = format_event_summary(&event);
        assert!(summary.contains("charge.created"));
        assert!(summary.contains("amount"));
    }
}
