CREATE TABLE dvr_policy (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    quota_bytes INTEGER NOT NULL CHECK(quota_bytes >= 0),
    minimum_free_bytes INTEGER NOT NULL CHECK(minimum_free_bytes >= 67108864),
    retention_days INTEGER NOT NULL CHECK(retention_days BETWEEN 1 AND 36500),
    decoder TEXT
) STRICT;
INSERT INTO dvr_policy VALUES (1, 50000000000, 268435456, 14, NULL);

CREATE TABLE recordings (
    id TEXT PRIMARY KEY NOT NULL REFERENCES capture_jobs(id),
    object_key TEXT UNIQUE NOT NULL CHECK(length(object_key) = 32 AND object_key NOT GLOB '*[^0-9a-f]*'),
    duration_seconds INTEGER NOT NULL CHECK(duration_seconds BETWEEN 1 AND 900),
    initial_retention TEXT NOT NULL CHECK(initial_retention IN ('temporary', 'kept', 'archived')),
    retention TEXT NOT NULL CHECK(retention IN ('temporary', 'kept', 'archived')),
    storage_state TEXT NOT NULL CHECK(storage_state IN ('reserved', 'retained', 'deleting', 'deleted')),
    charged_bytes INTEGER NOT NULL CHECK(charged_bytes >= 0),
    media_bytes INTEGER CHECK(media_bytes > 0),
    sha256 TEXT CHECK(length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    format TEXT CHECK(format IN ('mp3', 'aac', 'flac', 'ogg', 'wav')),
    decoded_microseconds INTEGER CHECK(decoded_microseconds > 0),
    end_reason TEXT CHECK(end_reason IN ('end_of_body', 'byte_limit', 'duration_limit', 'user_stop')),
    processing_receipt TEXT,
    failure_detail TEXT CHECK(length(failure_detail) <= 256),
    CHECK((storage_state = 'reserved' AND charged_bytes > 0 AND media_bytes IS NULL)
       OR (storage_state = 'retained' AND media_bytes IS NOT NULL AND charged_bytes = media_bytes)
       OR storage_state = 'deleting'
       OR (storage_state = 'deleted' AND charged_bytes = 0)),
    CHECK((media_bytes IS NULL AND sha256 IS NULL AND format IS NULL AND decoded_microseconds IS NULL AND end_reason IS NULL)
       OR (media_bytes IS NOT NULL AND sha256 IS NOT NULL AND format IS NOT NULL AND decoded_microseconds IS NOT NULL AND end_reason IS NOT NULL))
) STRICT;
CREATE TRIGGER recording_intent_immutable BEFORE UPDATE OF id, object_key, duration_seconds, initial_retention ON recordings BEGIN
    SELECT RAISE(ABORT, 'recording intent is immutable');
END;
CREATE TRIGGER recordings_no_delete BEFORE DELETE ON recordings BEGIN
    SELECT RAISE(ABORT, 'recording provenance is retained');
END;
PRAGMA user_version = 4;
