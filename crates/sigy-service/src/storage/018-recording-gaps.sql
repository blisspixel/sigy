-- A gap is a hole in one capture timeline. It is not a media file.
CREATE TABLE recording_gaps (
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0 AND ordinal < 1024),
    cause TEXT NOT NULL CHECK(cause IN (
        'disconnect',
        'recovery',
        'codec_change',
        'refused_renewal',
        'capture_pause',
        'backward_clock'
    )),
    start_us INTEGER NOT NULL CHECK(start_us >= 0),
    end_us INTEGER NOT NULL CHECK(end_us > start_us),
    PRIMARY KEY (recording_id, ordinal)
) STRICT;

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

PRAGMA user_version = 18;
