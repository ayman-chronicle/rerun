//! Event ingestion routes.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use chronicle_core::event::EventBuilder;
use chronicle_core::ids::{EventId, EventType, OrgId, Source, Topic};
use chronicle_core::media::MediaAttachment;
use chronicle_store::traits::EventStore;

use crate::ServerState;
use super::error::ApiError;

pub fn routes() -> Router<ServerState> {
    Router::new()
        .route("/v1/events", post(ingest_event))
        .route("/v1/events/batch", post(ingest_batch))
}

/// Request body for ingesting a single event.
#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    pub org_id: String,
    pub source: String,
    pub topic: String,
    pub event_type: String,
    #[serde(default)]
    pub entities: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

/// Response body after ingesting events.
#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub event_ids: Vec<String>,
    pub count: usize,
}

/// `POST /v1/events` -- ingest a single event.
async fn ingest_event(
    State(state): State<ServerState>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, ApiError> {
    let event = build_event(req);
    let ids = state.engine.events.insert_events(&[event]).await
        .map_err(chronicle_core::error::ChronicleError::from)?;

    Ok(Json(IngestResponse {
        count: ids.len(),
        event_ids: ids.iter().map(ToString::to_string).collect(),
    }))
}

/// `POST /v1/events/batch` -- ingest multiple events.
async fn ingest_batch(
    State(state): State<ServerState>,
    Json(requests): Json<Vec<IngestRequest>>,
) -> Result<Json<IngestResponse>, ApiError> {
    let events: Vec<_> = requests.into_iter().map(build_event).collect();
    let ids = state.engine.events.insert_events(&events).await
        .map_err(chronicle_core::error::ChronicleError::from)?;

    Ok(Json(IngestResponse {
        count: ids.len(),
        event_ids: ids.iter().map(ToString::to_string).collect(),
    }))
}

/// Convert an HTTP request into a domain Event.
fn build_event(req: IngestRequest) -> chronicle_core::event::Event {
    let mut builder = EventBuilder::new(
        req.org_id.as_str(),
        req.source.as_str(),
        req.topic.as_str(),
        req.event_type.as_str(),
    );

    for (etype, eid) in &req.entities {
        builder = builder.entity(etype.as_str(), eid.clone());
    }

    if let Some(payload) = req.payload {
        builder = builder.payload(payload);
    }

    if let Some(ts) = req.timestamp {
        builder = builder.event_time(ts);
    }

    builder.build()
}
