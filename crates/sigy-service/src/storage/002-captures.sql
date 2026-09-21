CREATE TABLE capture_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    source_revision TEXT NOT NULL CHECK(length(source_revision) BETWEEN 1 AND 128),
    starts_ms INTEGER NOT NULL CHECK(starts_ms >= 0),
    ends_ms INTEGER NOT NULL CHECK(ends_ms > starts_ms),
    maximum_bytes INTEGER NOT NULL CHECK(maximum_bytes > 0),
    state TEXT NOT NULL CHECK(state IN ('scheduled', 'starting', 'running', 'retrying', 'stopping', 'completed', 'interrupted', 'cancelled', 'failed')),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    generation INTEGER NOT NULL CHECK(generation >= 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    updated_ms INTEGER NOT NULL CHECK(updated_ms >= 0)
) STRICT;

CREATE TABLE capture_events (
    job_id TEXT NOT NULL REFERENCES capture_jobs(id),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    generation INTEGER NOT NULL CHECK(generation >= 0),
    previous_state TEXT,
    state TEXT NOT NULL,
    event TEXT CHECK(event IN ('start', 'connected', 'retry', 'stop', 'finalized', 'lost', 'cancel', 'fail')),
    reason TEXT NOT NULL CHECK(length(reason) BETWEEN 1 AND 128),
    recorded_ms INTEGER NOT NULL CHECK(recorded_ms >= 0),
    PRIMARY KEY(job_id, revision),
    CHECK((revision = 0 AND generation = 0 AND previous_state IS NULL AND event IS NULL AND state = 'scheduled')
        OR (revision > 0 AND previous_state IS NOT NULL AND event IS NOT NULL))
) STRICT;

CREATE TRIGGER capture_plan_immutable BEFORE UPDATE OF id, source_revision, starts_ms, ends_ms, maximum_bytes, created_ms ON capture_jobs BEGIN
    SELECT RAISE(ABORT, 'capture intent is immutable');
END;
CREATE TRIGGER capture_events_no_update BEFORE UPDATE ON capture_events BEGIN
    SELECT RAISE(ABORT, 'capture events are append-only');
END;
CREATE TRIGGER capture_events_no_delete BEFORE DELETE ON capture_events BEGIN
    SELECT RAISE(ABORT, 'capture events are append-only');
END;
CREATE INDEX captures_by_state ON capture_jobs(state, id);
PRAGMA user_version = 2;
