CREATE TABLE source_revisions (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK(kind = 'http_audio'),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 256),
    endpoint TEXT NOT NULL CHECK(length(endpoint) BETWEEN 1 AND 2048),
    network_scope TEXT NOT NULL CHECK(network_scope IN ('public_internet', 'pinned_address')),
    pinned_address TEXT,
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    CHECK((network_scope = 'public_internet' AND pinned_address IS NULL)
        OR (network_scope = 'pinned_address' AND pinned_address IS NOT NULL))
) STRICT;

CREATE TRIGGER source_revisions_no_update BEFORE UPDATE ON source_revisions BEGIN
    SELECT RAISE(ABORT, 'source revisions are immutable');
END;
CREATE TRIGGER source_revisions_no_delete BEFORE DELETE ON source_revisions BEGIN
    SELECT RAISE(ABORT, 'source revisions preserve capture provenance');
END;
PRAGMA user_version = 3;
