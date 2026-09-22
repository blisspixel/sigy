CREATE TABLE analysis_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 64),
    analysis_id TEXT NOT NULL,
    analysis_revision INTEGER NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    profile TEXT NOT NULL CHECK(profile = 'retained-sha256-v1'),
    state TEXT NOT NULL CHECK(state IN ('running', 'cancelling', 'verified', 'cancelled', 'failed', 'interrupted')),
    expected_bytes INTEGER NOT NULL CHECK(expected_bytes BETWEEN 1 AND 536870912),
    expected_files INTEGER NOT NULL CHECK(expected_files BETWEEN 1 AND 1024),
    manifest_sha256 TEXT NOT NULL CHECK(length(manifest_sha256) = 64 AND manifest_sha256 NOT GLOB '*[^0-9a-f]*'),
    verified_bytes INTEGER CHECK(verified_bytes = expected_bytes),
    reason TEXT CHECK(length(reason) BETWEEN 1 AND 128),
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    finished_ms INTEGER CHECK(finished_ms >= created_ms),
    FOREIGN KEY(analysis_id, analysis_revision) REFERENCES analysis_inputs(id, revision),
    CHECK((state IN ('running', 'cancelling')) = (finished_ms IS NULL)),
    CHECK((state = 'verified') = (verified_bytes IS NOT NULL)),
    CHECK((state IN ('cancelled', 'failed', 'interrupted')) = (reason IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX analysis_one_worker ON analysis_jobs((1)) WHERE state IN ('running', 'cancelling');
CREATE INDEX analysis_job_lease ON analysis_jobs(recording_id, state);
CREATE TRIGGER analysis_job_input
BEFORE INSERT ON analysis_jobs
WHEN NEW.state != 'running' OR NEW.generation != 1 OR NOT EXISTS (
    SELECT 1 FROM analysis_inputs a JOIN recordings r ON r.id = a.recording_id
    WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
      AND a.recording_id = NEW.recording_id AND a.state = 'published'
      AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id)
      AND r.storage_state = 'retained'
      AND r.sha256 = a.media_sha256 AND r.media_bytes = NEW.expected_bytes
)
BEGIN SELECT RAISE(ABORT, 'invalid analysis job input'); END;
CREATE TRIGGER analysis_job_request_immutable
BEFORE UPDATE OF id, analysis_id, analysis_revision, recording_id, profile, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms ON analysis_jobs
BEGIN SELECT RAISE(ABORT, 'analysis request is immutable'); END;
CREATE TRIGGER analysis_job_transition
BEFORE UPDATE OF state, generation, reason, finished_ms, verified_bytes ON analysis_jobs
WHEN NOT (
    (OLD.state = 'running' AND NEW.state = 'cancelling' AND NEW.generation = OLD.generation)
    OR (OLD.state = 'running' AND NEW.state IN ('verified', 'failed', 'cancelled') AND NEW.generation = OLD.generation)
    OR (OLD.state = 'cancelling' AND NEW.state = 'cancelled' AND NEW.generation = OLD.generation)
    OR (OLD.state IN ('running', 'cancelling') AND NEW.state = 'interrupted' AND NEW.generation = OLD.generation + 1)
)
BEGIN SELECT RAISE(ABORT, 'invalid analysis job transition'); END;
CREATE TRIGGER analysis_job_no_delete
BEFORE DELETE ON analysis_jobs
BEGIN SELECT RAISE(ABORT, 'analysis job history is retained'); END;
CREATE TRIGGER analysis_read_lease_delete
BEFORE UPDATE OF storage_state ON recordings
WHEN NEW.storage_state IN ('deleting', 'deleted') AND EXISTS (
    SELECT 1 FROM analysis_jobs j WHERE j.recording_id = OLD.id AND j.state IN ('running', 'cancelling')
)
BEGIN SELECT RAISE(ABORT, 'recording has an active analysis read lease'); END;
CREATE TRIGGER analysis_read_lease_release
BEFORE INSERT ON recording_releases
WHEN EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.recording_id = NEW.recording_id AND j.state IN ('running', 'cancelling'))
BEGIN SELECT RAISE(ABORT, 'recording has an active analysis read lease'); END;
PRAGMA user_version = 25;
