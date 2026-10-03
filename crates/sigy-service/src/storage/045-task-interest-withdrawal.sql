-- New explicit withdrawal leaves legacy admission-only cancellation untouched.
CREATE TABLE task_interest_withdrawals (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES task_processing(task_id),
    request_id TEXT NOT NULL CHECK(length(request_id) BETWEEN 1 AND 128),
    expected_processing_generation INTEGER NOT NULL CHECK(expected_processing_generation IN (1, 2)),
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json) AND length(CAST(payload_json AS BLOB)) BETWEEN 2 AND 16384),
    receipt_sha256 TEXT NOT NULL CHECK(length(receipt_sha256) = 64),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;
CREATE TABLE job_interest_withdrawals (
    family TEXT NOT NULL,
    job_id TEXT NOT NULL,
    authority TEXT NOT NULL CHECK(authority = 'task'),
    owner_id TEXT NOT NULL REFERENCES task_interest_withdrawals(task_id) DEFERRABLE INITIALLY DEFERRED,
    interest_created_ms INTEGER NOT NULL CHECK(interest_created_ms >= 0),
    PRIMARY KEY(family, job_id, authority, owner_id),
    FOREIGN KEY(family, job_id, authority, owner_id) REFERENCES job_interests(family, job_id, authority, owner_id)
) STRICT;
CREATE TABLE job_interest_legacy_guards (
    family TEXT NOT NULL CHECK(family IN ('recognition', 'translation')),
    job_id TEXT NOT NULL CHECK(length(job_id) BETWEEN 1 AND 128),
    PRIMARY KEY(family, job_id)
) STRICT;
INSERT INTO job_interest_legacy_guards SELECT DISTINCT family, job_id FROM job_interests WHERE origin = 'migrated';
CREATE TABLE native_stop_targets (
    family TEXT NOT NULL CHECK(family IN ('recognition', 'translation')),
    job_id TEXT NOT NULL CHECK(length(job_id) BETWEEN 1 AND 128),
    generation INTEGER NOT NULL CHECK(generation > 0),
    attempt INTEGER NOT NULL CHECK(attempt > 0),
    lease_owner TEXT NOT NULL CHECK(length(lease_owner) BETWEEN 1 AND 128),
    task_id TEXT NOT NULL REFERENCES task_interest_withdrawals(task_id) DEFERRABLE INITIALLY DEFERRED,
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY(family, job_id, generation)
) STRICT;
CREATE TABLE native_stop_completions (
    family TEXT NOT NULL,
    job_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    attempt INTEGER NOT NULL,
    lease_owner TEXT NOT NULL,
    observed_clock_ms INTEGER NOT NULL CHECK(observed_clock_ms >= 0),
    completed_ms INTEGER NOT NULL CHECK(completed_ms >= 0),
    PRIMARY KEY(family, job_id, generation),
    FOREIGN KEY(family, job_id, generation) REFERENCES native_stop_targets(family, job_id, generation)
) STRICT;
CREATE INDEX native_stop_targets_task ON native_stop_targets(task_id, family, job_id, generation);
CREATE INDEX job_interest_withdrawals_owner ON job_interest_withdrawals(owner_id, family, job_id);
CREATE TRIGGER task_withdrawal_no_update BEFORE UPDATE ON task_interest_withdrawals BEGIN SELECT RAISE(ABORT, 'withdrawal is immutable'); END;
CREATE TRIGGER task_withdrawal_no_delete BEFORE DELETE ON task_interest_withdrawals BEGIN SELECT RAISE(ABORT, 'withdrawal is retained'); END;
CREATE TRIGGER job_withdrawal_no_update BEFORE UPDATE ON job_interest_withdrawals BEGIN SELECT RAISE(ABORT, 'withdrawal is immutable'); END;
CREATE TRIGGER job_withdrawal_no_delete BEFORE DELETE ON job_interest_withdrawals BEGIN SELECT RAISE(ABORT, 'withdrawal is retained'); END;
CREATE TRIGGER legacy_guard_no_update BEFORE UPDATE ON job_interest_legacy_guards BEGIN SELECT RAISE(ABORT, 'legacy guard is immutable'); END;
CREATE TRIGGER legacy_guard_no_delete BEFORE DELETE ON job_interest_legacy_guards BEGIN SELECT RAISE(ABORT, 'legacy guard is retained'); END;
CREATE TRIGGER stop_target_no_update BEFORE UPDATE ON native_stop_targets BEGIN SELECT RAISE(ABORT, 'stop target is immutable'); END;
CREATE TRIGGER stop_target_no_delete BEFORE DELETE ON native_stop_targets BEGIN SELECT RAISE(ABORT, 'stop target is retained'); END;
CREATE TRIGGER stop_completion_no_update BEFORE UPDATE ON native_stop_completions BEGIN SELECT RAISE(ABORT, 'stop completion is immutable'); END;
CREATE TRIGGER stop_completion_no_delete BEFORE DELETE ON native_stop_completions BEGIN SELECT RAISE(ABORT, 'stop completion is retained'); END;
CREATE TRIGGER withdrawal_fences_processing BEFORE INSERT ON task_processing_steps
WHEN EXISTS(SELECT 1 FROM task_interest_withdrawals WHERE task_id = NEW.task_id)
BEGIN SELECT RAISE(ABORT, 'task interest withdrawn'); END;
CREATE TRIGGER withdrawal_interest_binding BEFORE INSERT ON job_interest_withdrawals
WHEN NEW.interest_created_ms IS NOT (SELECT created_ms FROM job_interests WHERE family = NEW.family AND job_id = NEW.job_id AND authority = 'task' AND owner_id = NEW.owner_id)
BEGIN SELECT RAISE(ABORT, 'withdrawal interest identity'); END;
CREATE TRIGGER stop_completion_binding BEFORE INSERT ON native_stop_completions
WHEN NOT EXISTS(SELECT 1 FROM native_stop_targets t WHERE t.family = NEW.family AND t.job_id = NEW.job_id AND t.generation = NEW.generation AND t.attempt = NEW.attempt AND t.lease_owner = NEW.lease_owner AND NEW.completed_ms = max(NEW.observed_clock_ms, t.created_ms, CASE t.family WHEN 'recognition' THEN (SELECT max(created_ms, coalesce(started_ms, created_ms)) FROM analysis_jobs WHERE id = t.job_id AND generation = t.generation AND attempt = t.attempt AND lease_owner = t.lease_owner) ELSE (SELECT max(created_ms, coalesce(started_ms, created_ms)) FROM translation_jobs WHERE id = t.job_id AND generation = t.generation AND attempt = t.attempt AND lease_owner = t.lease_owner) END))
BEGIN SELECT RAISE(ABORT, 'stop completion identity'); END;
PRAGMA user_version = 45;
