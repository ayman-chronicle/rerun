//! Tenant ownership validation for cross-cutting write operations.
//!
//! [`TenantGuard`] wraps a [`StorageEngine`] and verifies that the
//! target event belongs to the caller's org before allowing entity ref
//! additions or link creation. This prevents a user in org_A from
//! attaching data to events owned by org_B.
//!
//! # Usage
//!
//! ```ignore
//! let guard = TenantGuard::new(engine.clone(), OrgId::new("org_1"));
//! guard.assert_event_owned(&event_id).await?;
//! ```

use chronicle_core::error::StoreError;
use chronicle_core::ids::{EventId, OrgId};

use crate::StorageEngine;

/// Validates that events belong to the expected tenant before writes.
pub struct TenantGuard {
    engine: StorageEngine,
    org_id: OrgId,
}

impl TenantGuard {
    pub fn new(engine: StorageEngine, org_id: OrgId) -> Self {
        Self { engine, org_id }
    }

    pub fn org_id(&self) -> &OrgId {
        &self.org_id
    }

    /// Verify the event exists and belongs to this tenant.
    ///
    /// Returns `StoreError::NotFound` if the event doesn't exist or
    /// belongs to a different org (same error to avoid information leakage).
    pub async fn assert_event_owned(&self, event_id: &EventId) -> Result<(), StoreError> {
        let result = self.engine.events.get_event(&self.org_id, event_id).await?;
        match result {
            Some(r) if r.event.org_id == self.org_id => Ok(()),
            _ => Err(StoreError::NotFound {
                entity: "event",
                id: event_id.to_string(),
            }),
        }
    }

    /// Verify both source and target events belong to this tenant.
    pub async fn assert_link_owned(
        &self,
        source_event_id: &EventId,
        target_event_id: &EventId,
    ) -> Result<(), StoreError> {
        self.assert_event_owned(source_event_id).await?;
        self.assert_event_owned(target_event_id).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::InMemoryBackend;
    use chronicle_core::event::EventBuilder;
    use std::sync::Arc;

    fn make_engine() -> StorageEngine {
        let backend = Arc::new(InMemoryBackend::new());
        StorageEngine {
            events: backend.clone(),
            entity_refs: backend.clone(),
            links: backend.clone(),
            embeddings: backend.clone(),
            schemas: backend.clone(),
            subscriptions: Some(backend.clone()),
        }
    }

    #[tokio::test]
    async fn owned_event_passes() {
        let engine = make_engine();
        let event = EventBuilder::new("org_1", "stripe", "payments", "charge.created").build();
        let eid = event.event_id;
        engine.events.insert_events(&[event]).await.unwrap();

        let guard = TenantGuard::new(engine, OrgId::new("org_1"));
        assert!(guard.assert_event_owned(&eid).await.is_ok());
    }

    #[tokio::test]
    async fn wrong_org_fails() {
        let engine = make_engine();
        let event = EventBuilder::new("org_1", "stripe", "payments", "charge.created").build();
        let eid = event.event_id;
        engine.events.insert_events(&[event]).await.unwrap();

        let guard = TenantGuard::new(engine, OrgId::new("org_2"));
        assert!(guard.assert_event_owned(&eid).await.is_err());
    }

    #[tokio::test]
    async fn nonexistent_event_fails() {
        let engine = make_engine();
        let guard = TenantGuard::new(engine, OrgId::new("org_1"));
        assert!(guard.assert_event_owned(&EventId::new()).await.is_err());
    }

    #[tokio::test]
    async fn link_ownership_both_must_pass() {
        let engine = make_engine();
        let e1 = EventBuilder::new("org_1", "stripe", "payments", "charge.created").build();
        let e2 = EventBuilder::new("org_2", "stripe", "payments", "charge.created").build();
        let id1 = e1.event_id;
        let id2 = e2.event_id;
        engine.events.insert_events(&[e1, e2]).await.unwrap();

        let guard = TenantGuard::new(engine, OrgId::new("org_1"));
        assert!(guard.assert_link_owned(&id1, &id2).await.is_err());
    }
}
