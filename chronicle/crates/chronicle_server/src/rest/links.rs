//! Link and entity ref management routes.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use chronicle_core::ids::{Confidence, EntityId, EntityType, EventId, LinkId, OrgId};
use chronicle_core::link::EventLink;
use chrono::Utc;

use crate::ServerState;
use super::error::ApiError;

pub fn routes() -> Router<ServerState> {
    Router::new()
        .route("/v1/entity-refs", post(add_entity_ref))
        .route("/v1/event-links", post(create_link))
        .route("/v1/link-entity", post(link_entity))
}

/// Request to add an entity ref to an existing event.
#[derive(Debug, Deserialize)]
pub struct AddEntityRefRequest {
    pub org_id: String,
    pub event_id: String,
    pub entity_type: String,
    pub entity_id: String,
    #[serde(default = "default_created_by")]
    pub created_by: String,
}

fn default_created_by() -> String {
    "api".to_string()
}

/// `POST /v1/entity-refs`
async fn add_entity_ref(
    State(state): State<ServerState>,
    Json(req): Json<AddEntityRefRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let event_id: EventId = req.event_id.parse()
        .map_err(|e: chronicle_core::ids::IdParseError| {
            chronicle_core::error::ChronicleError::Validation(
                chronicle_core::error::ValidationError::InvalidValue {
                    field: "event_id",
                    reason: e.to_string(),
                },
            )
        })?;

    state.link.add_entity_ref(
        &OrgId::new(&req.org_id),
        event_id,
        req.entity_type.as_str(),
        req.entity_id.as_str(),
        &req.created_by,
    ).await?;

    Ok(Json(serde_json::json!({"status": "ok"})))
}

/// Request to create a link between two events.
#[derive(Debug, Deserialize)]
pub struct CreateLinkRequest {
    pub org_id: String,
    pub source_event_id: String,
    pub target_event_id: String,
    pub link_type: String,
    pub confidence: f32,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default = "default_created_by")]
    pub created_by: String,
}

#[derive(Debug, Serialize)]
pub struct CreateLinkResponse {
    pub link_id: String,
}

/// `POST /v1/event-links`
async fn create_link(
    State(state): State<ServerState>,
    Json(req): Json<CreateLinkRequest>,
) -> Result<Json<CreateLinkResponse>, ApiError> {
    let source_id: EventId = req.source_event_id.parse()
        .map_err(|e: chronicle_core::ids::IdParseError| {
            chronicle_core::error::ChronicleError::Validation(
                chronicle_core::error::ValidationError::InvalidValue {
                    field: "source_event_id",
                    reason: e.to_string(),
                },
            )
        })?;

    let target_id: EventId = req.target_event_id.parse()
        .map_err(|e: chronicle_core::ids::IdParseError| {
            chronicle_core::error::ChronicleError::Validation(
                chronicle_core::error::ValidationError::InvalidValue {
                    field: "target_event_id",
                    reason: e.to_string(),
                },
            )
        })?;

    let confidence = Confidence::new(req.confidence)
        .map_err(|e| chronicle_core::error::ChronicleError::Validation(
            chronicle_core::error::ValidationError::InvalidValue {
                field: "confidence",
                reason: e.to_string(),
            },
        ))?;

    let link = EventLink {
        link_id: LinkId::new(),
        source_event_id: source_id,
        target_event_id: target_id,
        link_type: req.link_type,
        confidence,
        reasoning: req.reasoning,
        created_by: req.created_by,
        created_at: Utc::now(),
    };

    let id = state.link.create_link(&OrgId::new(&req.org_id), &link).await?;
    Ok(Json(CreateLinkResponse {
        link_id: id.to_string(),
    }))
}

/// Request to propagate entity refs (JIT linking).
#[derive(Debug, Deserialize)]
pub struct LinkEntityRequest {
    pub org_id: String,
    pub from_entity_type: String,
    pub from_entity_id: String,
    pub to_entity_type: String,
    pub to_entity_id: String,
    #[serde(default = "default_created_by")]
    pub created_by: String,
}

#[derive(Debug, Serialize)]
pub struct LinkEntityResponse {
    pub linked_count: u64,
}

/// `POST /v1/link-entity`
async fn link_entity(
    State(state): State<ServerState>,
    Json(req): Json<LinkEntityRequest>,
) -> Result<Json<LinkEntityResponse>, ApiError> {
    let count = state.link.link_entity(
        &OrgId::new(&req.org_id),
        req.from_entity_type.as_str(),
        req.from_entity_id.as_str(),
        req.to_entity_type.as_str(),
        req.to_entity_id.as_str(),
        &req.created_by,
    ).await?;

    Ok(Json(LinkEntityResponse { linked_count: count }))
}
