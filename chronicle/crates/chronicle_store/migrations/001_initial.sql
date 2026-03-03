-- Chronicle initial schema.
-- All tables for events, entity refs, links, embeddings, and schema registry.

CREATE TABLE IF NOT EXISTS events (
    event_id         TEXT PRIMARY KEY,
    org_id           TEXT NOT NULL,
    source           TEXT NOT NULL,
    topic            TEXT NOT NULL,
    event_type       TEXT NOT NULL,
    event_time       TIMESTAMPTZ NOT NULL,
    ingestion_time   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload          JSONB,
    media_type       TEXT,
    media_ref        TEXT,
    media_blob       BYTEA,
    media_size_bytes BIGINT,
    raw_body         TEXT
);

CREATE INDEX IF NOT EXISTS idx_events_org_time
    ON events (org_id, event_time DESC);
CREATE INDEX IF NOT EXISTS idx_events_org_source_type
    ON events (org_id, source, event_type, event_time DESC);

-- Dynamic entity references (many-to-many between events and entities).
CREATE TABLE IF NOT EXISTS entity_refs (
    event_id     TEXT NOT NULL REFERENCES events(event_id),
    org_id       TEXT NOT NULL,
    entity_type  TEXT NOT NULL,
    entity_id    TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by   TEXT NOT NULL,
    PRIMARY KEY (event_id, entity_type, entity_id)
);

CREATE INDEX IF NOT EXISTS idx_refs_lookup
    ON entity_refs (org_id, entity_type, entity_id, event_id);

-- Causal/relational links between events.
CREATE TABLE IF NOT EXISTS event_links (
    link_id          TEXT PRIMARY KEY,
    org_id           TEXT NOT NULL,
    source_event_id  TEXT NOT NULL REFERENCES events(event_id),
    target_event_id  TEXT NOT NULL REFERENCES events(event_id),
    link_type        TEXT NOT NULL,
    confidence       REAL NOT NULL,
    reasoning        TEXT,
    created_by       TEXT NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_links_source
    ON event_links (org_id, source_event_id);
CREATE INDEX IF NOT EXISTS idx_links_target
    ON event_links (org_id, target_event_id);

-- Event embeddings for semantic search.
CREATE TABLE IF NOT EXISTS event_embeddings (
    event_id       TEXT PRIMARY KEY REFERENCES events(event_id),
    org_id         TEXT NOT NULL,
    embedding      REAL[] NOT NULL,
    embedded_text  TEXT,
    model_version  TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Schema registry for payload field discovery.
CREATE TABLE IF NOT EXISTS source_schemas (
    org_id       TEXT NOT NULL,
    source       TEXT NOT NULL,
    event_type   TEXT NOT NULL,
    version      INT NOT NULL,
    field_names  TEXT[] NOT NULL,
    field_types  TEXT[] NOT NULL,
    sample_event JSONB,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, source, event_type, version)
);
