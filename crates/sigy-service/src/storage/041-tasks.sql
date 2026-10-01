-- Accepted zero-cost task scopes and service-observed immutable checkpoints.
CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    spec_json TEXT NOT NULL CHECK(length(CAST(spec_json AS BLOB)) BETWEEN 2 AND 8192 AND json_valid(spec_json)),
    scope_sha256 TEXT NOT NULL CHECK(length(scope_sha256) = 64 AND scope_sha256 NOT GLOB '*[^0-9a-f]*'),
    monitor_id TEXT NOT NULL,
    monitor_version INTEGER NOT NULL CHECK(monitor_version BETWEEN 1 AND 1000),
    monitor_actions INTEGER NOT NULL CHECK(monitor_actions BETWEEN 0 AND 100000),
    monitor_spec_sha256 TEXT NOT NULL CHECK(length(monitor_spec_sha256) = 64 AND monitor_spec_sha256 NOT GLOB '*[^0-9a-f]*'),
    from_ms INTEGER NOT NULL CHECK(from_ms >= 0),
    to_ms INTEGER NOT NULL CHECK(to_ms > from_ms AND to_ms - from_ms <= 2678400000),
    template TEXT NOT NULL CHECK(template = 'monitor-observation-v1'),
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    FOREIGN KEY (monitor_id, monitor_version) REFERENCES monitor_versions(monitor_id, version),
    CHECK(json_extract(spec_json, '$.monitor_id') IS monitor_id),
    CHECK(json_extract(spec_json, '$.monitor_version') IS monitor_version),
    CHECK(json_extract(spec_json, '$.monitor_actions') IS monitor_actions),
    CHECK(json_extract(spec_json, '$.from_ms') IS from_ms),
    CHECK(json_extract(spec_json, '$.to_ms') IS to_ms)
) STRICT;
CREATE TABLE task_checkpoints (
    task_id TEXT NOT NULL REFERENCES tasks(id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 128),
    request_id TEXT NOT NULL CHECK(length(request_id) BETWEEN 1 AND 128),
    expected_checkpoint INTEGER NOT NULL CHECK(expected_checkpoint = ordinal - 1),
    observed_ms INTEGER NOT NULL CHECK(observed_ms >= 0),
    payload_json TEXT NOT NULL CHECK(length(CAST(payload_json AS BLOB)) BETWEEN 2 AND 65536 AND json_valid(payload_json)),
    payload_sha256 TEXT NOT NULL CHECK(length(payload_sha256) = 64 AND payload_sha256 NOT GLOB '*[^0-9a-f]*'),
    PRIMARY KEY (task_id, ordinal),
    UNIQUE (task_id, request_id),
    CHECK(json_extract(payload_json, '$.task_id') IS task_id),
    CHECK(json_extract(payload_json, '$.ordinal') IS ordinal),
    CHECK(json_extract(payload_json, '$.request_id') IS request_id),
    CHECK(json_extract(payload_json, '$.observed_ms') IS observed_ms)
) STRICT;
CREATE TRIGGER task_admission BEFORE INSERT ON tasks
WHEN (SELECT count(*) FROM tasks) >= 256
 OR NEW.monitor_version != (SELECT max(version) FROM monitor_versions WHERE monitor_id = NEW.monitor_id)
 OR NEW.monitor_actions != (SELECT count(*) FROM monitor_actions WHERE monitor_id = NEW.monitor_id)
 OR NEW.monitor_spec_sha256 != (SELECT spec_sha256 FROM monitor_versions WHERE monitor_id = NEW.monitor_id AND version = NEW.monitor_version)
 OR NEW.created_ms < (SELECT created_ms FROM monitor_versions WHERE monitor_id = NEW.monitor_id AND version = NEW.monitor_version)
 OR NEW.created_ms < coalesce((SELECT max(created_ms) FROM monitor_actions WHERE monitor_id = NEW.monitor_id), 0)
BEGIN SELECT RAISE(ABORT, 'task scope admission'); END;
CREATE TRIGGER task_checkpoint_admission BEFORE INSERT ON task_checkpoints
WHEN NEW.ordinal != coalesce((SELECT max(ordinal) + 1 FROM task_checkpoints WHERE task_id = NEW.task_id), 1)
 OR NEW.observed_ms < (SELECT created_ms FROM tasks WHERE id = NEW.task_id)
 OR NEW.observed_ms < coalesce((SELECT max(observed_ms) FROM task_checkpoints WHERE task_id = NEW.task_id), 0)
 OR EXISTS (SELECT 1 FROM tasks t WHERE t.id = NEW.task_id AND
     (t.monitor_version != (SELECT max(version) FROM monitor_versions WHERE monitor_id = t.monitor_id)
      OR t.monitor_actions != (SELECT count(*) FROM monitor_actions WHERE monitor_id = t.monitor_id)))
BEGIN SELECT RAISE(ABORT, 'task checkpoint admission'); END;
CREATE TRIGGER task_no_update BEFORE UPDATE ON tasks BEGIN SELECT RAISE(ABORT, 'tasks are immutable'); END;
CREATE TRIGGER task_no_delete BEFORE DELETE ON tasks BEGIN SELECT RAISE(ABORT, 'tasks are retained'); END;
CREATE TRIGGER task_checkpoint_no_update BEFORE UPDATE ON task_checkpoints BEGIN SELECT RAISE(ABORT, 'task checkpoints are immutable'); END;
CREATE TRIGGER task_checkpoint_no_delete BEFORE DELETE ON task_checkpoints BEGIN SELECT RAISE(ABORT, 'task checkpoints are retained'); END;
PRAGMA user_version = 41;
