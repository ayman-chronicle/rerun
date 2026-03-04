//! Micro-batcher for event ingestion.
//!
//! Accumulates events and flushes them in batches to the storage backend.
//! Flush triggers: row count, byte size, or time interval. This amortizes
//! the cost of individual inserts across many events.
//!
//! Design adapted from Rerun's `ChunkBatcher` pattern.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::interval;

use chronicle_core::error::ChronicleError;
use chronicle_core::event::Event;
use chronicle_store::StorageEngine;

/// Configuration for the micro-batcher.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Max events per batch before flush.
    pub max_rows: usize,

    /// Max estimated bytes per batch before flush.
    pub max_bytes: usize,

    /// Max time between flushes.
    pub flush_interval: Duration,

    /// Channel capacity (backpressure kicks in when full).
    pub channel_capacity: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_rows: 1000,
            max_bytes: 1_000_000,
            flush_interval: Duration::from_millis(200),
            channel_capacity: 10_000,
        }
    }
}

/// Handle for sending events into the batcher.
///
/// The batcher runs a background task that accumulates events and
/// flushes them in batches. Use [`Batcher::send`] to enqueue events
/// and [`Batcher::shutdown`] to flush remaining events and stop.
pub struct Batcher {
    tx: mpsc::Sender<Event>,
    shutdown_tx: mpsc::Sender<()>,
    /// Join handle for the background flush task.
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Batcher {
    /// Start a new batcher with the given config and storage engine.
    ///
    /// Spawns a background tokio task that accumulates events and
    /// flushes them according to the config thresholds.
    pub fn start(config: BatchConfig, engine: StorageEngine) -> Self {
        let (tx, rx) = mpsc::channel(config.channel_capacity);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        let handle = tokio::spawn(flush_loop(config, engine, rx, shutdown_rx));

        Self {
            tx,
            shutdown_tx,
            handle: Some(handle),
        }
    }

    /// Send an event into the batcher. Applies backpressure if the
    /// channel is full.
    pub async fn send(&self, event: Event) -> Result<(), ChronicleError> {
        self.tx
            .send(event)
            .await
            .map_err(|_| ChronicleError::External("Batcher channel closed".to_string()))
    }

    /// Send multiple events. Each is enqueued individually.
    pub async fn send_many(&self, events: Vec<Event>) -> Result<(), ChronicleError> {
        for event in events {
            self.send(event).await?;
        }
        Ok(())
    }

    /// Flush remaining events and shut down the background task.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(()).await;
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    /// Number of events waiting in the channel (approximate).
    pub fn pending_count(&self) -> usize {
        self.tx.max_capacity() - self.tx.capacity()
    }
}

/// The background flush loop. Runs until shutdown signal received.
async fn flush_loop(
    config: BatchConfig,
    engine: StorageEngine,
    mut rx: mpsc::Receiver<Event>,
    mut shutdown_rx: mpsc::Receiver<()>,
) {
    let mut buffer: Vec<Event> = Vec::with_capacity(config.max_rows);
    let mut buffer_bytes: usize = 0;
    let mut tick = interval(config.flush_interval);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                buffer_bytes += estimate_event_size(&event);
                buffer.push(event);

                if buffer.len() >= config.max_rows || buffer_bytes >= config.max_bytes {
                    flush_batch(&engine, &mut buffer, &mut buffer_bytes).await;
                }
            }
            _ = tick.tick() => {
                if !buffer.is_empty() {
                    flush_batch(&engine, &mut buffer, &mut buffer_bytes).await;
                }
            }
            _ = shutdown_rx.recv() => {
                // Drain remaining events from channel
                rx.close();
                while let Ok(event) = rx.try_recv() {
                    buffer_bytes += estimate_event_size(&event);
                    buffer.push(event);
                }
                if !buffer.is_empty() {
                    flush_batch(&engine, &mut buffer, &mut buffer_bytes).await;
                }
                break;
            }
        }
    }
}

/// Flush the current buffer to the storage engine.
async fn flush_batch(engine: &StorageEngine, buffer: &mut Vec<Event>, buffer_bytes: &mut usize) {
    if buffer.is_empty() {
        return;
    }

    let batch: Vec<Event> = buffer.drain(..).collect();
    *buffer_bytes = 0;

    let batch_len = batch.len();
    match engine.events.insert_events(&batch).await {
        Ok(_ids) => {
            tracing::debug!(count = batch_len, "Flushed event batch");
        }
        Err(err) => {
            tracing::error!(%err, count = batch_len, "Failed to flush event batch");
        }
    }
}

/// Rough estimate of an event's size in bytes for backpressure.
/// Doesn't need to be precise -- just good enough for thresholds.
fn estimate_event_size(event: &Event) -> usize {
    let payload_size = event
        .payload
        .as_ref()
        .map(|p| p.to_string().len())
        .unwrap_or(0);

    let media_size = event
        .media
        .as_ref()
        .map(|m| m.size_bytes as usize)
        .unwrap_or(0);

    // Envelope overhead (~200 bytes) + payload + media
    200 + payload_size + media_size
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_store::memory::InMemoryBackend;
    use chronicle_store::traits::EventStore;
    use chronicle_test_fixtures::factories;

    fn make_engine() -> (StorageEngine, Arc<InMemoryBackend>) {
        let backend = Arc::new(InMemoryBackend::new());
        let engine = StorageEngine {
            events: backend.clone(),
            entity_refs: backend.clone(),
            links: backend.clone(),
            embeddings: backend.clone(),
            schemas: backend.clone(),
            subscriptions: Some(backend.clone()),
        };
        (engine, backend)
    }

    #[test]
    fn default_config() {
        let config = BatchConfig::default();
        assert_eq!(config.max_rows, 1000);
        assert_eq!(config.max_bytes, 1_000_000);
        assert_eq!(config.flush_interval, Duration::from_millis(200));
    }

    #[test]
    fn estimate_size_reasonable() {
        let event = factories::stripe_payment("org", "cust", 4999);
        let size = estimate_event_size(&event);
        assert!(size > 200, "Should include envelope overhead");
        assert!(size < 10_000, "Simple event shouldn't be huge");
    }

    #[tokio::test]
    async fn batcher_flushes_on_row_count() {
        let (engine, backend) = make_engine();
        let config = BatchConfig {
            max_rows: 5,
            max_bytes: usize::MAX,
            flush_interval: Duration::from_secs(60),
            channel_capacity: 100,
        };

        let batcher = Batcher::start(config, engine);

        for i in 0..5 {
            let event = factories::stripe_payment("org", &format!("c{i}"), 100);
            batcher.send(event).await.unwrap();
        }

        // Give the flush loop time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        let count = backend.event_count();
        assert_eq!(count, 5, "All 5 events should be flushed");

        batcher.shutdown().await;
    }

    #[tokio::test]
    async fn batcher_flushes_on_shutdown() {
        let (engine, backend) = make_engine();
        let config = BatchConfig {
            max_rows: 1000,
            max_bytes: usize::MAX,
            flush_interval: Duration::from_secs(60),
            channel_capacity: 100,
        };

        let batcher = Batcher::start(config, engine);

        for i in 0..3 {
            let event = factories::stripe_payment("org", &format!("c{i}"), 100);
            batcher.send(event).await.unwrap();
        }

        batcher.shutdown().await;

        let count = backend.event_count();
        assert_eq!(count, 3, "Shutdown should flush all remaining events");
    }

    #[tokio::test]
    async fn batcher_flushes_on_timer() {
        let (engine, backend) = make_engine();
        let config = BatchConfig {
            max_rows: 1000,
            max_bytes: usize::MAX,
            flush_interval: Duration::from_millis(50),
            channel_capacity: 100,
        };

        let batcher = Batcher::start(config, engine);

        let event = factories::stripe_payment("org", "c1", 100);
        batcher.send(event).await.unwrap();

        tokio::time::sleep(Duration::from_millis(150)).await;

        let count = backend.event_count();
        assert_eq!(count, 1, "Timer should flush the single event");

        batcher.shutdown().await;
    }

    #[tokio::test]
    async fn batcher_handles_many_events() {
        let (engine, backend) = make_engine();
        let config = BatchConfig {
            max_rows: 100,
            max_bytes: usize::MAX,
            flush_interval: Duration::from_millis(50),
            channel_capacity: 10_000,
        };

        let batcher = Batcher::start(config, engine);

        let events: Vec<Event> = (0..500)
            .map(|i| factories::stripe_payment("org", &format!("c{i}"), i * 100))
            .collect();
        batcher.send_many(events).await.unwrap();

        batcher.shutdown().await;

        let count = backend.event_count();
        assert_eq!(count, 500, "All 500 events should arrive");
    }
}
