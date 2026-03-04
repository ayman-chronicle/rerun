//! Kurrent implementation of [`SubscriptionService`].
//!
//! Uses Kurrent's native `$all` catch-up subscription for low-latency
//! push delivery. Filter matching is applied client-side.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;

use crate::subscriptions::{
    EventHandler, SubFilter, SubscriptionHandle, SubscriptionPosition, SubscriptionService,
    matches_filter,
};
use super::KurrentBackend;

#[async_trait]
impl SubscriptionService for KurrentBackend {
    async fn subscribe(
        &self,
        filter: SubFilter,
        position: SubscriptionPosition,
        handler: Arc<dyn EventHandler>,
    ) -> Result<SubscriptionHandle, StoreError> {
        use kurrentdb::{StreamPosition, SubscribeToAllOptions};

        let (cancel_tx, mut cancel_rx) = broadcast::channel::<()>(1);
        let client = self.kurrent.clone();

        tokio::spawn(async move {
            let options = match position {
                SubscriptionPosition::Beginning => {
                    SubscribeToAllOptions::default().position(StreamPosition::Start)
                }
                SubscriptionPosition::End => {
                    SubscribeToAllOptions::default().position(StreamPosition::End)
                }
            };

            let mut sub = client.subscribe_to_all(&options).await;

            loop {
                tokio::select! {
                    _ = cancel_rx.recv() => {
                        tracing::info!("subscription cancelled");
                        break;
                    }
                    result = sub.next() => {
                        match result {
                            Ok(resolved) => {
                                let recorded = resolved.get_original_event();
                                let chronicle_event: Result<Event, _> =
                                    serde_json::from_slice(&recorded.data);

                                if let Ok(evt) = chronicle_event {
                                    if matches_filter(&evt, &filter) {
                                        if let Err(e) = handler.handle(&evt).await {
                                            tracing::warn!("handler error: {e}");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("subscription error: {e}");
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(SubscriptionHandle::new(cancel_tx))
    }
}
