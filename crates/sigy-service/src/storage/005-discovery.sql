CREATE TABLE directory_refreshes (
    id TEXT PRIMARY KEY NOT NULL,
    request_json TEXT NOT NULL CHECK(length(request_json) <= 8192),
    state TEXT NOT NULL CHECK(state IN ('running', 'completed', 'failed', 'interrupted')),
    started_ms INTEGER NOT NULL CHECK(started_ms >= 0),
    completed_ms INTEGER CHECK(completed_ms >= started_ms),
    accepted INTEGER NOT NULL DEFAULT 0 CHECK(accepted BETWEEN 0 AND 500),
    skipped INTEGER NOT NULL DEFAULT 0 CHECK(skipped BETWEEN 0 AND 500),
    mirror_origin TEXT,
    failure TEXT CHECK(length(failure) <= 256)
) STRICT;
CREATE UNIQUE INDEX one_directory_refresh ON directory_refreshes(state) WHERE state = 'running';
CREATE TRIGGER directory_request_immutable BEFORE UPDATE OF id, request_json, started_ms ON directory_refreshes BEGIN
    SELECT RAISE(ABORT, 'directory request is immutable');
END;
CREATE TABLE directory_stations (
    provider TEXT NOT NULL CHECK(provider = 'radio_browser'),
    id TEXT NOT NULL,
    metadata_json TEXT NOT NULL CHECK(length(metadata_json) <= 8192),
    endpoint TEXT NOT NULL CHECK(length(endpoint) <= 2048),
    name_folded TEXT NOT NULL,
    country TEXT NOT NULL,
    languages_folded TEXT NOT NULL,
    tags_folded TEXT NOT NULL,
    healthy INTEGER CHECK(healthy IN (0, 1)),
    refresh_id TEXT NOT NULL REFERENCES directory_refreshes(id),
    PRIMARY KEY(provider, id)
) STRICT;
CREATE INDEX station_country ON directory_stations(country, id);
CREATE TABLE source_directory_links (
    source_revision TEXT PRIMARY KEY NOT NULL REFERENCES source_revisions(id),
    provider TEXT NOT NULL,
    station_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL CHECK(length(metadata_json) <= 8192)
) STRICT;
CREATE TRIGGER source_directory_link_immutable BEFORE UPDATE ON source_directory_links BEGIN
    SELECT RAISE(ABORT, 'source directory provenance is immutable');
END;
CREATE TRIGGER source_directory_link_retained BEFORE DELETE ON source_directory_links BEGIN
    SELECT RAISE(ABORT, 'source directory provenance is retained');
END;
PRAGMA user_version = 5;
