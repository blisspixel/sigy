-- A reader protects one frozen sealed object until joined filesystem completion.
CREATE TABLE retained_readers (
    id TEXT PRIMARY KEY CHECK(length(id) BETWEEN 1 AND 128 AND id NOT GLOB '*[^a-zA-Z0-9_:.-]*'),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    seek_us INTEGER NOT NULL CHECK(seek_us >= 0),
    generation INTEGER NOT NULL CHECK(generation = 1),
    source_revision TEXT NOT NULL REFERENCES source_revisions(id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 1023),
    object_key TEXT NOT NULL CHECK(length(object_key) = 32 AND object_key NOT GLOB '*[^0-9a-f]*'),
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    format TEXT NOT NULL CHECK(format IN ('wav','mp3','aac','flac','ogg','mpegts')),
    bytes INTEGER NOT NULL CHECK(bytes BETWEEN 1 AND 536870912),
    timeline_start_us INTEGER NOT NULL CHECK(timeline_start_us >= 0),
    timeline_end_us INTEGER NOT NULL CHECK(timeline_end_us > timeline_start_us),
    file_seek_us INTEGER NOT NULL CHECK(file_seek_us >= 0 AND file_seek_us = seek_us - timeline_start_us AND seek_us < timeline_end_us),
    file_duration_us INTEGER NOT NULL CHECK(file_duration_us BETWEEN 1 AND 1800000000 AND file_duration_us = timeline_end_us - timeline_start_us),
    spec_sha256 TEXT NOT NULL CHECK(length(spec_sha256) = 64 AND spec_sha256 NOT GLOB '*[^0-9a-f]*'),
    state TEXT NOT NULL CHECK(state IN ('running','cancelling','recovery_held','completed','failed')),
    admitted_ms INTEGER NOT NULL CHECK(admitted_ms >= 0),
    updated_ms INTEGER NOT NULL CHECK(updated_ms >= admitted_ms),
    completion_reason TEXT CHECK(length(completion_reason) BETWEEN 1 AND 128 AND completion_reason NOT GLOB '*[^a-zA-Z0-9_:.-]*'),
    recovery_reason TEXT CHECK(length(recovery_reason) BETWEEN 1 AND 128 AND recovery_reason NOT GLOB '*[^a-zA-Z0-9_:.-]*'),
    CHECK((state IN ('completed','failed')) = (completion_reason IS NOT NULL)),
    CHECK(state != 'recovery_held' OR recovery_reason IS NOT NULL),
    FOREIGN KEY(recording_id,ordinal) REFERENCES recording_intervals(recording_id,ordinal)
) STRICT;
CREATE INDEX retained_readers_protection ON retained_readers(recording_id,ordinal)
WHERE state IN ('running','cancelling','recovery_held');
CREATE INDEX retained_readers_inspection ON retained_readers((state IN ('running','cancelling','recovery_held')) DESC,admitted_ms DESC,id);
CREATE TRIGGER retained_readers_admission BEFORE INSERT ON retained_readers BEGIN
    SELECT RAISE(ABORT,'retained reader capacity') WHERE
      (SELECT count(*) FROM retained_readers) >= 4096 OR
      (SELECT count(*) FROM retained_readers WHERE state IN ('running','cancelling','recovery_held')) >= 4;
    SELECT RAISE(ABORT,'retained reader identity') WHERE NEW.state != 'running' OR NOT EXISTS (
      SELECT 1 FROM recordings r JOIN capture_jobs c ON c.id=r.id
      JOIN recording_intervals i ON i.recording_id=r.id AND i.ordinal=NEW.ordinal
      WHERE r.id=NEW.recording_id AND r.storage_state IN ('reserved','retained')
      AND c.source_revision=NEW.source_revision AND i.object_key=NEW.object_key
      AND i.sha256=NEW.sha256 AND i.format=NEW.format AND i.byte_end-i.byte_start=NEW.bytes
      AND i.decoded_start_us=NEW.timeline_start_us AND i.decoded_end_us=NEW.timeline_end_us
      AND (r.open_object_key IS NULL OR r.open_object_key != i.object_key)
      AND NOT EXISTS(SELECT 1 FROM recording_releases x WHERE x.recording_id=i.recording_id AND x.segment_ordinal=i.ordinal)
      AND NOT EXISTS(SELECT 1 FROM recording_gaps g WHERE g.recording_id=r.id AND g.start_us<=NEW.seek_us AND g.end_us>NEW.seek_us)
    );
END;
CREATE TRIGGER retained_readers_identity BEFORE UPDATE OF id,recording_id,seek_us,generation,source_revision,ordinal,object_key,sha256,format,bytes,timeline_start_us,timeline_end_us,file_seek_us,file_duration_us,spec_sha256,admitted_ms ON retained_readers BEGIN
    SELECT RAISE(ABORT,'retained reader identity immutable');
END;
CREATE TRIGGER retained_readers_transition BEFORE UPDATE ON retained_readers BEGIN
    SELECT RAISE(ABORT,'retained reader transition') WHERE NEW.updated_ms<OLD.updated_ms OR
      OLD.state IN ('completed','failed') OR
      (NEW.state!=OLD.state AND NOT (
        (OLD.state='running' AND NEW.state IN ('cancelling','recovery_held','completed','failed')) OR
        (OLD.state='cancelling' AND NEW.state IN ('recovery_held','completed','failed')) OR
        (OLD.state='recovery_held' AND NEW.state IN ('completed','failed'))
      ));
END;
CREATE TRIGGER retained_readers_no_delete BEFORE DELETE ON retained_readers BEGIN
    SELECT RAISE(ABORT,'retained reader receipt immutable');
END;
CREATE TRIGGER retained_reader_delete_guard BEFORE UPDATE OF storage_state ON recordings
WHEN NEW.storage_state IN ('deleting','deleted') AND EXISTS (
    SELECT 1 FROM retained_readers q WHERE q.recording_id=OLD.id AND q.state IN ('running','cancelling','recovery_held')) BEGIN
    SELECT RAISE(ABORT,'recording has retained reader');
END;
CREATE TRIGGER retained_reader_release_guard BEFORE INSERT ON recording_releases
WHEN EXISTS (SELECT 1 FROM retained_readers q WHERE q.recording_id=NEW.recording_id AND q.ordinal=NEW.segment_ordinal AND q.state IN ('running','cancelling','recovery_held')) BEGIN
    SELECT RAISE(ABORT,'segment has retained reader');
END;
PRAGMA user_version = 49;
