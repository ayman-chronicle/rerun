//! `SchemaRegistry` for the hybrid backend -- delegates entirely to Postgres.
//!
//! Schema metadata always lives in Postgres.

use async_trait::async_trait;

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventType, OrgId, Source};

use crate::traits::{SchemaRegistry, SourceInfo, SourceSchema};
use super::HybridBackend;

#[async_trait]
impl SchemaRegistry for HybridBackend {
    async fn register_schema(&self, schema: &SourceSchema) -> Result<(), StoreError> {
        self.pg.register_schema(schema).await
    }

    async fn get_schema(
        &self,
        org_id: &OrgId,
        source: &Source,
        event_type: &EventType,
    ) -> Result<Option<SourceSchema>, StoreError> {
        self.pg.get_schema(org_id, source, event_type).await
    }

    async fn describe_sources(&self, org_id: &OrgId) -> Result<Vec<SourceInfo>, StoreError> {
        self.pg.describe_sources(org_id).await
    }
}
