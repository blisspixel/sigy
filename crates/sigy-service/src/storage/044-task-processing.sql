-- Durable interests distinguish direct, monitor and task authority over one canonical job.
-- A finite task processing grant charges only its own lifetime audio allowance. Cancellation
-- fences future task admissions; admitted jobs and independent interests keep their authority.
CREATE TABLE job_interests (
    family TEXT NOT NULL CHECK(family IN ('recognition', 'translation')),
    job_id TEXT NOT NULL CHECK(length(job_id) BETWEEN 1 AND 128),
    authority TEXT NOT NULL CHECK(authority IN ('direct', 'monitor', 'task')),
    owner_id TEXT NOT NULL CHECK(length(owner_id) BETWEEN 0 AND 128),
    origin TEXT NOT NULL CHECK(origin IN ('admitted', 'migrated')),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY(family, job_id, authority, owner_id),
    CHECK((authority = 'direct') = (owner_id = '')),
    CHECK(authority != 'task' OR origin = 'admitted')
) STRICT;
CREATE INDEX job_interests_owner ON job_interests(authority, owner_id);
CREATE TABLE task_processing (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES task_collections(task_id),
    request_id TEXT NOT NULL CHECK(length(request_id) BETWEEN 1 AND 128),
    spec_json TEXT NOT NULL CHECK(json_valid(spec_json) AND length(CAST(spec_json AS BLOB)) BETWEEN 2 AND 1024),
    recognition_profile TEXT NOT NULL REFERENCES recognition_profiles(id),
    recognition_profile_sha256 TEXT NOT NULL CHECK(length(recognition_profile_sha256) = 64 AND recognition_profile_sha256 NOT GLOB '*[^0-9a-f]*'),
    translation_profile TEXT REFERENCES translation_profiles(id),
    translation_profile_sha256 TEXT CHECK(translation_profile_sha256 IS NULL OR (length(translation_profile_sha256) = 64 AND translation_profile_sha256 NOT GLOB '*[^0-9a-f]*')),
    maximum_audio_us INTEGER NOT NULL CHECK(maximum_audio_us BETWEEN 1000000 AND 1800000000 AND maximum_audio_us % 1000000 = 0),
    collection_sha256 TEXT NOT NULL CHECK(length(collection_sha256) = 64),
    scope_sha256 TEXT NOT NULL CHECK(length(scope_sha256) = 64),
    grant_sha256 TEXT NOT NULL CHECK(length(grant_sha256) = 64 AND grant_sha256 NOT GLOB '*[^0-9a-f]*'),
    template TEXT NOT NULL CHECK(template = 'collected-processing-v1'),
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    CHECK((translation_profile IS NULL) = (translation_profile_sha256 IS NULL))
) STRICT;
CREATE TABLE task_processing_steps (
    task_id TEXT NOT NULL REFERENCES task_processing(task_id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 1),
    stage TEXT NOT NULL CHECK(stage IN ('recognition', 'translation')),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    decision TEXT NOT NULL CHECK(decision IN ('queued', 'skipped')),
    reason TEXT CHECK(reason IS NULL OR length(reason) BETWEEN 1 AND 64),
    input_id TEXT CHECK(input_id IS NULL OR length(input_id) BETWEEN 1 AND 128),
    input_revision INTEGER CHECK(input_revision IS NULL OR input_revision >= 1),
    job_id TEXT CHECK(job_id IS NULL OR length(job_id) BETWEEN 1 AND 128),
    audio_us INTEGER NOT NULL CHECK(audio_us >= 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY(task_id, ordinal, stage),
    FOREIGN KEY(task_id, ordinal) REFERENCES task_collection_admissions(task_id, ordinal),
    CHECK((decision = 'queued' AND reason IS NULL AND input_id IS NOT NULL AND input_revision IS NOT NULL AND job_id IS NOT NULL)
       OR (decision = 'skipped' AND reason IS NOT NULL AND input_id IS NULL AND input_revision IS NULL AND job_id IS NULL AND audio_us = 0)),
    CHECK(stage = 'recognition' OR audio_us = 0),
    CHECK(decision = 'skipped' OR stage = 'translation' OR audio_us > 0)
) STRICT;
CREATE INDEX task_processing_step_jobs ON task_processing_steps(job_id);
CREATE TABLE task_processing_cancellations (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES task_processing(task_id),
    request_id TEXT NOT NULL CHECK(length(request_id) BETWEEN 1 AND 128),
    expected_generation INTEGER NOT NULL CHECK(expected_generation = 1),
    generation INTEGER NOT NULL CHECK(generation = 2),
    step_mask INTEGER NOT NULL CHECK(step_mask BETWEEN 0 AND 15),
    receipt_sha256 TEXT NOT NULL CHECK(length(receipt_sha256) = 64 AND receipt_sha256 NOT GLOB '*[^0-9a-f]*'),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;
CREATE TRIGGER task_processing_no_update BEFORE UPDATE ON task_processing BEGIN
    SELECT RAISE(ABORT, 'task processing grant is immutable');
END;
CREATE TRIGGER task_processing_no_delete BEFORE DELETE ON task_processing BEGIN
    SELECT RAISE(ABORT, 'task processing grant is retained');
END;
CREATE TRIGGER task_processing_steps_no_update BEFORE UPDATE ON task_processing_steps BEGIN
    SELECT RAISE(ABORT, 'task processing step is immutable');
END;
CREATE TRIGGER task_processing_steps_no_delete BEFORE DELETE ON task_processing_steps BEGIN
    SELECT RAISE(ABORT, 'task processing step is retained');
END;
CREATE TRIGGER task_processing_cancellations_no_update BEFORE UPDATE ON task_processing_cancellations BEGIN
    SELECT RAISE(ABORT, 'task processing cancellation is immutable');
END;
CREATE TRIGGER task_processing_cancellations_no_delete BEFORE DELETE ON task_processing_cancellations BEGIN
    SELECT RAISE(ABORT, 'task processing cancellation is retained');
END;
CREATE TRIGGER job_interests_no_update BEFORE UPDATE ON job_interests BEGIN
    SELECT RAISE(ABORT, 'job interest is immutable');
END;
CREATE TRIGGER job_interests_no_delete BEFORE DELETE ON job_interests BEGIN
    SELECT RAISE(ABORT, 'job interest is retained');
END;
-- Exact collected recording identity, the cancellation fence and stage order.
CREATE TRIGGER task_processing_step_binding BEFORE INSERT ON task_processing_steps
WHEN NEW.recording_id IS NOT (SELECT occurrence_id FROM task_collection_admissions WHERE task_id = NEW.task_id AND ordinal = NEW.ordinal)
  OR EXISTS (SELECT 1 FROM task_processing_cancellations WHERE task_id = NEW.task_id)
  OR (NEW.stage = 'translation' AND NOT EXISTS (
      SELECT 1 FROM task_processing_steps r WHERE r.task_id = NEW.task_id AND r.ordinal = NEW.ordinal AND r.stage = 'recognition'))
BEGIN SELECT RAISE(ABORT, 'task processing step binding'); END;
-- Recognition binds the granted profile revision and the stored decoded audio charge.
CREATE TRIGGER task_processing_recognition_binding BEFORE INSERT ON task_processing_steps
WHEN NEW.stage = 'recognition' AND NEW.decision = 'queued' AND NOT EXISTS (
    SELECT 1 FROM analysis_jobs j JOIN task_processing p ON p.task_id = NEW.task_id
      JOIN recordings r ON r.id = NEW.recording_id
    WHERE j.id = NEW.job_id AND j.kind = 'local_asr' AND j.recording_id = NEW.recording_id
      AND j.analysis_id = NEW.input_id AND j.analysis_revision = NEW.input_revision
      AND j.profile = p.recognition_profile AND j.profile_sha256 = p.recognition_profile_sha256
      AND r.decoded_microseconds = NEW.audio_us)
BEGIN SELECT RAISE(ABORT, 'task recognition binding'); END;
-- Translation binds the transcript revision published by this task's recognition job.
CREATE TRIGGER task_processing_translation_binding BEFORE INSERT ON task_processing_steps
WHEN NEW.stage = 'translation' AND NEW.decision = 'queued' AND NOT EXISTS (
    SELECT 1 FROM translation_jobs j JOIN task_processing p ON p.task_id = NEW.task_id
      JOIN transcripts t ON t.id = j.transcript_id AND t.revision = j.transcript_revision
      JOIN task_processing_steps s ON s.task_id = NEW.task_id AND s.ordinal = NEW.ordinal
        AND s.stage = 'recognition' AND s.decision = 'queued' AND t.job_id = s.job_id
    WHERE j.id = NEW.job_id AND j.transcript_id = NEW.input_id AND j.transcript_revision = NEW.input_revision
      AND j.profile = p.translation_profile AND j.profile_sha256 = p.translation_profile_sha256
      AND t.recording_id = NEW.recording_id AND t.kind = 'recognition' AND t.outcome = 'text')
BEGIN SELECT RAISE(ABORT, 'task translation binding'); END;
-- The lifetime allowance never refills: steps are immutable and retained.
CREATE TRIGGER task_processing_allowance BEFORE INSERT ON task_processing_steps
WHEN NEW.stage = 'recognition' AND NEW.decision = 'queued' AND
    (SELECT coalesce(sum(audio_us), 0) FROM task_processing_steps WHERE task_id = NEW.task_id AND stage = 'recognition')
    + NEW.audio_us > (SELECT maximum_audio_us FROM task_processing WHERE task_id = NEW.task_id)
BEGIN SELECT RAISE(ABORT, 'task processing audio allowance'); END;
CREATE TRIGGER task_processing_cancellation_order BEFORE INSERT ON task_processing_cancellations
WHEN NEW.step_mask != (SELECT coalesce(sum(1 << (ordinal * 2 + (stage = 'translation'))), 0) FROM task_processing_steps WHERE task_id = NEW.task_id)
BEGIN SELECT RAISE(ABORT, 'task processing cancellation receipt'); END;
CREATE TRIGGER job_interest_job BEFORE INSERT ON job_interests
WHEN (NEW.family = 'recognition' AND NOT EXISTS (SELECT 1 FROM analysis_jobs WHERE id = NEW.job_id AND kind = 'local_asr'))
  OR (NEW.family = 'translation' AND NOT EXISTS (SELECT 1 FROM translation_jobs WHERE id = NEW.job_id))
BEGIN SELECT RAISE(ABORT, 'job interest requires its canonical job'); END;
CREATE TRIGGER job_interest_receipt BEFORE INSERT ON job_interests
WHEN (NEW.authority = 'monitor' AND NOT EXISTS (
        SELECT 1 FROM monitor_steps WHERE monitor_id = NEW.owner_id AND stage = NEW.family AND decision = 'queued' AND job_id = NEW.job_id))
  OR (NEW.authority = 'task' AND NOT EXISTS (
        SELECT 1 FROM task_processing_steps WHERE task_id = NEW.owner_id AND stage = NEW.family AND decision = 'queued' AND job_id = NEW.job_id))
BEGIN SELECT RAISE(ABORT, 'job interest requires its authority receipt'); END;
-- Earlier catalogs stored monitor receipts but no direct receipt. Derive interests once and
-- label them; a job that no queued monitor step references was admitted directly.
INSERT INTO job_interests(family, job_id, authority, owner_id, origin, created_ms)
SELECT s.stage, s.job_id, 'monitor', s.monitor_id, 'migrated', s.created_ms FROM monitor_steps s
WHERE s.decision = 'queued';
INSERT INTO job_interests(family, job_id, authority, owner_id, origin, created_ms)
SELECT 'recognition', j.id, 'direct', '', 'migrated', j.created_ms FROM analysis_jobs j
WHERE j.kind = 'local_asr' AND NOT EXISTS (
    SELECT 1 FROM monitor_steps s WHERE s.stage = 'recognition' AND s.decision = 'queued' AND s.job_id = j.id);
INSERT INTO job_interests(family, job_id, authority, owner_id, origin, created_ms)
SELECT 'translation', j.id, 'direct', '', 'migrated', j.created_ms FROM translation_jobs j
WHERE NOT EXISTS (
    SELECT 1 FROM monitor_steps s WHERE s.stage = 'translation' AND s.decision = 'queued' AND s.job_id = j.id);
PRAGMA user_version = 44;
