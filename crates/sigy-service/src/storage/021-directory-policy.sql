CREATE TABLE directory_refresh_policies (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 64),
    request_json TEXT NOT NULL CHECK(length(request_json) <= 8192),
    interval_ms INTEGER NOT NULL CHECK(interval_ms BETWEEN 3600000 AND 604800000),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    updated_ms INTEGER NOT NULL CHECK(updated_ms >= created_ms)
) STRICT;
CREATE TRIGGER directory_policy_identity_immutable BEFORE UPDATE OF id, created_ms ON directory_refresh_policies BEGIN
    SELECT RAISE(ABORT, 'directory policy identity is immutable');
END;
PRAGMA user_version = 21;
