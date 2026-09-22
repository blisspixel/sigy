-- One published file is one measured interval. The planned window and an
-- unpublished part file are not airtime and do not get a row.
CREATE TABLE recording_intervals (
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    ordinal INTEGER NOT NULL CHECK(ordinal = 0),
    decoded_start_us INTEGER NOT NULL CHECK(decoded_start_us = 0),
    decoded_end_us INTEGER NOT NULL CHECK(decoded_end_us > 0),
    byte_start INTEGER NOT NULL CHECK(byte_start = 0),
    byte_end INTEGER NOT NULL CHECK(byte_end > 0),
    PRIMARY KEY (recording_id, ordinal)
) STRICT;
INSERT INTO recording_intervals(
    recording_id, ordinal, decoded_start_us, decoded_end_us, byte_start, byte_end
)
SELECT id, 0, 0, decoded_microseconds, 0, media_bytes
FROM recordings
WHERE media_bytes IS NOT NULL;
CREATE TRIGGER recording_interval_matches_publication
BEFORE INSERT ON recording_intervals
BEGIN
    SELECT RAISE(ABORT, 'interval is not measured publication')
    WHERE NOT EXISTS (
        SELECT 1 FROM recordings
        WHERE id = NEW.recording_id
          AND media_bytes = NEW.byte_end
          AND decoded_microseconds = NEW.decoded_end_us
    );
END;
CREATE TRIGGER recording_intervals_immutable
BEFORE UPDATE ON recording_intervals
BEGIN
    SELECT RAISE(ABORT, 'published interval is immutable');
END;
CREATE TRIGGER recording_intervals_no_delete
BEFORE DELETE ON recording_intervals
BEGIN
    SELECT RAISE(ABORT, 'published interval is retained');
END;
PRAGMA user_version = 16;
