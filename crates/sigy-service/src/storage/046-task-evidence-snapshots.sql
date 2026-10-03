-- Exact observations share the existing finite observation allowance with checkpoints.
CREATE TABLE task_evidence_snapshots (
    task_id TEXT NOT NULL REFERENCES tasks(id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 128),
    request_id TEXT NOT NULL CHECK(length(request_id) BETWEEN 1 AND 128),
    expected_snapshot INTEGER NOT NULL CHECK(expected_snapshot = ordinal - 1),
    observed_ms INTEGER NOT NULL CHECK(observed_ms >= 0),
    scope_sha256 TEXT NOT NULL CHECK(length(scope_sha256) = 64 AND scope_sha256 NOT GLOB '*[^0-9a-f]*'),
    collection_sha256 TEXT NOT NULL CHECK(length(collection_sha256) = 64),
    processing_sha256 TEXT CHECK(length(processing_sha256) = 64),
    template TEXT NOT NULL CHECK(template = 'collected-evidence-snapshot-v1'),
    mode TEXT NOT NULL CHECK(mode = 'freeze-now-v1'),
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json) AND length(CAST(payload_json AS BLOB)) BETWEEN 2 AND 65536),
    payload_sha256 TEXT NOT NULL CHECK(length(payload_sha256) = 64 AND payload_sha256 NOT GLOB '*[^0-9a-f]*'),
    PRIMARY KEY(task_id, ordinal),
    UNIQUE(task_id, request_id),
    CHECK(json_extract(payload_json, '$.task_id') IS task_id),
    CHECK(json_extract(payload_json, '$.ordinal') IS ordinal),
    CHECK(json_extract(payload_json, '$.expected_snapshot') IS expected_snapshot),
    CHECK(json_extract(payload_json, '$.request_id') IS request_id),
    CHECK(json_extract(payload_json, '$.observed_ms') IS observed_ms),
    CHECK(json_extract(payload_json, '$.scope_sha256') IS scope_sha256),
    CHECK(json_extract(payload_json, '$.collection.grant_sha256') IS collection_sha256),
    CHECK(json_extract(payload_json, '$.processing.grant_sha256') IS processing_sha256),
    CHECK(json_extract(payload_json, '$.template') IS template),
    CHECK(json_extract(payload_json, '$.mode') IS mode)
) STRICT;
CREATE TRIGGER task_evidence_snapshot_admission BEFORE INSERT ON task_evidence_snapshots
WHEN NEW.ordinal != coalesce((SELECT max(ordinal) + 1 FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 1)
 OR (SELECT count(*) FROM task_checkpoints WHERE task_id = NEW.task_id)
    + (SELECT count(*) FROM task_evidence_snapshots WHERE task_id = NEW.task_id) >= 128
 OR NEW.observed_ms < (SELECT created_ms FROM tasks WHERE id = NEW.task_id)
 OR NEW.observed_ms < coalesce((SELECT max(observed_ms) FROM task_checkpoints WHERE task_id = NEW.task_id), 0)
 OR NEW.observed_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
 OR NEW.observed_ms < (SELECT created_ms FROM task_collections WHERE task_id = NEW.task_id)
 OR NEW.observed_ms < coalesce((SELECT max(admitted_ms) FROM task_collection_admissions WHERE task_id = NEW.task_id), 0)
 OR NEW.observed_ms < coalesce((SELECT created_ms FROM task_processing WHERE task_id = NEW.task_id), 0)
 OR NEW.observed_ms < coalesce((SELECT max(created_ms) FROM task_processing_steps WHERE task_id = NEW.task_id), 0)
 OR NOT EXISTS (
    SELECT 1 FROM tasks t JOIN task_collections c ON c.task_id = t.id
    WHERE t.id = NEW.task_id AND t.scope_sha256 = NEW.scope_sha256
      AND c.grant_sha256 = NEW.collection_sha256
      AND t.monitor_version = (SELECT max(version) FROM monitor_versions WHERE monitor_id = t.monitor_id)
      AND t.monitor_actions = (SELECT count(*) FROM monitor_actions WHERE monitor_id = t.monitor_id)
 )
 OR (NEW.processing_sha256 IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM task_processing p WHERE p.task_id = NEW.task_id AND p.grant_sha256 = NEW.processing_sha256
 ))
BEGIN SELECT RAISE(ABORT, 'task evidence snapshot admission'); END;
CREATE TRIGGER task_evidence_snapshot_no_update BEFORE UPDATE ON task_evidence_snapshots
BEGIN SELECT RAISE(ABORT, 'task evidence snapshots are immutable'); END;
CREATE TRIGGER task_evidence_snapshot_no_delete BEFORE DELETE ON task_evidence_snapshots
BEGIN SELECT RAISE(ABORT, 'task evidence snapshots are retained'); END;
CREATE TRIGGER task_checkpoint_shared_capacity BEFORE INSERT ON task_checkpoints
WHEN (SELECT count(*) FROM task_checkpoints WHERE task_id = NEW.task_id)
   + (SELECT count(*) FROM task_evidence_snapshots WHERE task_id = NEW.task_id) >= 128
 OR NEW.observed_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task observation capacity or clock'); END;
CREATE TRIGGER task_capture_snapshot_clock BEFORE INSERT ON task_collection_admissions
WHEN NEW.admitted_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task snapshot clock'); END;
CREATE TRIGGER task_collection_cancel_snapshot_clock BEFORE INSERT ON task_collection_cancellations
WHEN NEW.created_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task snapshot clock'); END;
CREATE TRIGGER task_processing_snapshot_clock BEFORE INSERT ON task_processing
WHEN NEW.created_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task snapshot clock'); END;
CREATE TRIGGER task_processing_step_snapshot_clock BEFORE INSERT ON task_processing_steps
WHEN NEW.created_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task snapshot clock'); END;
CREATE TRIGGER task_processing_cancel_snapshot_clock BEFORE INSERT ON task_processing_cancellations
WHEN NEW.created_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task snapshot clock'); END;
CREATE TRIGGER task_withdrawal_snapshot_clock BEFORE INSERT ON task_interest_withdrawals
WHEN NEW.created_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task snapshot clock'); END;
CREATE TRIGGER task_run_snapshot_clock BEFORE INSERT ON task_runs
WHEN NEW.created_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task snapshot clock'); END;
CREATE TRIGGER task_run_event_snapshot_clock BEFORE INSERT ON task_run_events
WHEN NEW.recorded_ms < coalesce((SELECT max(observed_ms) FROM task_evidence_snapshots WHERE task_id = NEW.task_id), 0)
BEGIN SELECT RAISE(ABORT, 'task snapshot clock'); END;
PRAGMA user_version = 46;
