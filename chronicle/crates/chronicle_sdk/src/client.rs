//! Chronicle HTTP client and event builder.

use std::collections::HashMap;

use chronicle_core::query::EventResult;
use serde::{Deserialize, Serialize};

/// SDK-level error type.
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Client configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Server endpoint (e.g., "http://localhost:3000").
    pub endpoint: String,

    /// Organization ID for all requests.
    pub org_id: String,

    /// Optional API key for authentication.
    pub api_key: Option<String>,
}

/// Chronicle HTTP client.
///
/// Provides a fluent API for logging events, querying data, and
/// managing entity links. All methods are async.
#[derive(Clone)]
pub struct ChronicleClient {
    config: ClientConfig,
    http: reqwest::Client,
}

impl ChronicleClient {
    /// Create a new client with the given configuration.
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Start building an event to log.
    ///
    /// Returns an [`EventBuilder`] for fluent configuration.
    pub fn log<'a>(
        &'a self,
        source: &'a str,
        topic: &'a str,
        event_type: &'a str,
    ) -> EventBuilder<'a> {
        EventBuilder {
            client: self,
            source,
            topic,
            event_type,
            entities: HashMap::new(),
            payload: None,
            timestamp: None,
        }
    }

    /// Query events with structured filters.
    pub async fn query_events(
        &self,
        source: Option<&str>,
        event_type: Option<&str>,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EventResult>, SdkError> {
        let mut params = vec![
            ("org_id", self.config.org_id.clone()),
            ("limit", limit.to_string()),
        ];
        if let Some(s) = source {
            params.push(("source", s.to_string()));
        }
        if let Some(t) = event_type {
            params.push(("event_type", t.to_string()));
        }
        if let Some(et) = entity_type {
            params.push(("entity_type", et.to_string()));
        }
        if let Some(eid) = entity_id {
            params.push(("entity_id", eid.to_string()));
        }

        let resp = self.get("/v1/events", &params).await?;
        Ok(resp)
    }

    /// Get the timeline for a specific entity.
    pub async fn timeline(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<Vec<EventResult>, SdkError> {
        let url = format!("{}/v1/timeline/{}/{}", self.config.endpoint, entity_type, entity_id);
        let resp = self
            .http
            .get(&url)
            .query(&[("org_id", &self.config.org_id)])
            .send()
            .await?;

        handle_response(resp).await
    }

    /// Add an entity ref to an existing event.
    pub async fn add_entity_ref(
        &self,
        event_id: &str,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<(), SdkError> {
        let body = serde_json::json!({
            "event_id": event_id,
            "entity_type": entity_type,
            "entity_id": entity_id,
        });
        self.post_json::<serde_json::Value>("/v1/entity-refs", &body).await?;
        Ok(())
    }

    /// Link all events of one entity to another entity (JIT linking).
    pub async fn link_entity(
        &self,
        from_entity_type: &str,
        from_entity_id: &str,
        to_entity_type: &str,
        to_entity_id: &str,
    ) -> Result<u64, SdkError> {
        let body = serde_json::json!({
            "org_id": self.config.org_id,
            "from_entity_type": from_entity_type,
            "from_entity_id": from_entity_id,
            "to_entity_type": to_entity_type,
            "to_entity_id": to_entity_id,
        });

        let resp: LinkEntityResp = self.post_json("/v1/link-entity", &body).await?;
        Ok(resp.linked_count)
    }

    // ----- Internal helpers -----

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T, SdkError> {
        let url = format!("{}{}", self.config.endpoint, path);
        let resp = self.http.get(&url).query(params).send().await?;
        handle_response(resp).await
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, SdkError> {
        let url = format!("{}{}", self.config.endpoint, path);
        let resp = self.http.post(&url).json(body).send().await?;
        handle_response(resp).await
    }
}

/// Fluent builder for logging events.
pub struct EventBuilder<'a> {
    client: &'a ChronicleClient,
    source: &'a str,
    topic: &'a str,
    event_type: &'a str,
    entities: HashMap<String, String>,
    payload: Option<serde_json::Value>,
    timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

impl<'a> EventBuilder<'a> {
    /// Attach an entity ref.
    pub fn entity(mut self, entity_type: &str, entity_id: &str) -> Self {
        self.entities.insert(entity_type.to_string(), entity_id.to_string());
        self
    }

    /// Attach a JSON payload.
    pub fn payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Set the event timestamp.
    pub fn timestamp(mut self, ts: chrono::DateTime<chrono::Utc>) -> Self {
        self.timestamp = Some(ts);
        self
    }

    /// Send the event to the server.
    pub async fn send(self) -> Result<Vec<String>, SdkError> {
        let body = serde_json::json!({
            "org_id": self.client.config.org_id,
            "source": self.source,
            "topic": self.topic,
            "event_type": self.event_type,
            "entities": self.entities,
            "payload": self.payload,
            "timestamp": self.timestamp,
        });

        let resp: IngestResp = self.client.post_json("/v1/events", &body).await?;
        Ok(resp.event_ids)
    }
}

#[derive(Deserialize)]
struct IngestResp {
    event_ids: Vec<String>,
}

#[derive(Deserialize)]
struct LinkEntityResp {
    linked_count: u64,
}

/// Check HTTP status and deserialize response body.
async fn handle_response<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T, SdkError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(SdkError::Api {
            status: status.as_u16(),
            message: body,
        });
    }
    Ok(resp.json().await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_config() {
        let config = ClientConfig {
            endpoint: "http://localhost:3000".to_string(),
            org_id: "org_1".to_string(),
            api_key: None,
        };
        let client = ChronicleClient::new(config.clone());
        assert_eq!(client.config.endpoint, "http://localhost:3000");
    }

    #[test]
    fn event_builder_entities() {
        let config = ClientConfig {
            endpoint: "http://localhost:3000".to_string(),
            org_id: "org_1".to_string(),
            api_key: None,
        };
        let client = ChronicleClient::new(config);
        let builder = client
            .log("stripe", "payments", "charge.created")
            .entity("customer", "cust_1")
            .entity("account", "acc_2")
            .payload(serde_json::json!({"amount": 100}));

        assert_eq!(builder.entities.len(), 2);
        assert_eq!(builder.entities["customer"], "cust_1");
        assert!(builder.payload.is_some());
    }
}
