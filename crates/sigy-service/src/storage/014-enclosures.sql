-- One explicit enclosure download reserves a fixed episode ceiling.
-- Radio recordings stay within 15 minutes and 256 MiB.
CREATE TABLE recordings_v14 (
    id TEXT PRIMARY KEY NOT NULL REFERENCES capture_jobs(id),
    object_key TEXT UNIQUE NOT NULL CHECK(length(object_key) = 32 AND object_key NOT GLOB '*[^0-9a-f]*'),
    duration_seconds INTEGER NOT NULL,
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
    http_route_json TEXT CHECK(http_route_json IS NULL OR length(http_route_json) <= 8192),
    metadata_requested INTEGER NOT NULL CHECK(metadata_requested IN (0, 1)),
    profile TEXT NOT NULL CHECK(profile IN ('radio', 'episode')),
    byte_ceiling INTEGER NOT NULL,
    CHECK((storage_state = 'reserved' AND charged_bytes > 0 AND media_bytes IS NULL)
       OR (storage_state = 'retained' AND media_bytes IS NOT NULL AND charged_bytes = media_bytes)
       OR storage_state = 'deleting'
       OR (storage_state = 'deleted' AND charged_bytes = 0)),
    CHECK((media_bytes IS NULL AND sha256 IS NULL AND format IS NULL AND decoded_microseconds IS NULL AND end_reason IS NULL)
       OR (media_bytes IS NOT NULL AND sha256 IS NOT NULL AND format IS NOT NULL AND decoded_microseconds IS NOT NULL AND end_reason IS NOT NULL)),
    CHECK((profile = 'radio' AND duration_seconds BETWEEN 1 AND 900 AND byte_ceiling BETWEEN 1 AND 268435456)
       OR (profile = 'episode' AND duration_seconds = 1800 AND byte_ceiling = 536870912
           AND metadata_requested = 0 AND (end_reason IS NULL OR end_reason = 'end_of_body')))
) STRICT;
INSERT INTO recordings_v14(
    id, object_key, duration_seconds, initial_retention, retention, storage_state, charged_bytes,
    media_bytes, sha256, format, decoded_microseconds, end_reason, processing_receipt, failure_detail,
    http_route_json, metadata_requested, profile, byte_ceiling
)
SELECT
    r.id, r.object_key, r.duration_seconds, r.initial_retention, r.retention, r.storage_state, r.charged_bytes,
    r.media_bytes, r.sha256, r.format, r.decoded_microseconds, r.end_reason, r.processing_receipt, r.failure_detail,
    r.http_route_json, r.metadata_requested, 'radio', c.maximum_bytes
FROM recordings AS r
JOIN capture_jobs AS c ON c.id = r.id;
CREATE TABLE recording_observations_v14 (
    recording_id TEXT NOT NULL REFERENCES recordings_v14(id),
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0 AND ordinal < 1024),
    audio_offset INTEGER NOT NULL CHECK(audio_offset >= 0),
    text TEXT NOT NULL CHECK(length(text) BETWEEN 1 AND 4080),
    PRIMARY KEY (recording_id, ordinal)
) STRICT;
INSERT INTO recording_observations_v14(recording_id, ordinal, audio_offset, text)
SELECT recording_id, ordinal, audio_offset, text FROM recording_observations;
DROP TABLE recording_observations;
DROP TABLE recordings;
ALTER TABLE recordings_v14 RENAME TO recordings;
ALTER TABLE recording_observations_v14 RENAME TO recording_observations;
CREATE TRIGGER recording_intent_immutable
BEFORE UPDATE OF id, object_key, duration_seconds, initial_retention, profile, byte_ceiling ON recordings
BEGIN
    SELECT RAISE(ABORT, 'recording intent is immutable');
END;
CREATE TRIGGER recordings_no_delete BEFORE DELETE ON recordings
BEGIN
    SELECT RAISE(ABORT, 'recording provenance is retained');
END;
CREATE TRIGGER recording_route_immutable BEFORE UPDATE OF http_route_json ON recordings
    WHEN OLD.http_route_json IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'published HTTP provenance is immutable');
END;
CREATE TRIGGER recording_metadata_request_immutable BEFORE UPDATE OF metadata_requested ON recordings
BEGIN
    SELECT RAISE(ABORT, 'recording metadata request is immutable');
END;
CREATE TRIGGER recording_observation_immutable BEFORE UPDATE ON recording_observations
BEGIN
    SELECT RAISE(ABORT, 'recording observation is immutable');
END;
CREATE TRIGGER recording_observations_no_delete BEFORE DELETE ON recording_observations
BEGIN
    SELECT RAISE(ABORT, 'recording observation is retained');
END;
CREATE TABLE podcast_downloads (
    recording_id TEXT PRIMARY KEY NOT NULL REFERENCES recordings(id),
    subscription_id TEXT NOT NULL REFERENCES podcast_subscriptions(id),
    episode_id TEXT NOT NULL CHECK(length(episode_id) = 64 AND episode_id NOT GLOB '*[^0-9a-f]*'),
    source_revision TEXT NOT NULL REFERENCES source_revisions(id)
) STRICT;
CREATE INDEX podcast_downloads_by_episode ON podcast_downloads(subscription_id, episode_id);
CREATE TRIGGER podcast_download_immutable BEFORE UPDATE ON podcast_downloads
BEGIN
    SELECT RAISE(ABORT, 'podcast download is immutable');
END;
CREATE TRIGGER podcast_downloads_retained BEFORE DELETE ON podcast_downloads
BEGIN
    SELECT RAISE(ABORT, 'podcast download is retained');
END;
PRAGMA user_version = 14;
