//! Automatic schema detection from event payloads.
//!
//! When a new `(source, event_type)` combination is seen, the detector
//! infers the payload schema from the JSON structure and registers it
//! in the [`SchemaRegistry`]. Subsequent events may trigger schema
//! evolution (new fields → new version).

use chronicle_core::ids::{EventType, OrgId, Source};
use chronicle_store::traits::{SchemaRegistry, SourceSchema};

/// Detect the schema of a JSON payload and register it if new.
///
/// Compares the payload's field names against the registered schema.
/// If new fields are found, registers a new version.
pub async fn detect_and_register(
    registry: &dyn SchemaRegistry,
    org_id: &OrgId,
    source: &Source,
    event_type: &EventType,
    payload: &serde_json::Value,
) -> Result<(), chronicle_core::error::StoreError> {
    let (field_names, field_types) = extract_fields(payload);
    if field_names.is_empty() {
        return Ok(());
    }

    let existing = registry.get_schema(org_id, source, event_type).await?;
    let needs_update = match &existing {
        None => true,
        Some(schema) => has_new_fields(&schema.field_names, &field_names),
    };

    if needs_update {
        let version = existing.map(|s| s.version + 1).unwrap_or(1);
        let schema = SourceSchema {
            org_id: *org_id,
            source: *source,
            event_type: *event_type,
            version,
            field_names,
            field_types,
            sample_event: Some(payload.clone()),
        };
        registry.register_schema(&schema).await?;
    }

    Ok(())
}

/// Extract top-level field names and their JSON types from a value.
fn extract_fields(value: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    match value.as_object() {
        Some(obj) => {
            let mut names = Vec::with_capacity(obj.len());
            let mut types = Vec::with_capacity(obj.len());
            for (key, val) in obj {
                names.push(key.clone());
                types.push(json_type_name(val).to_string());
            }
            (names, types)
        }
        None => (vec![], vec![]),
    }
}

/// Check if the new field list contains fields not in the existing schema.
fn has_new_fields(existing: &[String], new_fields: &[String]) -> bool {
    new_fields.iter().any(|f| !existing.contains(f))
}

/// Human-readable type name for a JSON value.
fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(n) if n.is_f64() => "float",
        serde_json::Value::Number(_) => "integer",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_store::memory::InMemoryBackend;

    #[test]
    fn extract_fields_from_object() {
        let payload = serde_json::json!({
            "amount": 4999,
            "currency": "usd",
            "succeeded": true,
            "metadata": {"key": "val"},
        });
        let (names, types) = extract_fields(&payload);
        assert_eq!(names.len(), 4);
        assert!(names.contains(&"amount".to_string()));
        assert!(types.contains(&"integer".to_string()));
        assert!(types.contains(&"string".to_string()));
        assert!(types.contains(&"boolean".to_string()));
        assert!(types.contains(&"object".to_string()));
    }

    #[test]
    fn extract_fields_from_non_object() {
        let (names, _types) = extract_fields(&serde_json::json!("not an object"));
        assert!(names.is_empty());
    }

    #[test]
    fn has_new_fields_detects_additions() {
        let existing = vec!["a".to_string(), "b".to_string()];
        assert!(!has_new_fields(&existing, &["a".to_string(), "b".to_string()]));
        assert!(has_new_fields(&existing, &["a".to_string(), "c".to_string()]));
    }

    #[tokio::test]
    async fn detect_registers_new_schema() {
        let backend = InMemoryBackend::new();
        let org = OrgId::new("org_1");
        let source = Source::new("stripe");
        let event_type = EventType::new("charge.created");
        let payload = serde_json::json!({"amount": 100, "currency": "usd"});

        detect_and_register(&backend, &org, &source, &event_type, &payload)
            .await
            .unwrap();

        let schema = backend.get_schema(&org, &source, &event_type).await.unwrap();
        assert!(schema.is_some());
        let schema = schema.unwrap();
        assert_eq!(schema.version, 1);
        assert!(schema.field_names.contains(&"amount".to_string()));
    }

    #[tokio::test]
    async fn detect_evolves_schema_on_new_fields() {
        let backend = InMemoryBackend::new();
        let org = OrgId::new("org_1");
        let source = Source::new("stripe");
        let event_type = EventType::new("charge.created");

        let v1 = serde_json::json!({"amount": 100});
        detect_and_register(&backend, &org, &source, &event_type, &v1).await.unwrap();

        let v2 = serde_json::json!({"amount": 200, "currency": "usd"});
        detect_and_register(&backend, &org, &source, &event_type, &v2).await.unwrap();

        let schema = backend.get_schema(&org, &source, &event_type).await.unwrap().unwrap();
        assert_eq!(schema.version, 2, "Should bump version on new field");
        assert!(schema.field_names.contains(&"currency".to_string()));
    }

    #[tokio::test]
    async fn detect_skips_if_no_new_fields() {
        let backend = InMemoryBackend::new();
        let org = OrgId::new("org_1");
        let source = Source::new("stripe");
        let event_type = EventType::new("charge.created");

        let payload = serde_json::json!({"amount": 100});
        detect_and_register(&backend, &org, &source, &event_type, &payload).await.unwrap();
        detect_and_register(&backend, &org, &source, &event_type, &payload).await.unwrap();

        let schema = backend.get_schema(&org, &source, &event_type).await.unwrap().unwrap();
        assert_eq!(schema.version, 1, "Should not bump version when no new fields");
    }
}
