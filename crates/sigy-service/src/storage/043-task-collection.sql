-- One lifetime finite grant per task; cancellation does not refund capture reservations.
ALTER TABLE schedule_rules ADD COLUMN task_owned INTEGER NOT NULL DEFAULT 0 CHECK(task_owned IN (0, 1));
CREATE TABLE task_collections (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id),
    request_id TEXT NOT NULL CHECK(length(request_id) BETWEEN 1 AND 128),
    spec_json TEXT NOT NULL CHECK(json_valid(spec_json) AND length(CAST(spec_json AS BLOB)) BETWEEN 2 AND 2048),
    grant_sha256 TEXT NOT NULL CHECK(length(grant_sha256) = 64 AND grant_sha256 NOT GLOB '*[^0-9a-f]*'),
    scope_sha256 TEXT NOT NULL CHECK(length(scope_sha256) = 64),
    template TEXT NOT NULL CHECK(template = 'bounded-collection-v1'),
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;
CREATE TABLE task_collection_rules (
    task_id TEXT NOT NULL REFERENCES task_collections(task_id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 1),
    rule_id TEXT NOT NULL UNIQUE REFERENCES schedule_rules(id),
    rule_revision INTEGER NOT NULL CHECK(rule_revision = 0),
    generation INTEGER NOT NULL CHECK(generation = 1),
    PRIMARY KEY(task_id, ordinal)
) STRICT;
CREATE TABLE task_collection_cancellations (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES task_collections(task_id),
    request_id TEXT NOT NULL CHECK(length(request_id) BETWEEN 1 AND 128),
    expected_generation INTEGER NOT NULL CHECK(expected_generation = 1),
    generation INTEGER NOT NULL CHECK(generation = 2),
    admitted_mask INTEGER NOT NULL CHECK(admitted_mask BETWEEN 0 AND 3),
    receipt_sha256 TEXT NOT NULL CHECK(length(receipt_sha256) = 64),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;
CREATE TABLE task_collection_admissions (
    task_id TEXT NOT NULL REFERENCES task_collections(task_id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 1),
    occurrence_id TEXT NOT NULL UNIQUE REFERENCES monitor_capture_admissions(occurrence_id),
    generation INTEGER NOT NULL CHECK(generation = 1),
    admitted_ms INTEGER NOT NULL CHECK(admitted_ms >= 0),
    PRIMARY KEY(task_id, ordinal),
    FOREIGN KEY(task_id, ordinal) REFERENCES task_collection_rules(task_id, ordinal)
) STRICT;
CREATE TRIGGER task_collections_no_update BEFORE UPDATE ON task_collections BEGIN
    SELECT RAISE(ABORT, 'task collection grant is immutable');
END;
CREATE TRIGGER task_collections_no_delete BEFORE DELETE ON task_collections BEGIN
    SELECT RAISE(ABORT, 'task collection grant is retained');
END;
CREATE TRIGGER task_collection_rules_no_update BEFORE UPDATE ON task_collection_rules BEGIN
    SELECT RAISE(ABORT, 'task collection rule binding is immutable');
END;
CREATE TRIGGER task_collection_rules_no_delete BEFORE DELETE ON task_collection_rules BEGIN
    SELECT RAISE(ABORT, 'task collection rule binding is retained');
END;
CREATE TRIGGER task_collection_cancellations_no_update BEFORE UPDATE ON task_collection_cancellations BEGIN
    SELECT RAISE(ABORT, 'task collection cancellation is immutable');
END;
CREATE TRIGGER task_collection_cancellations_no_delete BEFORE DELETE ON task_collection_cancellations BEGIN
    SELECT RAISE(ABORT, 'task collection cancellation is retained');
END;
CREATE TRIGGER task_collection_admissions_no_update BEFORE UPDATE ON task_collection_admissions BEGIN
    SELECT RAISE(ABORT, 'task collection admission is immutable');
END;
CREATE TRIGGER task_collection_admissions_no_delete BEFORE DELETE ON task_collection_admissions BEGIN
    SELECT RAISE(ABORT, 'task collection admission is retained');
END;
CREATE TRIGGER task_collection_admissions_cancelled BEFORE INSERT ON task_collection_admissions
WHEN EXISTS(SELECT 1 FROM task_collection_cancellations WHERE task_id = NEW.task_id) BEGIN
    SELECT RAISE(ABORT, 'task collection is cancelled');
END;
CREATE TRIGGER task_collection_schedule_no_update BEFORE UPDATE ON schedule_rules
WHEN OLD.task_owned = 1 OR EXISTS(SELECT 1 FROM task_collection_rules WHERE rule_id = OLD.id) BEGIN
    SELECT RAISE(ABORT, 'task collection schedule is immutable');
END;
CREATE TRIGGER task_collection_schedule_no_delete BEFORE DELETE ON schedule_rules
WHEN OLD.task_owned = 1 OR EXISTS(SELECT 1 FROM task_collection_rules WHERE rule_id = OLD.id) BEGIN
    SELECT RAISE(ABORT, 'task collection schedule is retained');
END;
PRAGMA user_version = 43;
