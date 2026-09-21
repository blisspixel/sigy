ALTER TABLE source_revisions ADD COLUMN redirect_policy TEXT NOT NULL DEFAULT 'deny'
    CHECK(redirect_policy IN ('deny', 'same-origin', 'public')
        AND (redirect_policy != 'public' OR network_scope = 'public_internet'));
ALTER TABLE recordings ADD COLUMN http_route_json TEXT
    CHECK(http_route_json IS NULL OR length(http_route_json) <= 8192);
CREATE TRIGGER recording_route_immutable BEFORE UPDATE OF http_route_json ON recordings
    WHEN OLD.http_route_json IS NOT NULL BEGIN
    SELECT RAISE(ABORT, 'published HTTP provenance is immutable');
END;
PRAGMA user_version = 6;
