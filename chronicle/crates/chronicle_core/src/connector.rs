//! Generic SaaS connector trait for webhook-to-Event conversion.
//!
//! Each SaaS source (Stripe, Gorgias, Shopify, etc.) implements
//! [`SaasConnector`] to convert raw webhook JSON into Chronicle
//! [`Event`] objects with automatic entity ref extraction and
//! topic derivation.
//!
//! # Adding a new connector
//!
//! 1. Create a new crate `chronicle_{source}` depending on `chronicle_core`
//! 2. Implement [`SaasConnector`] for your connector struct
//! 3. Add `ch.ingest_{source}()` to the Python API

use crate::event::Event;

/// Errors from webhook conversion.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("Invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("Missing field: {0}")]
    MissingField(String),

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),
}

/// A SaaS webhook-to-Chronicle-Event converter.
///
/// Implementors parse raw webhook JSON from a specific SaaS source,
/// extract entity references, derive the topic, and produce a
/// Chronicle [`Event`] with the full payload preserved.
pub trait SaasConnector: Send + Sync {
    /// The source name used in Chronicle events (e.g., `"stripe"`, `"gorgias"`).
    fn source_name(&self) -> &'static str;

    /// Convert a raw webhook JSON body into a Chronicle [`Event`].
    fn convert(&self, json: &str, org_id: &str) -> Result<Event, ConnectorError>;

    /// Derive the Chronicle topic from the webhook's event type string.
    fn derive_topic(&self, event_type: &str) -> &'static str;

    /// Extract entity references from the webhook's data object.
    ///
    /// Returns `(entity_type, entity_id)` pairs.
    fn extract_entities(&self, data: &serde_json::Value) -> Vec<(String, String)>;
}

/// Batch-convert multiple webhook JSON bodies using a connector.
pub fn convert_batch(
    connector: &dyn SaasConnector,
    jsons: &[&str],
    org_id: &str,
) -> Vec<Result<Event, ConnectorError>> {
    jsons
        .iter()
        .map(|json| connector.convert(json, org_id))
        .collect()
}
