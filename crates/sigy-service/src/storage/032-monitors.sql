-- Topic monitors: user-authored immutable versions and an append-only action log.
CREATE TABLE monitors (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;
CREATE TABLE monitor_versions (
    monitor_id TEXT NOT NULL REFERENCES monitors(id),
    version INTEGER NOT NULL CHECK(version BETWEEN 1 AND 1000),
    spec_json TEXT NOT NULL CHECK(length(CAST(spec_json AS BLOB)) BETWEEN 2 AND 65536 AND json_valid(spec_json)),
    spec_sha256 TEXT NOT NULL CHECK(length(spec_sha256) = 64 AND spec_sha256 NOT GLOB '*[^0-9a-f]*'),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (monitor_id, version)
) STRICT;
CREATE TABLE monitor_actions (
    monitor_id TEXT NOT NULL REFERENCES monitors(id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 100000),
    action_id TEXT NOT NULL CHECK(length(action_id) BETWEEN 1 AND 128),
    policy_version INTEGER NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('user', 'schedule', 'rule', 'model')),
    kind TEXT NOT NULL CHECK(kind IN ('pause', 'resume', 'add_source', 'remove_source', 'other')),
    proposal_json TEXT NOT NULL CHECK(length(CAST(proposal_json AS BLOB)) BETWEEN 2 AND 8192 AND json_valid(proposal_json)),
    decision TEXT NOT NULL CHECK(decision IN ('applied', 'refused')),
    reason TEXT NOT NULL CHECK(length(reason) BETWEEN 1 AND 64),
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (monitor_id, ordinal),
    UNIQUE (monitor_id, action_id),
    FOREIGN KEY (monitor_id, policy_version) REFERENCES monitor_versions(monitor_id, version),
    CHECK(kind != 'other' OR decision = 'refused')
) STRICT;
CREATE TRIGGER monitor_limit BEFORE INSERT ON monitors
WHEN (SELECT count(*) FROM monitors) >= 256
BEGIN SELECT RAISE(ABORT, 'monitor limit'); END;
CREATE TRIGGER monitor_version_next BEFORE INSERT ON monitor_versions
WHEN NEW.version != coalesce((SELECT max(version) + 1 FROM monitor_versions WHERE monitor_id = NEW.monitor_id), 1)
BEGIN SELECT RAISE(ABORT, 'monitor version order'); END;
CREATE TRIGGER monitor_action_next BEFORE INSERT ON monitor_actions
WHEN NEW.ordinal != coalesce((SELECT max(ordinal) + 1 FROM monitor_actions WHERE monitor_id = NEW.monitor_id), 1)
  OR NEW.policy_version != (SELECT max(version) FROM monitor_versions WHERE monitor_id = NEW.monitor_id)
BEGIN SELECT RAISE(ABORT, 'monitor action order'); END;
CREATE TRIGGER monitor_no_update BEFORE UPDATE ON monitors BEGIN SELECT RAISE(ABORT, 'monitors are immutable'); END;
CREATE TRIGGER monitor_no_delete BEFORE DELETE ON monitors BEGIN SELECT RAISE(ABORT, 'monitors are retained'); END;
CREATE TRIGGER monitor_version_no_update BEFORE UPDATE ON monitor_versions BEGIN SELECT RAISE(ABORT, 'monitor versions are immutable'); END;
CREATE TRIGGER monitor_version_no_delete BEFORE DELETE ON monitor_versions BEGIN SELECT RAISE(ABORT, 'monitor versions are retained'); END;
CREATE TRIGGER monitor_action_no_update BEFORE UPDATE ON monitor_actions BEGIN SELECT RAISE(ABORT, 'monitor actions are immutable'); END;
CREATE TRIGGER monitor_action_no_delete BEFORE DELETE ON monitor_actions BEGIN SELECT RAISE(ABORT, 'monitor actions are retained'); END;
PRAGMA user_version = 32;
