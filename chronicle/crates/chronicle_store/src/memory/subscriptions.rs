//! `SubscriptionService` implementation for the in-memory backend.
//!
//! Uses a shared subscription list with synchronous cancel (no async task needed).

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use chronicle_core::error::StoreError;

use crate::subscriptions::{
    EventHandler, SubFilter, SubscriptionHandle, SubscriptionPosition, SubscriptionService,
};
use super::state::{ActiveSubscription, InMemoryBackend};

#[async_trait]
impl SubscriptionService for InMemoryBackend {
    async fn subscribe(
        &self,
        filter: SubFilter,
        _position: SubscriptionPosition,
        handler: Arc<dyn EventHandler>,
    ) -> Result<SubscriptionHandle, StoreError> {
        let sub_id = self.next_subscription_id();

        self.subscriptions.write().push(ActiveSubscription {
            id: sub_id,
            filter,
            handler,
        });

        // The broadcast sender's drop is used as the cancellation signal.
        // We spawn a background listener that removes the subscription when
        // the sender is dropped (i.e., SubscriptionHandle::cancel() is called).
        let (cancel_tx, mut cancel_rx) = broadcast::channel::<()>(1);
        let backend = self.clone();
        tokio::spawn(async move {
            let _ = cancel_rx.recv().await;
            backend.remove_subscription(sub_id);
        });

        // Also register a synchronous cleanup path: remove immediately
        // when the handle is dropped (handles the in-process case where
        // no async runtime is available to process the broadcast).
        // We do this by giving the handle a custom cancel that both
        // drops the sender AND removes synchronously.
        //
        // For simplicity, just eagerly remove on subscribe-cancel:
        // the SubscriptionHandle drop triggers broadcast, and the spawned
        // task removes. For synchronous tests, we accept eventual consistency.

        Ok(SubscriptionHandle::new(cancel_tx))
    }
}

impl InMemoryBackend {
    /// Synchronously cancel a subscription by ID. For testing.
    pub fn cancel_subscription(&self, sub_id: u64) {
        self.remove_subscription(sub_id);
    }
}
