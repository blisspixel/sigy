-- Frozen coverage for one briefing generation. Later monitor versions do not rewrite it.
CREATE TABLE IF NOT EXISTS monitor_briefing_coverage (
    monitor_id TEXT NOT NULL,
    briefing_id TEXT NOT NULL,
    monitor_version INTEGER NOT NULL CHECK(monitor_version >= 1),
    daily_audio_seconds INTEGER NOT NULL CHECK(daily_audio_seconds >= 0),
    PRIMARY KEY (monitor_id, briefing_id),
    FOREIGN KEY (monitor_id, briefing_id) REFERENCES monitor_briefings(monitor_id, id)
) STRICT;

CREATE TABLE IF NOT EXISTS monitor_briefing_sources (
    monitor_id TEXT NOT NULL,
    briefing_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 31),
    source TEXT NOT NULL CHECK(length(source) BETWEEN 1 AND 128),
    captures INTEGER NOT NULL CHECK(captures >= 0),
    published INTEGER NOT NULL CHECK(published >= 0),
    recorded_us INTEGER NOT NULL CHECK(recorded_us >= 0),
    gaps INTEGER NOT NULL CHECK(gaps >= 0),
    gap_us INTEGER NOT NULL CHECK(gap_us >= 0),
    pinned INTEGER NOT NULL CHECK(pinned >= 0),
    transcribed INTEGER NOT NULL CHECK(transcribed >= 0),
    transcribed_us INTEGER NOT NULL CHECK(transcribed_us >= 0),
    no_text INTEGER NOT NULL CHECK(no_text >= 0),
    no_text_us INTEGER NOT NULL CHECK(no_text_us >= 0),
    translated_cues INTEGER NOT NULL CHECK(translated_cues >= 0),
    untranslated_cues INTEGER NOT NULL CHECK(untranslated_cues >= 0),
    cues_without_translation INTEGER NOT NULL CHECK(cues_without_translation >= 0),
    truncated INTEGER NOT NULL CHECK(truncated IN (0, 1)),
    PRIMARY KEY (monitor_id, briefing_id, ordinal),
    UNIQUE (monitor_id, briefing_id, source),
    FOREIGN KEY (monitor_id, briefing_id) REFERENCES monitor_briefing_coverage(monitor_id, briefing_id)
) STRICT;

CREATE TABLE IF NOT EXISTS monitor_briefing_reasons (
    monitor_id TEXT NOT NULL,
    briefing_id TEXT NOT NULL,
    source_ordinal INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 255),
    reason TEXT NOT NULL CHECK(length(reason) BETWEEN 1 AND 1024),
    count INTEGER NOT NULL CHECK(count >= 1),
    PRIMARY KEY (monitor_id, briefing_id, source_ordinal, ordinal),
    FOREIGN KEY (monitor_id, briefing_id, source_ordinal)
        REFERENCES monitor_briefing_sources(monitor_id, briefing_id, ordinal)
) STRICT;

CREATE TABLE IF NOT EXISTS monitor_briefing_schedules (
    monitor_id TEXT NOT NULL,
    briefing_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 31),
    schedule TEXT NOT NULL CHECK(length(schedule) BETWEEN 1 AND 128),
    admitted INTEGER NOT NULL CHECK(admitted >= 0),
    missed_elapsed INTEGER NOT NULL CHECK(missed_elapsed >= 0),
    missed_spring_forward INTEGER NOT NULL CHECK(missed_spring_forward >= 0),
    waiting INTEGER NOT NULL CHECK(waiting >= 0),
    PRIMARY KEY (monitor_id, briefing_id, ordinal),
    UNIQUE (monitor_id, briefing_id, schedule),
    FOREIGN KEY (monitor_id, briefing_id) REFERENCES monitor_briefing_coverage(monitor_id, briefing_id)
) STRICT;

DROP TRIGGER IF EXISTS monitor_briefing_coverage_no_update;
CREATE TRIGGER monitor_briefing_coverage_no_update
BEFORE UPDATE ON monitor_briefing_coverage
BEGIN
    SELECT RAISE(ABORT, 'briefings are immutable');
END;

DROP TRIGGER IF EXISTS monitor_briefing_coverage_no_delete;
CREATE TRIGGER monitor_briefing_coverage_no_delete
BEFORE DELETE ON monitor_briefing_coverage
BEGIN
    SELECT RAISE(ABORT, 'briefings are retained');
END;

DROP TRIGGER IF EXISTS monitor_briefing_source_no_update;
CREATE TRIGGER monitor_briefing_source_no_update
BEFORE UPDATE ON monitor_briefing_sources
BEGIN
    SELECT RAISE(ABORT, 'briefings are immutable');
END;

DROP TRIGGER IF EXISTS monitor_briefing_source_no_delete;
CREATE TRIGGER monitor_briefing_source_no_delete
BEFORE DELETE ON monitor_briefing_sources
BEGIN
    SELECT RAISE(ABORT, 'briefings are retained');
END;

DROP TRIGGER IF EXISTS monitor_briefing_reason_no_update;
CREATE TRIGGER monitor_briefing_reason_no_update
BEFORE UPDATE ON monitor_briefing_reasons
BEGIN
    SELECT RAISE(ABORT, 'briefings are immutable');
END;

DROP TRIGGER IF EXISTS monitor_briefing_reason_no_delete;
CREATE TRIGGER monitor_briefing_reason_no_delete
BEFORE DELETE ON monitor_briefing_reasons
BEGIN
    SELECT RAISE(ABORT, 'briefings are retained');
END;

DROP TRIGGER IF EXISTS monitor_briefing_schedule_no_update;
CREATE TRIGGER monitor_briefing_schedule_no_update
BEFORE UPDATE ON monitor_briefing_schedules
BEGIN
    SELECT RAISE(ABORT, 'briefings are immutable');
END;

DROP TRIGGER IF EXISTS monitor_briefing_schedule_no_delete;
CREATE TRIGGER monitor_briefing_schedule_no_delete
BEFORE DELETE ON monitor_briefing_schedules
BEGIN
    SELECT RAISE(ABORT, 'briefings are retained');
END;

PRAGMA user_version = 39;
