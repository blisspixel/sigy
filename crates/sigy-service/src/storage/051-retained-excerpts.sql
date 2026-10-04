-- Null extension columns preserve every legacy receipt and its original digest.
ALTER TABLE retained_readers ADD COLUMN excerpt_end_us INTEGER
CHECK(excerpt_end_us IS NULL OR (excerpt_end_us > seek_us AND excerpt_end_us <= timeline_end_us));
ALTER TABLE retained_readers ADD COLUMN citation_monitor TEXT
CHECK(citation_monitor IS NULL OR (length(citation_monitor) BETWEEN 1 AND 128 AND citation_monitor NOT GLOB '*[^a-zA-Z0-9_:.-]*'));
ALTER TABLE retained_readers ADD COLUMN citation_finding TEXT
CHECK(citation_finding IS NULL OR (length(citation_finding) BETWEEN 1 AND 128 AND citation_finding NOT GLOB '*[^a-zA-Z0-9_:.-]*'));
ALTER TABLE retained_readers ADD COLUMN citation_transcript TEXT
CHECK(citation_transcript IS NULL OR (length(citation_transcript) BETWEEN 1 AND 128 AND citation_transcript NOT GLOB '*[^a-zA-Z0-9_:.-]*'));
ALTER TABLE retained_readers ADD COLUMN citation_transcript_revision INTEGER
CHECK(citation_transcript_revision IS NULL OR citation_transcript_revision BETWEEN 1 AND 64);
ALTER TABLE retained_readers ADD COLUMN citation_translation_revision INTEGER
CHECK(citation_translation_revision IS NULL OR citation_translation_revision BETWEEN 1 AND 64);
ALTER TABLE retained_readers ADD COLUMN citation_cue_ordinal INTEGER
CHECK(citation_cue_ordinal IS NULL OR citation_cue_ordinal BETWEEN 0 AND 255);
ALTER TABLE retained_readers ADD COLUMN excerpt_version INTEGER CHECK(
  (excerpt_version IS NULL AND excerpt_end_us IS NULL AND citation_monitor IS NULL
   AND citation_finding IS NULL AND citation_transcript IS NULL
   AND citation_transcript_revision IS NULL AND citation_translation_revision IS NULL
   AND citation_cue_ordinal IS NULL)
  OR (excerpt_version IS 2 AND excerpt_end_us IS NOT NULL AND (
    (citation_monitor IS NULL AND citation_finding IS NULL AND citation_transcript IS NULL
     AND citation_transcript_revision IS NULL AND citation_translation_revision IS NULL
     AND citation_cue_ordinal IS NULL)
    OR (citation_monitor IS NOT NULL AND citation_finding IS NOT NULL
     AND citation_transcript IS NOT NULL AND citation_transcript_revision IS NOT NULL
     AND citation_translation_revision IS NOT NULL AND citation_cue_ordinal IS NOT NULL)
  ))
);
DROP TRIGGER retained_readers_identity;
CREATE TRIGGER retained_readers_identity BEFORE UPDATE OF id,recording_id,seek_us,generation,source_revision,ordinal,object_key,sha256,format,bytes,timeline_start_us,timeline_end_us,file_seek_us,file_duration_us,spec_sha256,admitted_ms,excerpt_version,excerpt_end_us,citation_monitor,citation_finding,citation_transcript,citation_transcript_revision,citation_translation_revision,citation_cue_ordinal ON retained_readers BEGIN
    SELECT RAISE(ABORT,'retained reader identity immutable');
END;
CREATE TRIGGER retained_excerpts_admission BEFORE INSERT ON retained_readers
WHEN NEW.excerpt_version IS 2 BEGIN
    SELECT RAISE(ABORT,'retained excerpt range') WHERE EXISTS (
      SELECT 1 FROM recording_gaps g WHERE g.recording_id=NEW.recording_id
      AND g.start_us<NEW.excerpt_end_us AND g.end_us>NEW.seek_us
    );
    SELECT RAISE(ABORT,'retained excerpt citation') WHERE NEW.citation_monitor IS NOT NULL AND NOT EXISTS (
      SELECT 1 FROM monitor_findings f
      JOIN transcripts t ON t.id=f.transcript_id AND t.revision=f.transcript_revision
      JOIN transcript_cues tc ON tc.transcript_id=t.id AND tc.revision=t.revision AND tc.ordinal=f.cue_ordinal
      JOIN recordings r ON r.id=f.recording_id
      WHERE f.monitor_id=NEW.citation_monitor AND f.id=NEW.citation_finding
      AND f.transcript_id=NEW.citation_transcript
      AND f.transcript_revision=NEW.citation_transcript_revision
      AND f.translation_revision=NEW.citation_translation_revision
      AND f.cue_ordinal=NEW.citation_cue_ordinal AND f.recording_id=NEW.recording_id
      AND f.original_state='retained' AND f.start_us=NEW.seek_us AND f.end_us=NEW.excerpt_end_us
      AND tc.start_us=f.start_us AND tc.end_us=f.end_us AND t.recording_id=r.id
      AND t.role='original' AND t.outcome='text' AND t.kind IN ('recognition','correction')
      AND r.storage_state='retained' AND r.sha256=t.media_sha256
      AND EXISTS(SELECT 1 FROM translation_cues tr WHERE tr.transcript_id=f.transcript_id
        AND tr.transcript_revision=f.transcript_revision AND tr.revision=f.translation_revision
        AND tr.ordinal=f.cue_ordinal)
    );
END;
PRAGMA user_version = 51;
