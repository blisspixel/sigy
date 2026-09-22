-- Explicit ICY metadata is an observation on one recording. It is not audio and not authority.
ALTER TABLE recordings ADD COLUMN metadata_requested INTEGER NOT NULL DEFAULT 0 CHECK(metadata_requested IN (0, 1));
CREATE TRIGGER recording_metadata_request_immutable BEFORE UPDATE OF metadata_requested ON recordings BEGIN
    SELECT RAISE(ABORT, 'recording metadata request is immutable');
END;
CREATE TABLE recording_observations (
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0 AND ordinal < 1024),
    audio_offset INTEGER NOT NULL CHECK(audio_offset >= 0),
    text TEXT NOT NULL CHECK(length(text) BETWEEN 1 AND 4080),
    PRIMARY KEY (recording_id, ordinal)
) STRICT;
CREATE TRIGGER recording_observation_immutable BEFORE UPDATE ON recording_observations BEGIN
    SELECT RAISE(ABORT, 'recording observation is immutable');
END;
CREATE TRIGGER recording_observations_no_delete BEFORE DELETE ON recording_observations BEGIN
    SELECT RAISE(ABORT, 'recording observation is retained');
END;
PRAGMA user_version = 11;
