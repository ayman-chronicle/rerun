//! Python bindings for Chronicle via PyO3.
//!
//! Exposes a Pythonic API where results are returned as JSON strings
//! that Python deserializes. This avoids complex PyO3 type conversions.
//!
//! ```python
//! import chronicle
//! import json
//!
//! ch = chronicle.Chronicle.in_memory()
//! event_id = ch.log("stripe", "payments", "charge.created",
//!                    entities={"customer": "cust_123"},
//!                    payload={"amount": 4999})
//!
//! results = json.loads(ch.query(source="stripe"))
//! timeline = json.loads(ch.timeline("customer", "cust_123"))
//! tools = json.loads(ch.agent_tools())
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use chronicle_core::event::EventBuilder;
use chronicle_core::ids::*;
use chronicle_core::link::EventLink;
use chronicle_core::query::*;
use chronicle_link::LinkService;
use chronicle_query::QueryService;
use chronicle_store::memory::InMemoryBackend;
use chronicle_store::StorageEngine;

/// The main Chronicle client for Python.
#[pyclass]
struct Chronicle {
    engine: StorageEngine,
    query: QueryService,
    link: LinkService,
    org_id: String,
    runtime: tokio::runtime::Runtime,
}

#[pymethods]
impl Chronicle {
    /// Create a Chronicle instance backed by in-memory storage.
    #[staticmethod]
    #[pyo3(signature = (org_id = "default"))]
    fn in_memory(org_id: &str) -> PyResult<Self> {
        let backend = Arc::new(InMemoryBackend::new());
        let engine = StorageEngine {
            events: backend.clone(),
            entity_refs: backend.clone(),
            links: backend.clone(),
            embeddings: backend.clone(),
            schemas: backend.clone(),
        };
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| PyValueError::new_err(format!("Runtime error: {e}")))?;

        Ok(Self {
            query: QueryService::new(engine.clone()),
            link: LinkService::new(engine.clone()),
            engine,
            org_id: org_id.to_string(),
            runtime,
        })
    }

    /// Log a single event. Returns the event ID.
    #[pyo3(signature = (source, topic, event_type, *, entities=None, payload=None))]
    fn log(
        &self,
        source: &str,
        topic: &str,
        event_type: &str,
        entities: Option<HashMap<String, String>>,
        payload: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<String> {
        let mut builder = EventBuilder::new(
            self.org_id.as_str(), source, topic, event_type,
        );

        if let Some(ents) = entities {
            for (etype, eid) in ents {
                builder = builder.entity(etype.as_str(), eid);
            }
        }

        if let Some(py_payload) = payload {
            let json_str = py_dict_to_json(py_payload)?;
            let value: serde_json::Value = serde_json::from_str(&json_str)
                .map_err(|e| PyValueError::new_err(format!("Invalid payload: {e}")))?;
            builder = builder.payload(value);
        }

        let event = builder.build();
        let event_id = event.event_id;

        self.runtime.block_on(async {
            self.engine.events.insert_events(&[event]).await
        }).map_err(|e| PyValueError::new_err(format!("Insert failed: {e}")))?;

        Ok(event_id.to_string())
    }

    /// Query events. Returns JSON string.
    #[pyo3(signature = (*, source=None, event_type=None, entity_type=None, entity_id=None, limit=50))]
    fn query(
        &self,
        source: Option<&str>,
        event_type: Option<&str>,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        limit: usize,
    ) -> PyResult<String> {
        let entity = match (entity_type, entity_id) {
            (Some(t), Some(id)) => Some((EntityType::new(t), EntityId::new(id))),
            _ => None,
        };

        let q = StructuredQuery {
            org_id: OrgId::new(&self.org_id),
            source: source.map(Source::new),
            topic: None,
            event_type: event_type.map(EventType::new),
            entity,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit,
            offset: 0,
        };

        let results = self.runtime.block_on(self.query.query(&q))
            .map_err(|e| PyValueError::new_err(format!("Query failed: {e}")))?;

        serde_json::to_string(&results)
            .map_err(|e| PyValueError::new_err(format!("Serialization failed: {e}")))
    }

    /// Get entity timeline. Returns JSON string.
    #[pyo3(signature = (entity_type, entity_id, *, include_linked=true))]
    fn timeline(
        &self,
        entity_type: &str,
        entity_id: &str,
        include_linked: bool,
    ) -> PyResult<String> {
        let q = TimelineQuery {
            org_id: OrgId::new(&self.org_id),
            entity_type: EntityType::new(entity_type),
            entity_id: EntityId::new(entity_id),
            time_range: None,
            sources: None,
            include_linked,
            include_entity_refs: true,
            link_depth: 1,
            min_link_confidence: 0.7,
        };

        let results = self.runtime.block_on(self.query.timeline(&q))
            .map_err(|e| PyValueError::new_err(format!("Timeline failed: {e}")))?;

        serde_json::to_string(&results)
            .map_err(|e| PyValueError::new_err(format!("Serialization failed: {e}")))
    }

    /// Add an entity ref to an existing event.
    fn add_entity_ref(&self, event_id: &str, entity_type: &str, entity_id: &str) -> PyResult<()> {
        let eid: EventId = event_id.parse()
            .map_err(|_| PyValueError::new_err("Invalid event_id"))?;

        self.runtime.block_on(self.link.add_entity_ref(eid, entity_type, entity_id, "python_sdk"))
            .map_err(|e| PyValueError::new_err(format!("{e}")))?;
        Ok(())
    }

    /// Link all events of one entity to another. Returns linked count.
    fn link_entity(
        &self,
        from_entity_type: &str,
        from_entity_id: &str,
        to_entity_type: &str,
        to_entity_id: &str,
    ) -> PyResult<u64> {
        self.runtime.block_on(self.link.link_entity(
            &OrgId::new(&self.org_id),
            from_entity_type, from_entity_id,
            to_entity_type, to_entity_id,
            "python_sdk",
        )).map_err(|e| PyValueError::new_err(format!("{e}")))
    }

    /// Create a causal link between two events. Returns link ID.
    #[pyo3(signature = (source_event_id, target_event_id, *, link_type="related_to", confidence=0.8, reasoning=None))]
    fn create_link(
        &self,
        source_event_id: &str,
        target_event_id: &str,
        link_type: &str,
        confidence: f32,
        reasoning: Option<&str>,
    ) -> PyResult<String> {
        let src: EventId = source_event_id.parse()
            .map_err(|_| PyValueError::new_err("Invalid source_event_id"))?;
        let tgt: EventId = target_event_id.parse()
            .map_err(|_| PyValueError::new_err("Invalid target_event_id"))?;
        let conf = Confidence::new(confidence)
            .map_err(|e| PyValueError::new_err(format!("{e}")))?;

        let link = EventLink {
            link_id: LinkId::new(),
            source_event_id: src,
            target_event_id: tgt,
            link_type: link_type.to_string(),
            confidence: conf,
            reasoning: reasoning.map(String::from),
            created_by: "python_sdk".to_string(),
            created_at: chrono::Utc::now(),
        };

        let id = self.runtime.block_on(self.link.create_link(&link))
            .map_err(|e| PyValueError::new_err(format!("{e}")))?;
        Ok(id.to_string())
    }

    /// Query events and return as Arrow IPC bytes.
    ///
    /// In Python, convert to a PyArrow Table:
    /// ```python
    /// import pyarrow as pa
    /// table = pa.ipc.open_stream(ch.query_df(source="stripe")).read_all()
    /// df = table.to_pandas()  # or table.to_pyarrow(), polars.from_arrow(table)
    /// ```
    ///
    /// The schema matches `event_arrow_schema()` -- the single source of
    /// truth for Chronicle Arrow data.
    #[pyo3(signature = (*, source=None, event_type=None, entity_type=None, entity_id=None, limit=1000))]
    fn query_df(
        &self,
        py: Python<'_>,
        source: Option<&str>,
        event_type: Option<&str>,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        limit: usize,
    ) -> PyResult<PyObject> {
        let entity = match (entity_type, entity_id) {
            (Some(t), Some(id)) => Some((EntityType::new(t), EntityId::new(id))),
            _ => None,
        };

        let q = StructuredQuery {
            org_id: OrgId::new(&self.org_id),
            source: source.map(Source::new),
            topic: None,
            event_type: event_type.map(EventType::new),
            entity,
            time_range: None,
            payload_filters: vec![],
            group_by: None,
            order_by: OrderBy::EventTimeDesc,
            limit,
            offset: 0,
        };

        let results = self.runtime.block_on(self.query.query(&q))
            .map_err(|e| PyValueError::new_err(format!("Query failed: {e}")))?;

        let events: Vec<chronicle_core::event::Event> =
            results.into_iter().map(|r| r.event).collect();

        let ipc_bytes = events_to_ipc_bytes(&events)?;
        Ok(pyo3::types::PyBytes::new(py, &ipc_bytes).into())
    }

    /// Get entity timeline as Arrow IPC bytes.
    ///
    /// ```python
    /// table = pa.ipc.open_stream(ch.timeline_df("customer", "cust_123")).read_all()
    /// ```
    #[pyo3(signature = (entity_type, entity_id, *, include_linked=true))]
    fn timeline_df(
        &self,
        py: Python<'_>,
        entity_type: &str,
        entity_id: &str,
        include_linked: bool,
    ) -> PyResult<PyObject> {
        let q = TimelineQuery {
            org_id: OrgId::new(&self.org_id),
            entity_type: EntityType::new(entity_type),
            entity_id: EntityId::new(entity_id),
            time_range: None,
            sources: None,
            include_linked,
            include_entity_refs: true,
            link_depth: 1,
            min_link_confidence: 0.7,
        };

        let results = self.runtime.block_on(self.query.timeline(&q))
            .map_err(|e| PyValueError::new_err(format!("Timeline failed: {e}")))?;

        let events: Vec<chronicle_core::event::Event> =
            results.into_iter().map(|r| r.event).collect();

        let ipc_bytes = events_to_ipc_bytes(&events)?;
        Ok(pyo3::types::PyBytes::new(py, &ipc_bytes).into())
    }

    /// List entity types. Returns JSON string.
    fn describe_entity_types(&self) -> PyResult<String> {
        let types = self.runtime.block_on(
            self.query.describe_entity_types(&OrgId::new(&self.org_id))
        ).map_err(|e| PyValueError::new_err(format!("{e}")))?;

        serde_json::to_string(&types)
            .map_err(|e| PyValueError::new_err(format!("{e}")))
    }

    /// Return AI agent tool definitions as JSON string (OpenAI format).
    fn agent_tools(&self) -> PyResult<String> {
        let tools = serde_json::json!([
            {
                "type": "function",
                "function": {
                    "name": "query_events",
                    "description": "Search for events by source, type, entity, and time range",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "source": {"type": "string"},
                            "event_type": {"type": "string"},
                            "entity_type": {"type": "string"},
                            "entity_id": {"type": "string"},
                            "limit": {"type": "integer", "default": 50}
                        }
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "customer_timeline",
                    "description": "Get chronological timeline for any entity",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "entity_type": {"type": "string"},
                            "entity_id": {"type": "string"},
                            "include_linked": {"type": "boolean", "default": true}
                        },
                        "required": ["entity_type", "entity_id"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "create_link",
                    "description": "Create a causal link between two events",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "source_event_id": {"type": "string"},
                            "target_event_id": {"type": "string"},
                            "link_type": {"type": "string"},
                            "confidence": {"type": "number"},
                            "reasoning": {"type": "string"}
                        },
                        "required": ["source_event_id", "target_event_id"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "link_entity",
                    "description": "Link all events of one entity to another",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "from_entity_type": {"type": "string"},
                            "from_entity_id": {"type": "string"},
                            "to_entity_type": {"type": "string"},
                            "to_entity_id": {"type": "string"}
                        },
                        "required": ["from_entity_type", "from_entity_id", "to_entity_type", "to_entity_id"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "add_entity_ref",
                    "description": "Associate an entity with an event",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "event_id": {"type": "string"},
                            "entity_type": {"type": "string"},
                            "entity_id": {"type": "string"}
                        },
                        "required": ["event_id", "entity_type", "entity_id"]
                    }
                }
            }
        ]);

        serde_json::to_string(&tools)
            .map_err(|e| PyValueError::new_err(format!("{e}")))
    }
}

/// Serialize events to Arrow IPC stream bytes.
///
/// Python deserializes with `pyarrow.ipc.open_stream(bytes).read_all()`.
/// Uses the shared `events_to_record_batch` from `arrow_export` (DRY).
fn events_to_ipc_bytes(events: &[chronicle_core::event::Event]) -> PyResult<Vec<u8>> {
    let batch = chronicle_store::arrow_export::events_to_record_batch(events)
        .map_err(|e| PyValueError::new_err(format!("Arrow conversion: {e}")))?;

    let mut buf = Vec::new();
    {
        let mut writer =
            arrow::ipc::writer::StreamWriter::try_new(&mut buf, &batch.schema())
                .map_err(|e| PyValueError::new_err(format!("IPC writer: {e}")))?;
        writer
            .write(&batch)
            .map_err(|e| PyValueError::new_err(format!("IPC write: {e}")))?;
        writer
            .finish()
            .map_err(|e| PyValueError::new_err(format!("IPC finish: {e}")))?;
    }
    Ok(buf)
}

fn py_dict_to_json(dict: &Bound<'_, PyDict>) -> PyResult<String> {
    let json_module = dict.py().import("json")?;
    let json_str = json_module.call_method1("dumps", (dict,))?;
    json_str.extract::<String>()
}

#[pymodule]
fn chronicle_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Chronicle>()?;
    Ok(())
}
