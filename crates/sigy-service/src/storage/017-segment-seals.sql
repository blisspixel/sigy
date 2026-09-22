-- A running capture can seal many measured segments. The open tail is not an
-- interval. The admitted byte budget stays one reservation.
ALTER TABLE recordings ADD COLUMN escrow_bytes INTEGER NOT NULL DEFAULT 0 CHECK(escrow_bytes >= 0);
ALTER TABLE recordings ADD COLUMN open_ceiling INTEGER NOT NULL DEFAULT 0 CHECK(open_ceiling >= 0);
ALTER TABLE recordings ADD COLUMN open_object_key TEXT CHECK(
    open_object_key IS NULL
    OR (length(open_object_key) = 32 AND open_object_key NOT GLOB '*[^0-9a-f]*')
);
ALTER TABLE recordings ADD COLUMN lease_expires_ms INTEGER CHECK(lease_expires_ms IS NULL OR lease_expires_ms >= 0);
ALTER TABLE recordings ADD COLUMN lease_renewals INTEGER NOT NULL DEFAULT 0 CHECK(lease_renewals >= 0);
UPDATE recordings SET escrow_bytes = charged_bytes WHERE storage_state = 'reserved';
CREATE UNIQUE INDEX recording_open_object_keys ON recordings(open_object_key) WHERE open_object_key IS NOT NULL;

CREATE TABLE recording_intervals_v17 (
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0 AND ordinal < 1024),
    decoded_start_us INTEGER NOT NULL CHECK(decoded_start_us >= 0),
    decoded_end_us INTEGER NOT NULL CHECK(decoded_end_us > decoded_start_us),
    byte_start INTEGER NOT NULL CHECK(byte_start >= 0),
    byte_end INTEGER NOT NULL CHECK(byte_end > byte_start),
    object_key TEXT NOT NULL UNIQUE CHECK(length(object_key) = 32 AND object_key NOT GLOB '*[^0-9a-f]*'),
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    format TEXT NOT NULL CHECK(format IN ('mp3', 'aac', 'flac', 'ogg', 'wav')),
    ceiling_bytes INTEGER NOT NULL CHECK(ceiling_bytes > 0 AND byte_end - byte_start <= ceiling_bytes),
    PRIMARY KEY (recording_id, ordinal)
) STRICT;
INSERT INTO recording_intervals_v17(
    recording_id, ordinal, decoded_start_us, decoded_end_us, byte_start, byte_end,
    object_key, sha256, format, ceiling_bytes
)
SELECT
    i.recording_id, i.ordinal, i.decoded_start_us, i.decoded_end_us, i.byte_start, i.byte_end,
    r.object_key, r.sha256, r.format, r.byte_ceiling
FROM recording_intervals AS i
JOIN recordings AS r ON r.id = i.recording_id
WHERE r.sha256 IS NOT NULL AND r.format IS NOT NULL AND r.media_bytes IS NOT NULL;
DROP TABLE recording_intervals;
ALTER TABLE recording_intervals_v17 RENAME TO recording_intervals;

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
                AND r.decoded_microseconds = NEW.decoded_end_us
                AND r.sha256 = NEW.sha256
                AND r.format = NEW.format
                AND r.object_key = NEW.object_key
                AND NEW.ordinal = 0
                AND NEW.byte_start = 0
                AND NEW.decoded_start_us = 0
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
CREATE TRIGGER recording_intervals_contiguous
BEFORE INSERT ON recording_intervals
BEGIN
    SELECT RAISE(ABORT, 'interval is out of order')
    WHERE NEW.ordinal != COALESCE((
            SELECT MAX(ordinal) + 1 FROM recording_intervals WHERE recording_id = NEW.recording_id
        ), 0)
        OR (NEW.ordinal = 0 AND (NEW.byte_start != 0 OR NEW.decoded_start_us != 0))
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
PRAGMA user_version = 17;
