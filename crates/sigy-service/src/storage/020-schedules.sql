-- A schedule names one source and one civil recurrence. Occurrences keep the
-- plan they were admitted with. A late start is a prefix gap, not a second job.
CREATE TABLE recording_gaps_v20 (
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0 AND ordinal < 1024),
    cause TEXT NOT NULL CHECK(cause IN (
        'disconnect',
        'recovery',
        'codec_change',
        'refused_renewal',
        'capture_pause',
        'backward_clock',
        'late_start'
    )),
    start_us INTEGER NOT NULL CHECK(start_us >= 0),
    end_us INTEGER NOT NULL CHECK(end_us > start_us),
    PRIMARY KEY (recording_id, ordinal)
) STRICT;
INSERT INTO recording_gaps_v20(recording_id, ordinal, cause, start_us, end_us)
SELECT recording_id, ordinal, cause, start_us, end_us FROM recording_gaps;
DROP TABLE recording_gaps;
ALTER TABLE recording_gaps_v20 RENAME TO recording_gaps;

CREATE TRIGGER recording_gaps_ordered
BEFORE INSERT ON recording_gaps
BEGIN
    SELECT RAISE(ABORT, 'gap is out of order')
    WHERE NEW.ordinal != COALESCE((
            SELECT MAX(ordinal) + 1 FROM recording_gaps WHERE recording_id = NEW.recording_id
        ), 0);
END;

CREATE TRIGGER recording_gaps_immutable
BEFORE UPDATE ON recording_gaps
BEGIN
    SELECT RAISE(ABORT, 'recorded gap is immutable');
END;

CREATE TRIGGER recording_gaps_no_delete
BEFORE DELETE ON recording_gaps
BEGIN
    SELECT RAISE(ABORT, 'recorded gap is retained');
END;

CREATE TABLE schedule_rules (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 80),
    source_revision TEXT NOT NULL REFERENCES source_revisions(id),
    zone TEXT NOT NULL CHECK(length(zone) BETWEEN 1 AND 64),
    recurrence TEXT NOT NULL CHECK(recurrence IN ('once', 'daily', 'weekly')),
    civil_date TEXT CHECK(civil_date IS NULL OR length(civil_date) = 10),
    weekday INTEGER CHECK(weekday IS NULL OR (weekday BETWEEN 1 AND 7)),
    hour INTEGER NOT NULL CHECK(hour BETWEEN 0 AND 23),
    minute INTEGER NOT NULL CHECK(minute BETWEEN 0 AND 59),
    second INTEGER NOT NULL CHECK(second BETWEEN 0 AND 59),
    duration_seconds INTEGER NOT NULL CHECK(duration_seconds BETWEEN 1 AND 900),
    maximum_bytes INTEGER NOT NULL CHECK(maximum_bytes BETWEEN 1 AND 268435456),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    updated_ms INTEGER NOT NULL CHECK(updated_ms >= 0),
    CHECK(
        (recurrence = 'once' AND civil_date IS NOT NULL AND weekday IS NULL)
        OR (recurrence = 'daily' AND civil_date IS NULL AND weekday IS NULL)
        OR (recurrence = 'weekly' AND civil_date IS NULL AND weekday IS NOT NULL)
    )
) STRICT;

CREATE TABLE schedule_occurrences (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    rule_id TEXT NOT NULL REFERENCES schedule_rules(id),
    rule_revision INTEGER NOT NULL CHECK(rule_revision >= 0),
    civil_date TEXT NOT NULL CHECK(length(civil_date) = 10),
    hour INTEGER NOT NULL CHECK(hour BETWEEN 0 AND 23),
    minute INTEGER NOT NULL CHECK(minute BETWEEN 0 AND 59),
    second INTEGER NOT NULL CHECK(second BETWEEN 0 AND 59),
    start_ms INTEGER CHECK(start_ms IS NULL OR start_ms >= 0),
    end_ms INTEGER CHECK(end_ms IS NULL OR (start_ms IS NOT NULL AND end_ms > start_ms)),
    offset_seconds INTEGER,
    transition_ms INTEGER CHECK(transition_ms IS NULL OR transition_ms >= 0),
    duration_seconds INTEGER NOT NULL CHECK(duration_seconds BETWEEN 1 AND 900),
    maximum_bytes INTEGER NOT NULL CHECK(maximum_bytes BETWEEN 1 AND 268435456),
    state TEXT NOT NULL CHECK(state IN ('waiting', 'admitted', 'missed')),
    miss_reason TEXT CHECK(miss_reason IS NULL OR miss_reason IN ('elapsed', 'spring_forward')),
    recording_id TEXT REFERENCES recordings(id),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    CHECK(
        (state = 'waiting' AND miss_reason IS NULL AND recording_id IS NULL AND (
            (start_ms IS NOT NULL AND end_ms > start_ms AND transition_ms IS NULL)
            OR (start_ms IS NULL AND end_ms IS NULL AND transition_ms IS NOT NULL)
        ))
        OR (state = 'missed' AND recording_id IS NULL AND (
            (miss_reason = 'elapsed' AND start_ms IS NOT NULL AND end_ms > start_ms)
            OR (miss_reason = 'spring_forward' AND start_ms IS NULL AND transition_ms IS NOT NULL)
        ))
        OR (state = 'admitted' AND recording_id IS NOT NULL AND miss_reason IS NULL
            AND start_ms IS NOT NULL AND end_ms > start_ms AND transition_ms IS NULL)
    ),
    UNIQUE(rule_id, civil_date, hour, minute, second)
) STRICT;

CREATE UNIQUE INDEX schedule_one_waiting
ON schedule_occurrences(rule_id) WHERE state = 'waiting';

CREATE TRIGGER schedule_occurrences_keep_resolved
BEFORE DELETE ON schedule_occurrences
BEGIN
    SELECT RAISE(ABORT, 'resolved occurrence is retained')
    WHERE OLD.state != 'waiting';
END;

DROP TRIGGER recording_interval_is_sealed_segment;
CREATE TRIGGER recording_interval_is_sealed_segment
BEFORE INSERT ON recording_intervals
BEGIN
    SELECT RAISE(ABORT, 'interval is not a sealed segment')
    WHERE NOT EXISTS (
        SELECT 1
        FROM recordings AS r
        JOIN capture_jobs AS c ON c.id = r.id
        WHERE r.id = NEW.recording_id
          AND (
            (
                r.storage_state = 'retained'
                AND c.state = 'starting'
                AND r.open_ceiling = 0
                AND r.open_object_key IS NULL
                AND r.media_bytes = NEW.byte_end
                AND r.decoded_microseconds = NEW.decoded_end_us - NEW.decoded_start_us
                AND r.sha256 = NEW.sha256
                AND r.format = NEW.format
                AND r.object_key = NEW.object_key
                AND NEW.ordinal = 0
                AND NEW.byte_start = 0
                AND NEW.decoded_start_us = COALESCE((
                    SELECT end_us FROM recording_gaps
                    WHERE recording_id = NEW.recording_id AND start_us = 0
                ), 0)
            )
            OR (
                c.state = 'running'
                AND r.storage_state = 'reserved'
                AND r.open_ceiling > 0
                AND r.open_object_key = NEW.object_key
                AND NEW.ceiling_bytes = r.open_ceiling
                AND NEW.byte_end - NEW.byte_start <= r.open_ceiling
            )
          )
    );
END;
DROP TRIGGER recording_intervals_contiguous;
CREATE TRIGGER recording_intervals_contiguous
BEFORE INSERT ON recording_intervals
BEGIN
    SELECT RAISE(ABORT, 'interval is out of order')
    WHERE NEW.ordinal != COALESCE((
            SELECT MAX(ordinal) + 1 FROM recording_intervals WHERE recording_id = NEW.recording_id
        ), 0)
        OR (NEW.ordinal = 0 AND (
            NEW.byte_start != 0
            OR NEW.decoded_start_us != COALESCE((
                SELECT end_us FROM recording_gaps
                WHERE recording_id = NEW.recording_id AND start_us = 0
            ), 0)
        ))
        OR (
            NEW.ordinal > 0
            AND (
                NEW.byte_start != (
                    SELECT byte_end FROM recording_intervals
                    WHERE recording_id = NEW.recording_id AND ordinal = NEW.ordinal - 1
                )
                OR NEW.decoded_start_us != (
                    SELECT decoded_end_us FROM recording_intervals
                    WHERE recording_id = NEW.recording_id AND ordinal = NEW.ordinal - 1
                )
            )
        );
END;

CREATE TRIGGER schedule_admitted_plan_immutable
BEFORE UPDATE OF start_ms, end_ms, offset_seconds, duration_seconds, maximum_bytes, recording_id, rule_revision, civil_date, hour, minute, second
ON schedule_occurrences
WHEN OLD.state = 'admitted'
BEGIN
    SELECT RAISE(ABORT, 'admitted plan is immutable');
END;

PRAGMA user_version = 20;
