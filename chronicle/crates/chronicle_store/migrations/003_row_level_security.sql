-- Row-Level Security: defense-in-depth for multi-tenancy.
--
-- Even if application code has a bug that omits a WHERE org_id clause,
-- Postgres itself will reject cross-tenant rows.
--
-- Usage: before each request, set the session variable:
--   SET LOCAL app.current_org_id = 'org_xxx';
--
-- The special value '' (empty string) disables RLS filtering, which is
-- used by migrations and admin operations. Use current_setting with
-- missing_ok=true so queries work even if the variable is not set.

-- Enable RLS on all tenant-scoped tables.
ALTER TABLE events ENABLE ROW LEVEL SECURITY;
ALTER TABLE entity_refs ENABLE ROW LEVEL SECURITY;
ALTER TABLE event_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE event_embeddings ENABLE ROW LEVEL SECURITY;
ALTER TABLE source_schemas ENABLE ROW LEVEL SECURITY;

-- Policies: allow access only when the row's org_id matches the session var,
-- OR the session var is empty (admin/migration mode).
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE policyname = 'tenant_isolation_events') THEN
        CREATE POLICY tenant_isolation_events ON events
            USING (
                current_setting('app.current_org_id', true) = ''
                OR org_id = current_setting('app.current_org_id', true)
            );
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE policyname = 'tenant_isolation_refs') THEN
        CREATE POLICY tenant_isolation_refs ON entity_refs
            USING (
                current_setting('app.current_org_id', true) = ''
                OR org_id = current_setting('app.current_org_id', true)
            );
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE policyname = 'tenant_isolation_links') THEN
        CREATE POLICY tenant_isolation_links ON event_links
            USING (
                current_setting('app.current_org_id', true) = ''
                OR org_id = current_setting('app.current_org_id', true)
            );
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE policyname = 'tenant_isolation_embeddings') THEN
        CREATE POLICY tenant_isolation_embeddings ON event_embeddings
            USING (
                current_setting('app.current_org_id', true) = ''
                OR org_id = current_setting('app.current_org_id', true)
            );
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_policies WHERE policyname = 'tenant_isolation_schemas') THEN
        CREATE POLICY tenant_isolation_schemas ON source_schemas
            USING (
                current_setting('app.current_org_id', true) = ''
                OR org_id = current_setting('app.current_org_id', true)
            );
    END IF;
END $$;
