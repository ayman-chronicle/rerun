//! Push subscription support via Kurrent persistent subscriptions.
//!
//! The [`SubscriptionService`] trait defines a backend-agnostic way to
//! receive events as they are written. The Kurrent implementation uses
//! native catch-up subscriptions on `$all` for low-latency push delivery.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::{EventType, OrgId, Source};

// ---------------------------------------------------------------------------
// Trait definition (backend-agnostic)
// ---------------------------------------------------------------------------

/// Position from which to start a subscription.
#[derive(Debug, Clone)]
pub enum SubscriptionPosition {
    /// From the very beginning of the stream/log.
    Beginning,
    /// From the current end (only new events).
    End,
}

/// Filter for which events a subscription should receive.
#[derive(Debug, Clone, Default)]
pub struct SubFilter {
    pub org_id: Option<OrgId>,
    pub sources: Option<Vec<Source>>,
    pub event_types: Option<Vec<EventType>>,
}

/// A handle to a running subscription. Drop to unsubscribe.
pub struct SubscriptionHandle {
    _cancel: broadcast::Sender<()>,
}

impl SubscriptionHandle {
    pub fn cancel(self) {
        drop(self._cancel);
    }
}

/// Receives events from a subscription.
#[async_trait]
pub trait EventHandler: Send + Sync + 'static {
    async fn handle(&self, event: &Event) -> Result<(), StoreError>;
}

/// Push-based event subscription service.
#[async_trait]
pub trait SubscriptionService: Send + Sync + 'static {
    async fn subscribe(
        &self,
        filter: SubFilter,
        position: SubscriptionPosition,
        handler: Arc<dyn EventHandler>,
    ) -> Result<SubscriptionHandle, StoreError>;
}

// ---------------------------------------------------------------------------
// Kurrent implementation
// ---------------------------------------------------------------------------

use super::KurrentBackend;

#[async_trait]
impl SubscriptionService for KurrentBackend {
    /// Subscribe to events via Kurrent's `$all` catch-up subscription.
    ///
    /// Spawns a background task that reads from `$all` and invokes the
    /// handler for each event matching the filter.
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

        Ok(SubscriptionHandle { _cancel: cancel_tx })
    }
}

fn matches_filter(event: &Event, filter: &SubFilter) -> bool {
    if let Some(ref org) = filter.org_id {
        if event.org_id != *org {
            return false;
        }
    }
    if let Some(ref sources) = filter.sources {
        if !sources.iter().any(|s| event.source == *s) {
            return false;
        }
    }
    if let Some(ref types) = filter.event_types {
        if !types.iter().any(|t| event.event_type == *t) {
            return false;
        }
    }
    true
}
