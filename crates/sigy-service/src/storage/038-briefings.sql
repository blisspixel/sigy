-- One frozen briefing generation. Coverage is read again when the generation is shown.
-- Classification stays off. A model proposal cannot insert a row.
CREATE TABLE IF NOT EXISTS monitor_briefings (
    monitor_id TEXT NOT NULL REFERENCES monitors(id),
    id TEXT NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 1024),
    from_ms INTEGER NOT NULL CHECK(from_ms >= 0),
    to_ms INTEGER NOT NULL,
    monitor_version INTEGER NOT NULL CHECK(monitor_version >= 1),
    classification TEXT NOT NULL CHECK(classification = 'off'),
    corroboration INTEGER NOT NULL CHECK(corroboration BETWEEN 0 AND 1024),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (monitor_id, id),
    UNIQUE (monitor_id, generation),
    CHECK(to_ms > from_ms AND to_ms - from_ms <= 2678400000)
) STRICT;

CREATE TABLE IF NOT EXISTS monitor_briefing_members (
    monitor_id TEXT NOT NULL,
    briefing_id TEXT NOT NULL,
    finding_id TEXT NOT NULL,
    group_ordinal INTEGER NOT NULL CHECK(group_ordinal BETWEEN 0 AND 1023),
    PRIMARY KEY (monitor_id, briefing_id, finding_id),
    FOREIGN KEY (monitor_id, briefing_id) REFERENCES monitor_briefings(monitor_id, id),
    FOREIGN KEY (monitor_id, finding_id) REFERENCES monitor_findings(monitor_id, id)
) STRICT;

DROP TRIGGER IF EXISTS monitor_briefing_limit;
CREATE TRIGGER monitor_briefing_limit
BEFORE INSERT ON monitor_briefings
WHEN (SELECT count(*) FROM monitor_briefings WHERE monitor_id = NEW.monitor_id) >= 1024
BEGIN
    SELECT RAISE(ABORT, 'briefing limit');
END;

DROP TRIGGER IF EXISTS monitor_briefing_generation;
CREATE TRIGGER monitor_briefing_generation
BEFORE INSERT ON monitor_briefings
WHEN NEW.generation <> COALESCE(
    (SELECT max(generation) FROM monitor_briefings WHERE monitor_id = NEW.monitor_id),
    0
) + 1
BEGIN
    SELECT RAISE(ABORT, 'briefing generation');
END;

DROP TRIGGER IF EXISTS monitor_briefing_no_update;
CREATE TRIGGER monitor_briefing_no_update
BEFORE UPDATE ON monitor_briefings
BEGIN
    SELECT RAISE(ABORT, 'briefings are immutable');
END;

DROP TRIGGER IF EXISTS monitor_briefing_no_delete;
CREATE TRIGGER monitor_briefing_no_delete
BEFORE DELETE ON monitor_briefings
BEGIN
    SELECT RAISE(ABORT, 'briefings are retained');
END;

DROP TRIGGER IF EXISTS monitor_briefing_member_no_update;
CREATE TRIGGER monitor_briefing_member_no_update
BEFORE UPDATE ON monitor_briefing_members
BEGIN
    SELECT RAISE(ABORT, 'briefings are immutable');
END;

DROP TRIGGER IF EXISTS monitor_briefing_member_no_delete;
CREATE TRIGGER monitor_briefing_member_no_delete
BEFORE DELETE ON monitor_briefing_members
BEGIN
    SELECT RAISE(ABORT, 'briefings are retained');
END;

PRAGMA user_version = 38;
