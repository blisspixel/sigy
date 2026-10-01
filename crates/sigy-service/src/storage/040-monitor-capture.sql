-- Ownership is opt-in for a new rule. Existing schedules retain their authority.
CREATE TABLE monitor_capture_rules (
    rule_id TEXT PRIMARY KEY NOT NULL REFERENCES schedule_rules(id),
    monitor_id TEXT NOT NULL,
    created_version INTEGER NOT NULL CHECK(created_version > 0),
    FOREIGN KEY(monitor_id, created_version) REFERENCES monitor_versions(monitor_id, version)
) STRICT;

CREATE TABLE monitor_capture_admissions (
    occurrence_id TEXT PRIMARY KEY NOT NULL REFERENCES schedule_occurrences(id),
    monitor_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal > 0),
    policy_version INTEGER NOT NULL CHECK(policy_version > 0),
    policy_sha256 TEXT NOT NULL CHECK(length(policy_sha256) = 64),
    rule_revision INTEGER NOT NULL CHECK(rule_revision >= 0),
    source_revision TEXT NOT NULL REFERENCES source_revisions(id),
    start_ms INTEGER NOT NULL CHECK(start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK(end_ms > start_ms),
    planned_seconds INTEGER NOT NULL CHECK(planned_seconds BETWEEN 1 AND 900),
    maximum_bytes INTEGER NOT NULL CHECK(maximum_bytes BETWEEN 1 AND 268435456),
    daily_cap INTEGER NOT NULL CHECK(daily_cap BETWEEN 1 AND 86400),
    total_cap INTEGER NOT NULL CHECK(total_cap BETWEEN 1 AND 31622400),
    byte_cap INTEGER NOT NULL CHECK(byte_cap > 0),
    admitted_ms INTEGER NOT NULL CHECK(admitted_ms >= start_ms AND admitted_ms < end_ms),
    action_ordinal INTEGER NOT NULL CHECK(action_ordinal BETWEEN 0 AND 100000),
    amount_micros INTEGER NOT NULL DEFAULT 0 CHECK(amount_micros = 0),
    CHECK(end_ms - start_ms = planned_seconds * 1000),
    FOREIGN KEY(monitor_id, policy_version) REFERENCES monitor_versions(monitor_id, version),
    UNIQUE(monitor_id, ordinal)
) STRICT;

CREATE TABLE monitor_capture_days (
    occurrence_id TEXT NOT NULL REFERENCES monitor_capture_admissions(occurrence_id),
    utc_day INTEGER NOT NULL CHECK(utc_day >= 0),
    seconds INTEGER NOT NULL CHECK(seconds BETWEEN 1 AND 900),
    PRIMARY KEY(occurrence_id, utc_day)
) STRICT;

CREATE TABLE monitor_capture_refusals (
    -- Waiting occurrences may be replaced by a user schedule revision. Keep the
    -- refused plan here instead of permitting that revision to delete history.
    occurrence_id TEXT NOT NULL,
    rule_id TEXT NOT NULL REFERENCES schedule_rules(id),
    rule_revision INTEGER NOT NULL CHECK(rule_revision >= 0),
    source_revision TEXT NOT NULL REFERENCES source_revisions(id),
    start_ms INTEGER NOT NULL CHECK(start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK(end_ms > start_ms),
    planned_seconds INTEGER NOT NULL CHECK(planned_seconds BETWEEN 1 AND 900),
    maximum_bytes INTEGER NOT NULL CHECK(maximum_bytes BETWEEN 1 AND 268435456),
    monitor_id TEXT NOT NULL,
    policy_version INTEGER NOT NULL CHECK(policy_version > 0),
    reason TEXT NOT NULL CHECK(reason IN ('capture-disabled','source-not-followed','daily-cap','total-cap','byte-cap')),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    amount_micros INTEGER NOT NULL DEFAULT 0 CHECK(amount_micros = 0),
    PRIMARY KEY(occurrence_id, rule_revision, policy_version, reason),
    FOREIGN KEY(monitor_id, policy_version) REFERENCES monitor_versions(monitor_id, version)
) STRICT;

CREATE TRIGGER monitor_capture_admission_policy
BEFORE INSERT ON monitor_capture_admissions
BEGIN
    SELECT RAISE(ABORT, 'monitor capture admission order') WHERE NEW.ordinal != COALESCE((SELECT MAX(ordinal) + 1 FROM monitor_capture_admissions WHERE monitor_id = NEW.monitor_id), 1);
    SELECT RAISE(ABORT, 'monitor capture admission provenance') WHERE NOT EXISTS (
        SELECT 1 FROM schedule_occurrences o
        JOIN schedule_rules r ON r.id = o.rule_id
        JOIN monitor_capture_rules owner ON owner.rule_id = r.id
        JOIN monitor_versions v ON v.monitor_id = owner.monitor_id AND v.version = NEW.policy_version
        WHERE o.id = NEW.occurrence_id AND o.state = 'admitted' AND o.recording_id = o.id
          AND owner.monitor_id = NEW.monitor_id AND o.rule_revision = NEW.rule_revision
          AND NEW.policy_version >= owner.created_version AND o.rule_revision = r.revision
          AND o.duration_seconds = r.duration_seconds AND o.maximum_bytes = r.maximum_bytes
          AND r.source_revision = NEW.source_revision AND o.start_ms = NEW.start_ms
          AND o.end_ms = NEW.end_ms AND o.duration_seconds = NEW.planned_seconds
          AND o.maximum_bytes = NEW.maximum_bytes AND v.spec_sha256 = NEW.policy_sha256
          AND NEW.policy_version = (SELECT MAX(version) FROM monitor_versions WHERE monitor_id = NEW.monitor_id)
          AND json_extract(v.spec_json, '$.capture.daily_seconds') = NEW.daily_cap
          AND json_extract(v.spec_json, '$.capture.total_seconds') = NEW.total_cap
          AND json_extract(v.spec_json, '$.capture.total_bytes') = NEW.byte_cap
          AND (EXISTS(SELECT 1 FROM json_each(v.spec_json, '$.sources') WHERE value = NEW.source_revision) OR EXISTS(SELECT 1 FROM json_each(v.spec_json, '$.candidate_sources') WHERE value = NEW.source_revision))
          AND NEW.action_ordinal = COALESCE((SELECT MAX(ordinal) FROM monitor_actions WHERE monitor_id = NEW.monitor_id), 0)
          AND COALESCE((SELECT kind = 'add_source' FROM monitor_actions WHERE monitor_id = NEW.monitor_id AND policy_version = NEW.policy_version AND decision = 'applied' AND kind IN ('add_source', 'remove_source') AND json_extract(proposal_json, '$.source') = NEW.source_revision ORDER BY ordinal DESC LIMIT 1), EXISTS(SELECT 1 FROM json_each(v.spec_json, '$.sources') WHERE value = NEW.source_revision))
    );
    SELECT RAISE(ABORT, 'monitor capture lifetime cap')
    WHERE COALESCE((SELECT SUM(planned_seconds) FROM monitor_capture_admissions WHERE monitor_id = NEW.monitor_id), 0) > NEW.total_cap - NEW.planned_seconds
       OR COALESCE((SELECT SUM(maximum_bytes) FROM monitor_capture_admissions WHERE monitor_id = NEW.monitor_id), 0) > NEW.byte_cap - NEW.maximum_bytes;
END;

CREATE TRIGGER monitor_capture_daily_cap
BEFORE INSERT ON monitor_capture_days
BEGIN
    SELECT RAISE(ABORT, 'monitor capture daily cap') WHERE EXISTS (
        SELECT 1 FROM monitor_capture_admissions a WHERE a.occurrence_id = NEW.occurrence_id
        AND COALESCE((SELECT SUM(d.seconds) FROM monitor_capture_days d JOIN monitor_capture_admissions prior ON prior.occurrence_id = d.occurrence_id WHERE prior.monitor_id = a.monitor_id AND d.utc_day = NEW.utc_day), 0) > a.daily_cap - NEW.seconds
    );
END;

CREATE TRIGGER monitor_capture_rules_immutable BEFORE UPDATE ON monitor_capture_rules BEGIN SELECT RAISE(ABORT, 'capture owner is immutable'); END;
CREATE TRIGGER monitor_capture_rules_retained BEFORE DELETE ON monitor_capture_rules BEGIN SELECT RAISE(ABORT, 'capture owner is retained'); END;
CREATE TRIGGER monitor_capture_admissions_immutable BEFORE UPDATE ON monitor_capture_admissions BEGIN SELECT RAISE(ABORT, 'capture admission is immutable'); END;
CREATE TRIGGER monitor_capture_admissions_retained BEFORE DELETE ON monitor_capture_admissions BEGIN SELECT RAISE(ABORT, 'capture admission is retained'); END;
CREATE TRIGGER monitor_capture_days_immutable BEFORE UPDATE ON monitor_capture_days BEGIN SELECT RAISE(ABORT, 'capture daily reservation is immutable'); END;
CREATE TRIGGER monitor_capture_days_retained BEFORE DELETE ON monitor_capture_days BEGIN SELECT RAISE(ABORT, 'capture daily reservation is retained'); END;
CREATE TRIGGER monitor_capture_refusals_immutable BEFORE UPDATE ON monitor_capture_refusals BEGIN SELECT RAISE(ABORT, 'capture refusal is immutable'); END;
CREATE TRIGGER monitor_capture_refusals_retained BEFORE DELETE ON monitor_capture_refusals BEGIN SELECT RAISE(ABORT, 'capture refusal is retained'); END;

PRAGMA user_version = 40;
