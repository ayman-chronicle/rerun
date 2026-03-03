-- Drop FK constraints to enable event archival to Parquet.
--
-- When the HybridBackend archives old events from Postgres to Parquet,
-- the event rows are deleted from the events table. Entity refs, links,
-- and embeddings remain in Postgres and reference event_ids that now
-- live in Parquet files.

ALTER TABLE entity_refs DROP CONSTRAINT IF EXISTS entity_refs_event_id_fkey;
ALTER TABLE event_links DROP CONSTRAINT IF EXISTS event_links_source_event_id_fkey;
ALTER TABLE event_links DROP CONSTRAINT IF EXISTS event_links_target_event_id_fkey;
ALTER TABLE event_embeddings DROP CONSTRAINT IF EXISTS event_embeddings_event_id_fkey;
