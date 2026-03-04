//! Internal SQL query builder for the Postgres backend.
//!
//! Provides a clean, composable API for building parameterized SQL.
//! Column lists are defined once. Optional filters are added fluently.
//! Parameter indices are tracked automatically.
//!
//! # Example
//!
//! ```ignore
//! let mut qb = SelectBuilder::events()
//!     .where_org("org_1")
//!     .where_source("stripe")
//!     .where_time_range(range)
//!     .order_by(OrderBy::EventTimeDesc)
//!     .limit(50);
//!
//! let (sql, params) = qb.build();
//! ```

use chronicle_core::query::OrderBy;

/// All columns for full event queries (get_event, query_timeline).
pub const EVENT_COLUMNS: &str =
    "e.event_id, e.org_id, e.source, e.topic, e.event_type, e.event_time, \
     e.ingestion_time, e.payload, e.media_type, e.media_ref, e.media_blob, \
     e.media_size_bytes, e.raw_body";

/// Envelope-only columns for listing queries. Skips payload, media, and
/// raw_body — avoids JSONB deserialization and large column transfer.
pub const EVENT_COLUMNS_LIGHT: &str =
    "e.event_id, e.org_id, e.source, e.topic, e.event_type, e.event_time, \
     e.ingestion_time";

/// A parameterized SQL value to bind at execution time.
#[derive(Debug, Clone)]
pub enum ParamValue {
    Text(String),
    Timestamp(chrono::DateTime<chrono::Utc>),
    Float(f32),
    Int(i32),
    Int64(i64),
}

/// Composable SELECT query builder.
///
/// Tracks parameter indices automatically so you never write `$1`, `$2`
/// manually. Optional filters are no-ops when `None`.
pub struct SelectBuilder {
    select: String,
    from: String,
    joins: Vec<String>,
    conditions: Vec<String>,
    params: Vec<ParamValue>,
    order: Option<String>,
    limit_val: Option<usize>,
    next_param: u32,
}

impl SelectBuilder {
    /// Start a SELECT on the events table with all event columns.
    pub fn events() -> Self {
        Self {
            select: EVENT_COLUMNS.to_string(),
            from: "events e".to_string(),
            joins: Vec::new(),
            conditions: Vec::new(),
            params: Vec::new(),
            order: None,
            limit_val: None,
            next_param: 1,
        }
    }

    /// Start a SELECT with envelope-only columns (no payload/media/raw_body).
    ///
    /// Use for listing and filtering queries where the caller doesn't need
    /// the full event body. Dramatically reduces deserialization cost and
    /// network transfer for large result sets.
    pub fn events_light() -> Self {
        Self {
            select: EVENT_COLUMNS_LIGHT.to_string(),
            from: "events e".to_string(),
            joins: Vec::new(),
            conditions: Vec::new(),
            params: Vec::new(),
            order: None,
            limit_val: None,
            next_param: 1,
        }
    }

    /// Start a SELECT with custom columns.
    pub fn custom(columns: &str, from: &str) -> Self {
        Self {
            select: columns.to_string(),
            from: from.to_string(),
            joins: Vec::new(),
            conditions: Vec::new(),
            params: Vec::new(),
            order: None,
            limit_val: None,
            next_param: 1,
        }
    }

    /// Add a JOIN clause.
    pub fn join(mut self, join_clause: &str) -> Self {
        self.joins.push(join_clause.to_string());
        self
    }

    /// Add a JOIN to entity_refs (needed for entity-scoped queries).
    pub fn join_entity_refs(self) -> Self {
        self.join("JOIN entity_refs er ON e.event_id = er.event_id")
    }

    // --- Required filter (always applied) ---

    /// Filter by org_id (required on every query for tenant isolation).
    pub fn where_org(mut self, org_id: &str) -> Self {
        let p = self.next_param();
        self.conditions.push(format!("e.org_id = ${p}"));
        self.params.push(ParamValue::Text(org_id.to_string()));
        self
    }

    // --- Optional filters (no-op when None) ---

    /// Filter by source. No-op if `None`.
    pub fn where_source(mut self, source: Option<&str>) -> Self {
        if let Some(s) = source {
            let p = self.next_param();
            self.conditions.push(format!("e.source = ${p}"));
            self.params.push(ParamValue::Text(s.to_string()));
        }
        self
    }

    /// Filter by event_type. No-op if `None`.
    pub fn where_event_type(mut self, event_type: Option<&str>) -> Self {
        if let Some(t) = event_type {
            let p = self.next_param();
            self.conditions.push(format!("e.event_type = ${p}"));
            self.params.push(ParamValue::Text(t.to_string()));
        }
        self
    }

    /// Filter by entity (requires entity_refs join). Legacy — prefer `where_entity_jsonb`.
    pub fn where_entity(mut self, entity_type: Option<&str>, entity_id: Option<&str>) -> Self {
        if let (Some(et), Some(eid)) = (entity_type, entity_id) {
            let p1 = self.next_param();
            let p2 = self.next_param();
            self.conditions.push(format!("er.entity_type = ${p1}"));
            self.conditions.push(format!("er.entity_id = ${p2}"));
            self.params.push(ParamValue::Text(et.to_string()));
            self.params.push(ParamValue::Text(eid.to_string()));
        }
        self
    }

    /// Filter by entity using JSONB `@>` on the embedded `_entity_refs` array.
    ///
    /// No JOIN required — queries the `payload` column directly with GIN index
    /// support. The payload contains `{"_entity_refs": [{"type":"T","id":"I"}]}`.
    pub fn where_entity_jsonb(mut self, entity_type: &str, entity_id: &str) -> Self {
        let p = self.next_param();
        self.conditions
            .push(format!("e.payload->'_entity_refs' @> ${p}::jsonb"));
        let ref_json = serde_json::json!([{ "type": entity_type, "id": entity_id }]);
        self.params
            .push(ParamValue::Text(ref_json.to_string()));
        self
    }

    /// Filter by time range. No-op if `None`.
    pub fn where_time_range(mut self, range: Option<&chronicle_core::time_range::TimeRange>) -> Self {
        if let Some(r) = range {
            let p1 = self.next_param();
            let p2 = self.next_param();
            self.conditions.push(format!("e.event_time >= ${p1}"));
            self.conditions.push(format!("e.event_time <= ${p2}"));
            self.params.push(ParamValue::Timestamp(r.min()));
            self.params.push(ParamValue::Timestamp(r.max()));
        }
        self
    }

    /// Set ORDER BY clause.
    pub fn order_by(mut self, order: &OrderBy) -> Self {
        self.order = Some(match order {
            OrderBy::EventTimeAsc => "e.event_time ASC".to_string(),
            OrderBy::EventTimeDesc => "e.event_time DESC".to_string(),
            OrderBy::IngestionTimeAsc => "e.ingestion_time ASC".to_string(),
            OrderBy::IngestionTimeDesc => "e.ingestion_time DESC".to_string(),
        });
        self
    }

    /// Set LIMIT.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit_val = Some(n);
        self
    }

    /// Build the final SQL string and parameter list.
    pub fn build(self) -> (String, Vec<ParamValue>) {
        let mut sql = format!("SELECT {} FROM {}", self.select, self.from);

        for join in &self.joins {
            sql.push(' ');
            sql.push_str(join);
        }

        if !self.conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.conditions.join(" AND "));
        }

        if let Some(ref order) = self.order {
            sql.push_str(" ORDER BY ");
            sql.push_str(order);
        }

        if let Some(limit) = self.limit_val {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        (sql, self.params)
    }

    /// Allocate the next parameter index.
    fn next_param(&mut self) -> u32 {
        let p = self.next_param;
        self.next_param += 1;
        p
    }
}

/// Bind [`ParamValue`]s to a sqlx query. Returns the bound query.
///
/// This is the bridge between our builder and sqlx's runtime binding.
pub fn bind_params<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    params: &'q [ParamValue],
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    for param in params {
        query = match param {
            ParamValue::Text(s) => query.bind(s.as_str()),
            ParamValue::Timestamp(t) => query.bind(t),
            ParamValue::Float(f) => query.bind(f),
            ParamValue::Int(i) => query.bind(i),
            ParamValue::Int64(i) => query.bind(i),
        };
    }
    query
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_event_query() {
        let (sql, params) = SelectBuilder::events()
            .where_org("org_1")
            .order_by(&OrderBy::EventTimeDesc)
            .limit(50)
            .build();

        assert!(sql.contains("SELECT e.event_id"));
        assert!(sql.contains("FROM events e"));
        assert!(sql.contains("WHERE e.org_id = $1"));
        assert!(sql.contains("ORDER BY e.event_time DESC"));
        assert!(sql.contains("LIMIT 50"));
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn query_with_all_filters() {
        let (sql, params) = SelectBuilder::events()
            .join_entity_refs()
            .where_org("org_1")
            .where_source(Some("stripe"))
            .where_event_type(Some("charge.created"))
            .where_entity(Some("customer"), Some("cust_1"))
            .order_by(&OrderBy::EventTimeDesc)
            .limit(10)
            .build();

        assert!(sql.contains("JOIN entity_refs"));
        assert!(sql.contains("e.org_id = $1"));
        assert!(sql.contains("e.source = $2"));
        assert!(sql.contains("e.event_type = $3"));
        assert!(sql.contains("er.entity_type = $4"));
        assert!(sql.contains("er.entity_id = $5"));
        assert_eq!(params.len(), 5);
    }

    #[test]
    fn optional_filters_are_skipped() {
        let (sql, params) = SelectBuilder::events()
            .where_org("org_1")
            .where_source(None)
            .where_event_type(None)
            .where_entity(None, None)
            .build();

        assert_eq!(params.len(), 1, "Only org_id should be bound");
        assert!(!sql.contains("e.source"));
    }

    #[test]
    fn param_indices_are_sequential() {
        let (sql, _) = SelectBuilder::events()
            .where_org("o")
            .where_source(Some("s"))
            .where_event_type(Some("t"))
            .build();

        assert!(sql.contains("$1"));
        assert!(sql.contains("$2"));
        assert!(sql.contains("$3"));
        assert!(!sql.contains("$4"));
    }
}
