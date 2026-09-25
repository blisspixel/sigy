-- A durable local job pool: queued work, leases, attempts and one active job per
-- transcript lineage. The analysis_jobs and translation_jobs definitions are rebuilt in
-- Rust from their stored text before this file runs, because SQLite cannot alter a
-- CHECK constraint. Per-kind concurrency caps are enforced by the service scheduler.
DROP INDEX analysis_one_worker;
DROP INDEX translation_one_worker;
CREATE UNIQUE INDEX analysis_one_per_lineage ON analysis_jobs(lineage) WHERE state IN ('running', 'cancelling');
CREATE UNIQUE INDEX translation_one_per_lineage ON translation_jobs(lineage) WHERE state IN ('running', 'cancelling');
CREATE INDEX analysis_job_queue ON analysis_jobs(kind, state, created_ms);
CREATE INDEX translation_job_queue ON translation_jobs(state, created_ms);

-- One immutable row for every attempt a restart ended. Paid work never enters these
-- tables: provider attempts keep their own reservations and are never redelivered.
CREATE TABLE job_attempts (
    family TEXT NOT NULL CHECK(family IN ('analysis', 'translation')),
    job_id TEXT NOT NULL CHECK(length(job_id) BETWEEN 1 AND 128),
    attempt INTEGER NOT NULL CHECK(attempt BETWEEN 1 AND 8),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 64),
    lease_owner TEXT NOT NULL CHECK(length(lease_owner) BETWEEN 1 AND 128),
    started_ms INTEGER NOT NULL CHECK(started_ms >= 0),
    ended_ms INTEGER NOT NULL CHECK(ended_ms >= started_ms),
    outcome TEXT NOT NULL CHECK(outcome = 'interrupted'),
    reason TEXT NOT NULL CHECK(reason = 'service-restarted'),
    PRIMARY KEY (family, job_id, attempt)
) STRICT;
CREATE TRIGGER job_attempt_recorded BEFORE INSERT ON job_attempts
WHEN NOT (
    (NEW.family = 'analysis' AND EXISTS (
        SELECT 1 FROM analysis_jobs j WHERE j.id = NEW.job_id AND j.state IN ('running', 'cancelling')
          AND j.attempt = NEW.attempt AND j.generation = NEW.generation
          AND j.lease_owner = NEW.lease_owner AND j.started_ms = NEW.started_ms))
    OR (NEW.family = 'translation' AND EXISTS (
        SELECT 1 FROM translation_jobs j WHERE j.id = NEW.job_id AND j.state IN ('running', 'cancelling')
          AND j.attempt = NEW.attempt AND j.generation = NEW.generation
          AND j.lease_owner = NEW.lease_owner AND j.started_ms = NEW.started_ms))
)
BEGIN SELECT RAISE(ABORT, 'job attempt does not match an active lease'); END;
CREATE TRIGGER job_attempt_no_update BEFORE UPDATE ON job_attempts
BEGIN SELECT RAISE(ABORT, 'job attempts are immutable'); END;
CREATE TRIGGER job_attempt_no_delete BEFORE DELETE ON job_attempts
BEGIN SELECT RAISE(ABORT, 'job attempts are retained'); END;

DROP TRIGGER analysis_job_input;
CREATE TRIGGER analysis_job_input
BEFORE INSERT ON analysis_jobs
WHEN NEW.state != 'queued' OR NEW.generation != 1 OR NEW.attempt != 1
    OR NEW.lease_owner IS NOT NULL OR NEW.lease_expires_ms IS NOT NULL OR NEW.started_ms IS NOT NULL
    OR NOT EXISTS (
    SELECT 1 FROM analysis_inputs a JOIN recordings r ON r.id = a.recording_id
    WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
      AND a.recording_id = NEW.recording_id AND a.state = 'published'
      AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id)
      AND r.storage_state = 'retained'
      AND r.sha256 = a.media_sha256 AND r.media_bytes = NEW.expected_bytes
)
BEGIN SELECT RAISE(ABORT, 'invalid analysis job input'); END;
DROP TRIGGER analysis_job_id_retained;
CREATE TRIGGER analysis_job_id_retained BEFORE INSERT ON analysis_jobs
WHEN EXISTS (SELECT 1 FROM analysis_jobs WHERE id = NEW.id)
BEGIN SELECT RAISE(ABORT, 'analysis job history is retained'); END;
CREATE TRIGGER analysis_job_queue_bound BEFORE INSERT ON analysis_jobs
WHEN (SELECT count(*) FROM analysis_jobs WHERE state IN ('queued', 'running', 'cancelling')) >= 1024
BEGIN SELECT RAISE(ABORT, 'analysis queue is full'); END;
DROP TRIGGER analysis_job_request_immutable;
CREATE TRIGGER analysis_job_request_immutable
BEFORE UPDATE OF id, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms, lineage ON analysis_jobs
BEGIN SELECT RAISE(ABORT, 'analysis request is immutable'); END;
DROP TRIGGER analysis_job_transition;
CREATE TRIGGER analysis_job_transition
BEFORE UPDATE OF state, generation, reason, finished_ms, verified_bytes, attempt, lease_owner, lease_expires_ms, started_ms ON analysis_jobs
WHEN NOT (
    (OLD.state = 'queued' AND NEW.state = 'running' AND NEW.generation = OLD.generation AND NEW.attempt = OLD.attempt
        AND NEW.lease_owner IS NOT NULL AND NEW.lease_expires_ms IS NOT NULL AND NEW.started_ms IS NOT NULL)
    OR (OLD.state = 'queued' AND NEW.state IN ('cancelled', 'failed') AND NEW.generation = OLD.generation
        AND NEW.attempt = OLD.attempt AND NEW.lease_owner IS NULL AND NEW.lease_expires_ms IS NULL AND NEW.started_ms IS NULL)
    OR (NEW.attempt = OLD.attempt AND NEW.lease_owner IS OLD.lease_owner
        AND NEW.lease_expires_ms IS OLD.lease_expires_ms AND NEW.started_ms IS OLD.started_ms AND (
        (OLD.state = 'running' AND NEW.state = 'cancelling' AND NEW.generation = OLD.generation)
        OR (OLD.state = 'running' AND NEW.state IN ('verified', 'succeeded', 'failed', 'cancelled') AND NEW.generation = OLD.generation)
        OR (OLD.state = 'cancelling' AND NEW.state = 'cancelled' AND NEW.generation = OLD.generation)
        OR (OLD.state IN ('running', 'cancelling') AND NEW.state = 'interrupted' AND NEW.generation = OLD.generation + 1)))
    OR (OLD.state = 'running' AND NEW.state = 'queued' AND OLD.amount_micros = 0
        AND NEW.generation = OLD.generation + 1 AND NEW.attempt = OLD.attempt + 1
        AND NEW.lease_owner IS NULL AND NEW.lease_expires_ms IS NULL AND NEW.started_ms IS NULL
        AND EXISTS (SELECT 1 FROM job_attempts a WHERE a.family = 'analysis' AND a.job_id = OLD.id
            AND a.attempt = OLD.attempt AND a.generation = OLD.generation))
)
BEGIN SELECT RAISE(ABORT, 'invalid analysis job transition'); END;

DROP TRIGGER translation_job_limit;
CREATE TRIGGER translation_job_admission BEFORE INSERT ON translation_jobs
WHEN NEW.state != 'queued' OR NEW.generation != 1 OR NEW.attempt != 1
    OR NEW.lease_owner IS NOT NULL OR NEW.lease_expires_ms IS NOT NULL OR NEW.started_ms IS NOT NULL
    OR EXISTS (SELECT 1 FROM translation_jobs WHERE id = NEW.id)
    OR (SELECT count(*) FROM translation_jobs WHERE state IN ('queued', 'running', 'cancelling')) >= 1024
BEGIN SELECT RAISE(ABORT, 'translation job admission'); END;
CREATE TRIGGER translation_job_request_immutable
BEFORE UPDATE OF id, transcript_id, transcript_revision, profile, profile_sha256, created_ms, lineage ON translation_jobs
BEGIN SELECT RAISE(ABORT, 'translation request is immutable'); END;
CREATE TRIGGER translation_job_transition
BEFORE UPDATE OF state, generation, reason, finished_ms, attempt, lease_owner, lease_expires_ms, started_ms ON translation_jobs
WHEN NOT (
    (OLD.state = 'queued' AND NEW.state = 'running' AND NEW.generation = OLD.generation AND NEW.attempt = OLD.attempt
        AND NEW.lease_owner IS NOT NULL AND NEW.lease_expires_ms IS NOT NULL AND NEW.started_ms IS NOT NULL)
    OR (OLD.state = 'queued' AND NEW.state IN ('cancelled', 'failed') AND NEW.generation = OLD.generation
        AND NEW.attempt = OLD.attempt AND NEW.lease_owner IS NULL AND NEW.lease_expires_ms IS NULL AND NEW.started_ms IS NULL)
    OR (NEW.attempt = OLD.attempt AND NEW.lease_owner IS OLD.lease_owner
        AND NEW.lease_expires_ms IS OLD.lease_expires_ms AND NEW.started_ms IS OLD.started_ms AND (
        (OLD.state = 'running' AND NEW.state = 'cancelling' AND NEW.generation = OLD.generation)
        OR (OLD.state = 'running' AND NEW.state IN ('succeeded', 'failed', 'cancelled') AND NEW.generation = OLD.generation)
        OR (OLD.state = 'cancelling' AND NEW.state = 'cancelled' AND NEW.generation = OLD.generation)
        OR (OLD.state IN ('running', 'cancelling') AND NEW.state = 'interrupted' AND NEW.generation = OLD.generation + 1)))
    OR (OLD.state = 'running' AND NEW.state = 'queued'
        AND NEW.generation = OLD.generation + 1 AND NEW.attempt = OLD.attempt + 1
        AND NEW.lease_owner IS NULL AND NEW.lease_expires_ms IS NULL AND NEW.started_ms IS NULL
        AND EXISTS (SELECT 1 FROM job_attempts a WHERE a.family = 'translation' AND a.job_id = OLD.id
            AND a.attempt = OLD.attempt AND a.generation = OLD.generation))
)
BEGIN SELECT RAISE(ABORT, 'invalid translation job transition'); END;
PRAGMA user_version = 31;
