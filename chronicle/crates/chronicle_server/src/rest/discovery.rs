//! Discovery routes: list sources, entity types, schemas.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use chronicle_core::ids::{EntityType, EventType, OrgId, Source};
use chronicle_store::traits::{EntityInfo, EntityTypeInfo, SourceInfo, SourceSchema};

use crate::ServerState;
use super::error::ApiError;

pub fn routes() -> Router<ServerState> {
    Router::new()
        .route("/v1/discover/sources", get(describe_sources))
        .route("/v1/discover/entity-types", get(describe_entity_types))
        .route("/v1/discover/entities/{entity_type}", get(list_entities))
        .route("/v1/discover/schema/{source}/{event_type}", get(describe_schema))
}

#[derive(Debug, Deserialize)]
pub struct OrgQuery {
    pub org_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ListEntitiesQuery {
    pub org_id: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    100
}

/// `GET /v1/discover/sources?org_id=...`
async fn describe_sources(
    State(state): State<ServerState>,
    Query(params): Query<OrgQuery>,
) -> Result<Json<Vec<SourceInfo>>, ApiError> {
    let results = state.query.describe_sources(&OrgId::new(&params.org_id)).await?;
    Ok(Json(results))
}

/// `GET /v1/discover/entity-types?org_id=...`
async fn describe_entity_types(
    State(state): State<ServerState>,
    Query(params): Query<OrgQuery>,
) -> Result<Json<Vec<EntityTypeInfo>>, ApiError> {
    let results = state.query.describe_entity_types(&OrgId::new(&params.org_id)).await?;
    Ok(Json(results))
}

/// `GET /v1/discover/entities/{entity_type}?org_id=...&limit=100`
async fn list_entities(
    State(state): State<ServerState>,
    Path(entity_type): Path<String>,
    Query(params): Query<ListEntitiesQuery>,
) -> Result<Json<Vec<EntityInfo>>, ApiError> {
    let results = state.query.list_entities(
        &OrgId::new(&params.org_id),
        &EntityType::new(&entity_type),
        params.limit,
    ).await?;
    Ok(Json(results))
}

#[derive(Debug, Deserialize)]
pub struct SchemaPath {
    pub source: String,
    pub event_type: String,
}

/// `GET /v1/discover/schema/{source}/{event_type}?org_id=...`
async fn describe_schema(
    State(state): State<ServerState>,
    Path(path): Path<SchemaPath>,
    Query(params): Query<OrgQuery>,
) -> Result<Json<Option<SourceSchema>>, ApiError> {
    let result = state.query.describe_schema(
        &OrgId::new(&params.org_id),
        &Source::new(&path.source),
        &EventType::new(&path.event_type),
    ).await?;
    Ok(Json(result))
}
