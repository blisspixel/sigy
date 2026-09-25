-- Automatic monitor processing: one append-only row per monitor, recording and stage.
-- A queued recognition step charges the recording's decoded audio against the monitor's
-- caps on the UTC day it was queued. The caps are checked here as well as in Rust.
CREATE TABLE monitor_steps (
    monitor_id TEXT NOT NULL REFERENCES monitors(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    stage TEXT NOT NULL CHECK(stage IN ('recognition', 'translation')),
    policy_version INTEGER NOT NULL,
    decision TEXT NOT NULL CHECK(decision IN ('queued', 'skipped')),
    reason TEXT CHECK(reason IS NULL OR length(reason) BETWEEN 1 AND 64),
    analysis_id TEXT CHECK(analysis_id IS NULL OR length(analysis_id) BETWEEN 1 AND 128),
    job_id TEXT CHECK(job_id IS NULL OR length(job_id) BETWEEN 1 AND 128),
    audio_us INTEGER NOT NULL CHECK(audio_us >= 0),
    charged_day INTEGER NOT NULL CHECK(charged_day >= 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (monitor_id, recording_id, stage),
    FOREIGN KEY (monitor_id, policy_version) REFERENCES monitor_versions(monitor_id, version),
    CHECK((decision = 'queued' AND reason IS NULL AND job_id IS NOT NULL AND analysis_id IS NOT NULL)
       OR (decision = 'skipped' AND reason IS NOT NULL AND job_id IS NULL AND audio_us = 0)),
    CHECK(stage = 'recognition' OR audio_us = 0)
) STRICT;
CREATE INDEX monitor_steps_usage ON monitor_steps(monitor_id, stage, charged_day);
CREATE TRIGGER monitor_step_policy BEFORE INSERT ON monitor_steps
WHEN NEW.policy_version != (SELECT max(version) FROM monitor_versions WHERE monitor_id = NEW.monitor_id)
BEGIN SELECT RAISE(ABORT, 'monitor step policy version'); END;
CREATE TRIGGER monitor_step_daily_cap BEFORE INSERT ON monitor_steps
WHEN NEW.stage = 'recognition' AND NEW.decision = 'queued' AND
    (SELECT coalesce(sum(audio_us), 0) FROM monitor_steps
      WHERE monitor_id = NEW.monitor_id AND stage = 'recognition' AND charged_day = NEW.charged_day)
    + NEW.audio_us >
    (SELECT json_extract(spec_json, '$.daily_audio_seconds') * 1000000 FROM monitor_versions
      WHERE monitor_id = NEW.monitor_id AND version = NEW.policy_version)
BEGIN SELECT RAISE(ABORT, 'monitor daily audio cap'); END;
CREATE TRIGGER monitor_step_total_cap BEFORE INSERT ON monitor_steps
WHEN NEW.stage = 'recognition' AND NEW.decision = 'queued' AND
    (SELECT coalesce(sum(audio_us), 0) FROM monitor_steps
      WHERE monitor_id = NEW.monitor_id AND stage = 'recognition')
    + NEW.audio_us >
    (SELECT json_extract(spec_json, '$.total_audio_seconds') * 1000000 FROM monitor_versions
      WHERE monitor_id = NEW.monitor_id AND version = NEW.policy_version)
BEGIN SELECT RAISE(ABORT, 'monitor total audio cap'); END;
CREATE TRIGGER monitor_step_no_update BEFORE UPDATE ON monitor_steps BEGIN SELECT RAISE(ABORT, 'monitor steps are immutable'); END;
CREATE TRIGGER monitor_step_no_delete BEFORE DELETE ON monitor_steps BEGIN SELECT RAISE(ABORT, 'monitor steps are retained'); END;
PRAGMA user_version = 33;
