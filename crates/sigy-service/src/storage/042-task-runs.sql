-- Explicit immutable delegation and ordered effect intents for one frozen checkpoint.
CREATE TABLE task_runs (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id),
    request_id TEXT NOT NULL CHECK(length(request_id) BETWEEN 1 AND 128),
    spec_json TEXT NOT NULL CHECK(json_valid(spec_json) AND length(CAST(spec_json AS BLOB)) BETWEEN 2 AND 1024),
    grant_sha256 TEXT NOT NULL CHECK(length(grant_sha256) = 64 AND grant_sha256 NOT GLOB '*[^0-9a-f]*'),
    scope_sha256 TEXT NOT NULL CHECK(length(scope_sha256) = 64),
    checkpoint_ordinal INTEGER NOT NULL CHECK(checkpoint_ordinal BETWEEN 1 AND 128),
    checkpoint_sha256 TEXT NOT NULL CHECK(length(checkpoint_sha256) = 64),
    maximum_findings INTEGER NOT NULL CHECK(maximum_findings BETWEEN 1 AND 64),
    planned_findings INTEGER NOT NULL CHECK(planned_findings BETWEEN 0 AND maximum_findings),
    initial_partial INTEGER NOT NULL CHECK(initial_partial IN (0, 1)),
    template TEXT NOT NULL CHECK(template = 'literal-briefing-v1'),
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    FOREIGN KEY (task_id, checkpoint_ordinal) REFERENCES task_checkpoints(task_id, ordinal),
    CHECK(json_extract(spec_json, '$.checkpoint_ordinal') IS checkpoint_ordinal),
    CHECK(json_extract(spec_json, '$.maximum_findings') IS maximum_findings)
) STRICT;
CREATE TABLE task_run_intents (
    task_id TEXT NOT NULL REFERENCES task_runs(task_id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 65),
    kind TEXT NOT NULL CHECK(kind IN ('finding', 'briefing')),
    effect_id TEXT NOT NULL CHECK(length(effect_id) BETWEEN 1 AND 128),
    citation_ordinal INTEGER CHECK(citation_ordinal BETWEEN 0 AND 63),
    PRIMARY KEY (task_id, ordinal),
    UNIQUE (task_id, effect_id),
    CHECK((kind = 'finding' AND citation_ordinal IS ordinal - 1) OR (kind = 'briefing' AND citation_ordinal IS NULL))
) STRICT;
CREATE TABLE task_run_events (
    task_id TEXT NOT NULL REFERENCES task_runs(task_id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 66),
    generation INTEGER NOT NULL CHECK(generation IS ordinal + 1),
    state TEXT NOT NULL CHECK(state IN ('running', 'completed', 'partial', 'cancelled', 'revoked')),
    kind TEXT NOT NULL CHECK(kind IN ('finding', 'briefing', 'cancelled', 'revoked')),
    effect_id TEXT NOT NULL CHECK(length(effect_id) BETWEEN 1 AND 128),
    citation_ordinal INTEGER CHECK(citation_ordinal BETWEEN 0 AND 63),
    finding_id TEXT CHECK(length(finding_id) BETWEEN 1 AND 128),
    reason TEXT CHECK(length(reason) BETWEEN 1 AND 128),
    request_id TEXT CHECK(length(request_id) BETWEEN 1 AND 128),
    expected_generation INTEGER CHECK(expected_generation BETWEEN 1 AND 66),
    recorded_ms INTEGER NOT NULL CHECK(recorded_ms >= 0),
    PRIMARY KEY (task_id, ordinal),
    UNIQUE (task_id, generation),
    UNIQUE (task_id, request_id),
    CHECK((kind = 'finding' AND state = 'running' AND citation_ordinal IS ordinal - 1 AND ((finding_id IS effect_id AND (reason IS NULL OR reason IN ('original-expired', 'original-missing'))) OR (finding_id IS NULL AND reason IS NOT NULL)))
       OR (kind = 'briefing' AND state IN ('completed', 'partial') AND citation_ordinal IS NULL AND finding_id IS NULL)
       OR (kind IN ('cancelled', 'revoked') AND state IS kind AND citation_ordinal IS NULL AND finding_id IS NULL AND reason IS NOT NULL)),
    CHECK((kind = 'cancelled' AND request_id IS NOT NULL AND expected_generation IS ordinal) OR (kind != 'cancelled' AND request_id IS NULL AND expected_generation IS NULL))
) STRICT;
CREATE TRIGGER task_run_admission BEFORE INSERT ON task_runs
WHEN NOT EXISTS (
    SELECT 1 FROM tasks t JOIN task_checkpoints c ON c.task_id = t.id AND c.ordinal = NEW.checkpoint_ordinal
    WHERE t.id = NEW.task_id AND t.scope_sha256 = NEW.scope_sha256 AND c.payload_sha256 = NEW.checkpoint_sha256
      AND t.monitor_version = (SELECT max(version) FROM monitor_versions WHERE monitor_id = t.monitor_id)
      AND t.monitor_actions = (SELECT count(*) FROM monitor_actions WHERE monitor_id = t.monitor_id)
      AND NEW.created_ms >= c.observed_ms AND json_extract(c.payload_json, '$.monitor_paused') IS 0
)
BEGIN SELECT RAISE(ABORT, 'task run scope'); END;
CREATE TRIGGER task_run_intent_admission BEFORE INSERT ON task_run_intents
WHEN NEW.ordinal != coalesce((SELECT max(ordinal) + 1 FROM task_run_intents WHERE task_id = NEW.task_id), 1)
 OR EXISTS (SELECT 1 FROM task_run_events WHERE task_id = NEW.task_id)
 OR NOT EXISTS (SELECT 1 FROM task_runs r WHERE r.task_id = NEW.task_id AND
    ((NEW.kind = 'finding' AND NEW.ordinal <= r.planned_findings) OR (NEW.kind = 'briefing' AND NEW.ordinal = r.planned_findings + 1)))
BEGIN SELECT RAISE(ABORT, 'task run intent'); END;
CREATE TRIGGER task_run_event_admission BEFORE INSERT ON task_run_events
WHEN NEW.ordinal != coalesce((SELECT max(ordinal) + 1 FROM task_run_events WHERE task_id = NEW.task_id), 1)
 OR NEW.recorded_ms < (SELECT created_ms FROM task_runs WHERE task_id = NEW.task_id)
 OR NEW.recorded_ms < coalesce((SELECT max(recorded_ms) FROM task_run_events WHERE task_id = NEW.task_id), 0)
 OR EXISTS (SELECT 1 FROM task_run_events WHERE task_id = NEW.task_id AND state != 'running')
 OR (SELECT count(*) FROM task_run_intents WHERE task_id = NEW.task_id) != (SELECT planned_findings + 1 FROM task_runs WHERE task_id = NEW.task_id)
 OR (NEW.kind IN ('finding', 'briefing') AND NOT EXISTS (SELECT 1 FROM task_run_intents i WHERE i.task_id = NEW.task_id AND i.ordinal = NEW.ordinal AND i.kind = NEW.kind AND i.effect_id = NEW.effect_id AND i.citation_ordinal IS NEW.citation_ordinal))
 OR (NEW.kind IN ('finding', 'briefing') AND EXISTS (SELECT 1 FROM tasks t WHERE t.id = NEW.task_id AND
    (t.monitor_version != (SELECT max(version) FROM monitor_versions WHERE monitor_id = t.monitor_id)
     OR t.monitor_actions != (SELECT count(*) FROM monitor_actions WHERE monitor_id = t.monitor_id))))
BEGIN SELECT RAISE(ABORT, 'task run event'); END;
CREATE TRIGGER task_run_no_update BEFORE UPDATE ON task_runs BEGIN SELECT RAISE(ABORT, 'task run grants are immutable'); END;
CREATE TRIGGER task_run_no_delete BEFORE DELETE ON task_runs BEGIN SELECT RAISE(ABORT, 'task run grants are retained'); END;
CREATE TRIGGER task_run_intent_no_update BEFORE UPDATE ON task_run_intents BEGIN SELECT RAISE(ABORT, 'task run intents are immutable'); END;
CREATE TRIGGER task_run_intent_no_delete BEFORE DELETE ON task_run_intents BEGIN SELECT RAISE(ABORT, 'task run intents are retained'); END;
CREATE TRIGGER task_run_event_no_update BEFORE UPDATE ON task_run_events BEGIN SELECT RAISE(ABORT, 'task run events are immutable'); END;
CREATE TRIGGER task_run_event_no_delete BEFORE DELETE ON task_run_events BEGIN SELECT RAISE(ABORT, 'task run events are retained'); END;
PRAGMA user_version = 42;
