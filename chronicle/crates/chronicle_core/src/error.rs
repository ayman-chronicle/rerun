//! Shared error types for Chronicle.
//!
//! Each crate may define its own specific error enum, but this module
//! provides the common error that surfaces at API boundaries.

/// Top-level error type for Chronicle operations.
#[derive(Debug, thiserror::Error)]
pub enum ChronicleError {
    /// Storage backend error (Postgres, Parquet, etc.).
    #[error("storage error: {0}")]
    Store(#[from] StoreError),

    /// Validation error on input data.
    #[error("validation error: {0}")]
    Validation(#[from] ValidationError),

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// External service error (embedding model, media storage, etc.).
    #[error("external service error: {0}")]
    External(String),
}

/// Error from the storage layer.
///
/// Backend implementations convert their native errors into this type
/// so the service layer doesn't depend on any specific backend.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found: {entity} with id {id:?}")]
    NotFound { entity: &'static str, id: String },

    #[error("duplicate: {entity} with id {id:?} already exists")]
    Duplicate { entity: &'static str, id: String },

    #[error("query error: {0}")]
    Query(String),

    #[error("connection error: {0}")]
    Connection(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("internal storage error: {0}")]
    Internal(String),
}

/// Error validating input data.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("missing required field: {0}")]
    MissingField(&'static str),

    #[error("invalid value for {field}: {reason}")]
    InvalidValue {
        field: &'static str,
        reason: String,
    },

    #[error("self-link not allowed: event cannot link to itself")]
    SelfLink,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = StoreError::NotFound {
            entity: "event",
            id: "evt_123".to_string(),
        };
        assert_eq!(err.to_string(), "not found: event with id \"evt_123\"");
    }

    #[test]
    fn chronicle_error_from_store() {
        let store_err = StoreError::Connection("timeout".to_string());
        let err: ChronicleError = store_err.into();
        assert!(matches!(err, ChronicleError::Store(_)));
    }

    #[test]
    fn validation_error_display() {
        let err = ValidationError::InvalidValue {
            field: "confidence",
            reason: "must be between 0.0 and 1.0".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("confidence"));
        assert!(msg.contains("0.0 and 1.0"));
    }
}
