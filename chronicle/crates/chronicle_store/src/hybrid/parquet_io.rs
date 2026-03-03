//! Parquet I/O utilities: Arrow schema, event <-> RecordBatch conversion,
//! Parquet read/write.

use std::path::Path;
use std::sync::Arc;

use datafusion::arrow::array::{
    ArrayRef, Int64Builder, RecordBatch, StringBuilder, TimestampMicrosecondBuilder,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, TimeUnit};

use chronicle_core::error::StoreError;
use chronicle_core::event::Event;
use chronicle_core::ids::*;
use chronicle_core::media::MediaAttachment;
use chronicle_core::query::EventResult;

/// The Arrow schema for event Parquet files.
///
/// Covers all envelope fields plus serialized payload. Media blobs
/// are NOT stored in Parquet (only the external ref).
pub fn event_arrow_schema() -> Schema {
    Schema::new(vec![
        Field::new("event_id", DataType::Utf8, false),
        Field::new("org_id", DataType::Utf8, false),
        Field::new("source", DataType::Utf8, false),
        Field::new("topic", DataType::Utf8, false),
        Field::new("event_type", DataType::Utf8, false),
        Field::new(
            "event_time",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new(
            "ingestion_time",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("payload", DataType::Utf8, true),
        Field::new("media_type", DataType::Utf8, true),
        Field::new("media_ref", DataType::Utf8, true),
        Field::new("media_size_bytes", DataType::Int64, true),
        Field::new("raw_body", DataType::Utf8, true),
    ])
}

/// Convert a slice of [`Event`]s into an Arrow [`RecordBatch`].
pub fn events_to_record_batch(events: &[Event]) -> Result<RecordBatch, StoreError> {
    let schema = Arc::new(event_arrow_schema());
    let n = events.len();

    let mut event_id = StringBuilder::with_capacity(n, n * 36);
    let mut org_id = StringBuilder::with_capacity(n, n * 16);
    let mut source = StringBuilder::with_capacity(n, n * 16);
    let mut topic = StringBuilder::with_capacity(n, n * 16);
    let mut event_type = StringBuilder::with_capacity(n, n * 32);
    let mut event_time = TimestampMicrosecondBuilder::with_capacity(n);
    let mut ingestion_time = TimestampMicrosecondBuilder::with_capacity(n);
    let mut payload = StringBuilder::with_capacity(n, n * 256);
    let mut media_type = StringBuilder::with_capacity(n, n * 16);
    let mut media_ref = StringBuilder::with_capacity(n, n * 64);
    let mut media_size = Int64Builder::with_capacity(n);
    let mut raw_body = StringBuilder::with_capacity(n, n * 64);

    for e in events {
        event_id.append_value(e.event_id.to_string());
        org_id.append_value(e.org_id.as_str());
        source.append_value(e.source.as_str());
        topic.append_value(e.topic.as_str());
        event_type.append_value(e.event_type.as_str());
        event_time.append_value(e.event_time.timestamp_micros());
        ingestion_time.append_value(e.ingestion_time.timestamp_micros());

        match &e.payload {
            Some(p) => payload.append_value(serde_json::to_string(p).unwrap_or_default()),
            None => payload.append_null(),
        }

        match &e.media {
            Some(m) => {
                media_type.append_value(&m.media_type);
                match &m.external_ref {
                    Some(r) => media_ref.append_value(r),
                    None => media_ref.append_null(),
                }
                media_size.append_value(m.size_bytes as i64);
            }
            None => {
                media_type.append_null();
                media_ref.append_null();
                media_size.append_null();
            }
        }

        match &e.raw_body {
            Some(b) => raw_body.append_value(b),
            None => raw_body.append_null(),
        }
    }

    let columns: Vec<ArrayRef> = vec![
        Arc::new(event_id.finish()),
        Arc::new(org_id.finish()),
        Arc::new(source.finish()),
        Arc::new(topic.finish()),
        Arc::new(event_type.finish()),
        Arc::new(
            event_time
                .finish()
                .with_timezone("UTC"),
        ),
        Arc::new(
            ingestion_time
                .finish()
                .with_timezone("UTC"),
        ),
        Arc::new(payload.finish()),
        Arc::new(media_type.finish()),
        Arc::new(media_ref.finish()),
        Arc::new(media_size.finish()),
        Arc::new(raw_body.finish()),
    ];

    RecordBatch::try_new(schema, columns).map_err(|e| StoreError::Internal(e.to_string()))
}

/// Convert Arrow [`RecordBatch`]es back into [`EventResult`]s.
pub fn batches_to_event_results(batches: &[RecordBatch]) -> Vec<EventResult> {
    let mut results = Vec::new();
    for batch in batches {
        for i in 0..batch.num_rows() {
            if let Some(event) = row_to_event(batch, i) {
                results.push(EventResult {
                    event,
                    entity_refs: vec![],
                    search_distance: None,
                });
            }
        }
    }
    results
}

/// Extract a string value from a column that could be `StringArray` or
/// `StringViewArray` (DataFusion 52+ uses StringView for Parquet reads).
/// Extract a string value from a column that could be `StringArray` or
/// `StringViewArray` (DataFusion 52+ uses StringView for Parquet reads).
fn get_str<'a>(col: &'a dyn datafusion::arrow::array::Array, row: usize) -> Option<&'a str> {
    use datafusion::arrow::array::{Array, StringArray, StringViewArray};

    if col.is_null(row) {
        return None;
    }
    if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
        Some(arr.value(row))
    } else if let Some(arr) = col.as_any().downcast_ref::<StringViewArray>() {
        Some(arr.value(row))
    } else {
        None
    }
}

fn get_i64(col: &dyn datafusion::arrow::array::Array, row: usize) -> Option<i64> {
    use datafusion::arrow::array::{Array, Int64Array};

    if col.is_null(row) {
        return None;
    }
    col.as_any().downcast_ref::<Int64Array>().map(|a| a.value(row))
}

fn get_timestamp_us(col: &dyn datafusion::arrow::array::Array, row: usize) -> Option<i64> {
    use datafusion::arrow::array::{Array, TimestampMicrosecondArray};

    if col.is_null(row) {
        return None;
    }
    col.as_any()
        .downcast_ref::<TimestampMicrosecondArray>()
        .map(|a| a.value(row))
}

fn row_to_event(batch: &RecordBatch, row: usize) -> Option<Event> {
    let event_id_str = get_str(batch.column_by_name("event_id")?, row)?;
    let eid: EventId = event_id_str.parse().ok()?;

    let org_id = get_str(batch.column_by_name("org_id")?, row)?;
    let source = get_str(batch.column_by_name("source")?, row)?;
    let topic = get_str(batch.column_by_name("topic")?, row)?;
    let event_type = get_str(batch.column_by_name("event_type")?, row)?;

    let event_time_us = get_timestamp_us(batch.column_by_name("event_time")?, row)?;
    let ingestion_time_us = get_timestamp_us(batch.column_by_name("ingestion_time")?, row)?;

    let event_time = chrono::DateTime::from_timestamp_micros(event_time_us)?;
    let ingestion_time = chrono::DateTime::from_timestamp_micros(ingestion_time_us)?;

    let payload_col = batch.column_by_name("payload")?;
    let payload = get_str(payload_col.as_ref(), row)
        .and_then(|s| serde_json::from_str(s).ok());

    let media_type_col = batch.column_by_name("media_type")?;
    let media = get_str(media_type_col.as_ref(), row).map(|mt| {
        let media_ref_col = batch.column_by_name("media_ref").unwrap();
        let media_size_col = batch.column_by_name("media_size_bytes").unwrap();
        MediaAttachment {
            media_type: mt.to_string(),
            inline_blob: None,
            external_ref: get_str(media_ref_col.as_ref(), row).map(|s| s.to_string()),
            size_bytes: get_i64(media_size_col.as_ref(), row).unwrap_or(0) as u64,
        }
    });

    let raw_body_col = batch.column_by_name("raw_body")?;
    let raw_body = get_str(raw_body_col.as_ref(), row).map(|s| s.to_string());

    Some(Event {
        event_id: eid,
        org_id: OrgId::new(org_id),
        source: Source::new(source),
        topic: Topic::new(topic),
        event_type: EventType::new(event_type),
        event_time,
        ingestion_time,
        payload,
        media,
        entity_refs: vec![],
        raw_body,
    })
}

/// Build the Parquet directory path for a given (org, source, event_type).
pub fn parquet_dir_for_group(base: &Path, org_id: &str, source: &str, event_type: &str) -> std::path::PathBuf {
    base.join(org_id).join(source).join(event_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_core::event::EventBuilder;

    #[test]
    fn round_trip_events_through_arrow() {
        let events = vec![
            EventBuilder::new("org_1", "stripe", "payments", "payment_intent.succeeded")
                .payload(serde_json::json!({"amount": 4999}))
                .build(),
            EventBuilder::new("org_1", "support", "tickets", "ticket.created")
                .payload(serde_json::json!({"subject": "Help"}))
                .build(),
        ];

        let batch = events_to_record_batch(&events).expect("should create batch");
        assert_eq!(batch.num_rows(), 2);

        let results = batches_to_event_results(&[batch]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].event.source, "stripe");
        assert_eq!(results[1].event.source, "support");
        assert_eq!(results[0].event.event_id, events[0].event_id);
    }

    #[test]
    fn handles_null_payload_and_media() {
        let event = EventBuilder::new("org_1", "s", "t", "e").build();
        let batch = events_to_record_batch(&[event.clone()]).unwrap();
        let results = batches_to_event_results(&[batch]);
        assert_eq!(results.len(), 1);
        assert!(results[0].event.payload.is_none());
        assert!(results[0].event.media.is_none());
    }
}
