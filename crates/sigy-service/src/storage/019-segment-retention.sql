-- A saved range protects whole sealed segments. Aged temporary segments
-- release their files. The interval row stays. The open tail is not a segment.
CREATE TABLE recording_segment_clocks (
    recording_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
    sealed_ms INTEGER NOT NULL CHECK(sealed_ms >= 0),
    PRIMARY KEY (recording_id, ordinal)
) STRICT;
INSERT INTO recording_segment_clocks(recording_id, ordinal, sealed_ms)
SELECT i.recording_id, i.ordinal, c.created_ms
FROM recording_intervals AS i
JOIN capture_jobs AS c ON c.id = i.recording_id;

CREATE TABLE recording_holds (
    recording_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
    start_us INTEGER NOT NULL CHECK(start_us >= 0),
    end_us INTEGER NOT NULL CHECK(end_us > start_us),
    PRIMARY KEY (recording_id, ordinal)
) STRICT;
CREATE TRIGGER recording_holds_ordered
BEFORE INSERT ON recording_holds
BEGIN
    SELECT RAISE(ABORT, 'hold is out of order')
    WHERE NEW.ordinal != COALESCE((
        SELECT MAX(ordinal) + 1 FROM recording_holds WHERE recording_id = NEW.recording_id
    ), 0);
END;

CREATE TABLE recording_hold_segments (
    recording_id TEXT NOT NULL,
    hold_ordinal INTEGER NOT NULL CHECK(hold_ordinal >= 0),
    segment_ordinal INTEGER NOT NULL CHECK(segment_ordinal >= 0),
    PRIMARY KEY (recording_id, hold_ordinal, segment_ordinal)
) STRICT;

CREATE TABLE recording_hold_gaps (
    recording_id TEXT NOT NULL,
    hold_ordinal INTEGER NOT NULL CHECK(hold_ordinal >= 0),
    gap_ordinal INTEGER NOT NULL CHECK(gap_ordinal >= 0),
    PRIMARY KEY (recording_id, hold_ordinal, gap_ordinal)
) STRICT;

CREATE TABLE recording_releases (
    recording_id TEXT NOT NULL,
    segment_ordinal INTEGER NOT NULL CHECK(segment_ordinal >= 0),
    byte_length INTEGER NOT NULL CHECK(byte_length > 0),
    PRIMARY KEY (recording_id, segment_ordinal)
) STRICT;
CREATE TRIGGER recording_releases_immutable
BEFORE UPDATE ON recording_releases
BEGIN
    SELECT RAISE(ABORT, 'released segment is immutable');
END;
CREATE TRIGGER recording_releases_no_delete
BEFORE DELETE ON recording_releases
BEGIN
    SELECT RAISE(ABORT, 'released segment stays recorded');
END;

PRAGMA user_version = 19;
