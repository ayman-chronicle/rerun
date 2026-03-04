-- GIN index on embedded entity refs for JSONB @> queries.
-- Supports entity-scoped queries without JOIN on entity_refs table.
CREATE INDEX IF NOT EXISTS idx_events_entity_jsonb
    ON events USING GIN ((payload->'_entity_refs'));
