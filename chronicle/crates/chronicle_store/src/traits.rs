//! Storage trait definitions.
//!
//! Each trait covers one domain of storage. A backend may implement all
//! of them (like `PostgresBackend`) or delegate some to another backend
//! (like `KurrentBackend` delegating refs/links/embeddings to Postgres).

use async_trait::async_trait;
use std::sync::Arc;

use chronicle_core::{
    EntityRef, Event, EventLink, EventResult,
    GraphQuery, SemanticQuery, StructuredQuery, TimelineQuery,
    ids::{EntityId, EntityType, EventId, LinkId, OrgId, Source, EventType},
    error::StoreError,
};

// ---------------------------------------------------------------------------
// EventStore
// ---------------------------------------------------------------------------

/// Core event storage: insert, retrieve, and query events.
///
/// This is the primary trait. Every query mode except semantic search
/// and graph traversal goes through here.
#[async_trait]
pub trait EventStore: Send + Sync + 'static {
    /// Insert one or more events. Returns the generated event IDs.
    ///
    /// Entity refs from `event.entity_refs` should be materialized and
    /// stored via the [`EntityRefStore`] during this call.
    async fn insert_events(&self, events: &[Event]) -> Result<Vec<EventId>, StoreError>;

    /// Retrieve a single event by ID. Returns `None` if not found.
    async fn get_event(&self, org_id: &OrgId, id: &EventId) -> Result<Option<EventResult>, StoreError>;

    /// Run a structured query (filter on envelope fields, optionally with entity join).
    async fn query_structured(&self, query: &StructuredQuery) -> Result<Vec<EventResult>, StoreError>;

    /// Get the chronological timeline for a specific entity.
    async fn query_timeline(&self, query: &TimelineQuery) -> Result<Vec<EventResult>, StoreError>;

    /// Run a raw SQL query. The backend may restrict which tables/operations are allowed.
    async fn query_sql(&self, org_id: &OrgId, sql: &str) -> Result<Vec<EventResult>, StoreError>;

    /// Count events matching a structured query (without returning the events).
    async fn count(&self, query: &StructuredQuery) -> Result<u64, StoreError>;
}

// ---------------------------------------------------------------------------
// EntityRefStore
// ---------------------------------------------------------------------------

/// Dynamic entity reference storage.
///
/// Entity refs are the many-to-many mapping between events and typed
/// entities. They can be added at any time (during ingestion, by
/// enrichment agents, or by AI agents during investigation).
#[async_trait]
pub trait EntityRefStore: Send + Sync + 'static {
    /// Add one or more entity refs. Idempotent (duplicates are ignored).
    ///
    /// `org_id` is stored alongside the refs for tenant isolation.
    async fn add_refs(&self, org_id: &OrgId, refs: &[EntityRef]) -> Result<(), StoreError>;

    /// Get all entity refs for a specific event, scoped to the org.
    async fn get_refs_for_event(&self, org_id: &OrgId, event_id: &EventId) -> Result<Vec<EntityRef>, StoreError>;

    /// Get all event IDs associated with an entity.
    async fn get_events_for_entity(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        entity_id: &EntityId,
    ) -> Result<Vec<EventId>, StoreError>;

    /// Propagate entity refs: for every event that references `(from_type, from_id)`,
    /// add a new ref to `(to_type, to_id)`. Returns the number of new refs created.
    ///
    /// This powers JIT entity linking (e.g., linking all session events to a customer).
    async fn link_entity(
        &self,
        org_id: &OrgId,
        from_type: &EntityType,
        from_id: &EntityId,
        to_type: &EntityType,
        to_id: &EntityId,
        created_by: &str,
    ) -> Result<u64, StoreError>;

    /// List all entity types in use for an org.
    async fn list_entity_types(&self, org_id: &OrgId) -> Result<Vec<EntityTypeInfo>, StoreError>;

    /// List entities of a specific type.
    async fn list_entities(
        &self,
        org_id: &OrgId,
        entity_type: &EntityType,
        limit: usize,
    ) -> Result<Vec<EntityInfo>, StoreError>;
}

/// Summary of an entity type.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityTypeInfo {
    pub entity_type: EntityType,
    pub entity_count: u64,
    pub first_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
}

/// Summary of a specific entity instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityInfo {
    pub entity_type: EntityType,
    pub entity_id: EntityId,
    pub event_count: u64,
    pub first_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
}

// ---------------------------------------------------------------------------
// EventLinkStore
// ---------------------------------------------------------------------------

/// Event link (graph edge) storage.
///
/// Links represent directed causal or relational edges between events.
/// They are created by AI agents, automated rules, or users.
#[async_trait]
pub trait EventLinkStore: Send + Sync + 'static {
    /// Create a new link between two events. Validates no self-links.
    ///
    /// `org_id` is stored on the link row for tenant isolation.
    async fn create_link(&self, org_id: &OrgId, link: &EventLink) -> Result<LinkId, StoreError>;

    /// Get links from/to a specific event, scoped to the org.
    async fn get_links_for_event(&self, org_id: &OrgId, event_id: &EventId) -> Result<Vec<EventLink>, StoreError>;

    /// Traverse the link graph from a starting event.
    ///
    /// Follows links up to `max_depth` hops, filtering by direction,
    /// link types, and minimum confidence. Returns the events found
    /// along the traversal path.
    async fn traverse(&self, query: &GraphQuery) -> Result<Vec<EventResult>, StoreError>;
}

// ---------------------------------------------------------------------------
// EmbeddingStore
// ---------------------------------------------------------------------------

/// Embedding storage and vector search.
///
/// Stores vector embeddings alongside event IDs and supports
/// hybrid search (structured filters + vector similarity).
#[async_trait]
pub trait EmbeddingStore: Send + Sync + 'static {
    /// Store one or more event embeddings.
    async fn store_embeddings(&self, embeddings: &[EventEmbedding]) -> Result<(), StoreError>;

    /// Search for events by semantic similarity, optionally filtered
    /// by structured criteria.
    async fn search(&self, query: &SemanticQuery) -> Result<Vec<EventResult>, StoreError>;

    /// Check if an event already has an embedding stored, scoped to the org.
    async fn has_embedding(&self, org_id: &OrgId, event_id: &EventId) -> Result<bool, StoreError>;
}

/// An embedding for a single event.
#[derive(Debug, Clone)]
pub struct EventEmbedding {
    pub event_id: EventId,
    pub org_id: OrgId,
    pub embedding: Vec<f32>,
    pub embedded_text: String,
    pub model_version: String,
}

// ---------------------------------------------------------------------------
// SchemaRegistry
// ---------------------------------------------------------------------------

/// Schema registry for payload field discovery.
///
/// Tracks the structure of event payloads per (source, event_type) so
/// AI agents can discover what data exists without parsing JSON.
#[async_trait]
pub trait SchemaRegistry: Send + Sync + 'static {
    /// Register or update a schema. If a newer version already exists,
    /// this is a no-op.
    async fn register_schema(&self, schema: &SourceSchema) -> Result<(), StoreError>;

    /// Get the latest schema for a (source, event_type).
    async fn get_schema(
        &self,
        org_id: &OrgId,
        source: &Source,
        event_type: &EventType,
    ) -> Result<Option<SourceSchema>, StoreError>;

    /// List all sources and their metadata for an org.
    async fn describe_sources(&self, org_id: &OrgId) -> Result<Vec<SourceInfo>, StoreError>;
}

/// Schema of a specific (source, event_type) payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceSchema {
    pub org_id: OrgId,
    pub source: Source,
    pub event_type: EventType,
    pub version: u32,
    pub field_names: Vec<String>,
    pub field_types: Vec<String>,
    pub sample_event: Option<serde_json::Value>,
}

/// Summary of a connected data source.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceInfo {
    pub source: Source,
    pub event_types: Vec<EventType>,
    pub event_count: u64,
    pub first_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
}
