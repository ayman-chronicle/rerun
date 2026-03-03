//! REST API routes.
//!
//! Each module defines handlers for one domain. All routes are
//! assembled into a single axum [`Router`] via [`build_router`].

pub mod discovery;
pub mod ingest;
pub mod links;
pub mod queries;
mod error;

use axum::Router;
use crate::ServerState;

/// Build the full REST API router with all routes.
pub fn build_router(state: ServerState) -> Router {
    Router::new()
        .merge(ingest::routes())
        .merge(queries::routes())
        .merge(links::routes())
        .merge(discovery::routes())
        .with_state(state)
}
