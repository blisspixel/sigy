-- A listen receipt is one explicit playback request. It is not a recording and stores no stream URL.
CREATE TABLE listen_sessions (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    source_revision TEXT NOT NULL REFERENCES source_revisions(id),
    state TEXT NOT NULL CHECK(state IN ('running', 'completed', 'failed', 'interrupted')),
    started_ms INTEGER NOT NULL CHECK(started_ms >= 0),
    completed_ms INTEGER CHECK(completed_ms IS NULL OR completed_ms >= started_ms),
    format TEXT CHECK(format IS NULL OR format IN ('mp3', 'aac', 'flac', 'ogg', 'wav')),
    failure TEXT CHECK(failure IS NULL OR length(failure) <= 256)
) STRICT;
CREATE UNIQUE INDEX one_running_listen_per_revision ON listen_sessions(source_revision) WHERE state = 'running';
CREATE TRIGGER listen_session_immutable BEFORE UPDATE OF id, source_revision, started_ms ON listen_sessions BEGIN
    SELECT RAISE(ABORT, 'listen session is immutable');
END;
PRAGMA user_version = 10;
