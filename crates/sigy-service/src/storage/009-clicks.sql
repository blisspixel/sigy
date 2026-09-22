-- A directory click is one explicit request. The returned stream URL is not stored.
CREATE TABLE directory_clicks (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    station_id TEXT NOT NULL CHECK(length(station_id) = 36),
    request_json TEXT NOT NULL CHECK(length(request_json) BETWEEN 2 AND 4096),
    state TEXT NOT NULL CHECK(state IN ('running', 'completed', 'failed', 'interrupted')),
    started_ms INTEGER NOT NULL CHECK(started_ms >= 0),
    completed_ms INTEGER CHECK(completed_ms IS NULL OR completed_ms >= started_ms),
    mirror_origin TEXT CHECK(mirror_origin IS NULL OR length(mirror_origin) BETWEEN 1 AND 2048),
    acknowledged INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged IN (0, 1)),
    failure TEXT CHECK(failure IS NULL OR length(failure) <= 256)
) STRICT;
CREATE UNIQUE INDEX one_directory_click ON directory_clicks(state) WHERE state = 'running';
CREATE TRIGGER directory_click_immutable BEFORE UPDATE OF id, station_id, request_json, started_ms ON directory_clicks BEGIN
    SELECT RAISE(ABORT, 'directory click is immutable');
END;
PRAGMA user_version = 9;
