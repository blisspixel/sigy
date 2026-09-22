CREATE TABLE analysis_inputs (
    id TEXT NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 64),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    media_sha256 TEXT NOT NULL CHECK(length(media_sha256) = 64 AND media_sha256 NOT GLOB '*[^0-9a-f]*'),
    timeline_json TEXT NOT NULL CHECK(length(timeline_json) BETWEEN 2 AND 65536),
    state TEXT NOT NULL CHECK(state IN ('admitted', 'published', 'superseded')),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (id, revision)
) STRICT;
CREATE UNIQUE INDEX analysis_one_admitted ON analysis_inputs(id) WHERE state = 'admitted';
CREATE TRIGGER analysis_revision_immutable
BEFORE UPDATE OF id, revision, recording_id, media_sha256, timeline_json, created_ms ON analysis_inputs
BEGIN
    SELECT RAISE(ABORT, 'analysis revision is immutable');
END;
CREATE TRIGGER analysis_state_transition
BEFORE UPDATE OF state ON analysis_inputs
WHEN NOT (
    (OLD.state = 'admitted' AND NEW.state = 'published')
    OR (OLD.state = 'admitted' AND NEW.state = 'superseded')
)
BEGIN
    SELECT RAISE(ABORT, 'analysis revision transition is invalid');
END;
CREATE TRIGGER analysis_input_no_delete
BEFORE DELETE ON analysis_inputs
BEGIN
    SELECT RAISE(ABORT, 'analysis input is retained');
END;
PRAGMA user_version = 22;
