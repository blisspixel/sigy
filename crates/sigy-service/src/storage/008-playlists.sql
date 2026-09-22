-- Playlist resolution is a durable request. Entry URLs are not ordinary view fields.
CREATE TABLE playlist_resolves (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    parent_revision TEXT NOT NULL REFERENCES source_revisions(id),
    state TEXT NOT NULL CHECK(state IN ('running', 'completed', 'failed', 'interrupted')),
    started_ms INTEGER NOT NULL CHECK(started_ms >= 0),
    completed_ms INTEGER CHECK(completed_ms IS NULL OR completed_ms >= started_ms),
    document_sha256 TEXT CHECK(document_sha256 IS NULL OR length(document_sha256) = 64),
    final_origin TEXT CHECK(final_origin IS NULL OR length(final_origin) BETWEEN 1 AND 2048),
    failure TEXT CHECK(failure IS NULL OR length(failure) <= 256),
    entry_count INTEGER NOT NULL DEFAULT 0 CHECK(entry_count BETWEEN 0 AND 32)
) STRICT;
CREATE UNIQUE INDEX one_playlist_resolve ON playlist_resolves(state) WHERE state = 'running';
CREATE TRIGGER playlist_request_immutable BEFORE UPDATE OF id, parent_revision, started_ms ON playlist_resolves BEGIN
    SELECT RAISE(ABORT, 'playlist request is immutable');
END;
CREATE TABLE playlist_entries (
    resolve_id TEXT NOT NULL REFERENCES playlist_resolves(id),
    entry_index INTEGER NOT NULL CHECK(entry_index BETWEEN 0 AND 31),
    endpoint TEXT NOT NULL CHECK(length(endpoint) BETWEEN 1 AND 2048),
    origin TEXT NOT NULL CHECK(length(origin) BETWEEN 1 AND 2048),
    PRIMARY KEY(resolve_id, entry_index)
) STRICT;
CREATE TRIGGER playlist_entries_no_update BEFORE UPDATE ON playlist_entries BEGIN
    SELECT RAISE(ABORT, 'playlist entries are immutable');
END;
CREATE TRIGGER playlist_entries_no_delete BEFORE DELETE ON playlist_entries BEGIN
    SELECT RAISE(ABORT, 'playlist entries are retained');
END;
CREATE TABLE playlist_acceptances (
    resolve_id TEXT NOT NULL,
    entry_index INTEGER NOT NULL CHECK(entry_index BETWEEN 0 AND 31),
    child_revision TEXT NOT NULL UNIQUE REFERENCES source_revisions(id),
    parent_revision TEXT NOT NULL REFERENCES source_revisions(id),
    document_sha256 TEXT NOT NULL CHECK(length(document_sha256) = 64),
    PRIMARY KEY(resolve_id, entry_index),
    FOREIGN KEY(resolve_id, entry_index) REFERENCES playlist_entries(resolve_id, entry_index)
) STRICT;
CREATE TRIGGER playlist_acceptance_no_update BEFORE UPDATE ON playlist_acceptances BEGIN
    SELECT RAISE(ABORT, 'playlist acceptance is immutable');
END;
CREATE TRIGGER playlist_acceptance_no_delete BEFORE DELETE ON playlist_acceptances BEGIN
    SELECT RAISE(ABORT, 'playlist acceptance is retained');
END;
PRAGMA user_version = 8;
