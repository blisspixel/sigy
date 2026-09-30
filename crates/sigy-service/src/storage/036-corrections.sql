-- A user correction is a new transcript revision. The previous revision stays.
-- Cue times are copied. No recognition job, coverage row, or paid request is created.
DROP TRIGGER IF EXISTS transcript_cue_insert;
CREATE TRIGGER transcript_cue_insert
BEFORE INSERT ON transcript_cues
WHEN EXISTS (SELECT 1 FROM analysis_decisions WHERE transcript_id = NEW.transcript_id AND transcript_revision = NEW.revision)
    OR EXISTS (SELECT 1 FROM transcript_cues WHERE transcript_id = NEW.transcript_id AND revision = NEW.revision AND ordinal = NEW.ordinal)
    OR NOT EXISTS (
        SELECT 1 FROM transcripts t WHERE t.id = NEW.transcript_id AND t.revision = NEW.revision
          AND ((t.kind = 'legacy_placeholder' AND NEW.script = '')
            OR (t.kind = 'recognition' AND t.outcome = 'text' AND length(CAST(NEW.script AS BLOB)) > 0
                AND NEW.ordinal < t.cue_count
                AND NEW.ordinal = (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision)
                AND length(CAST(NEW.script AS BLOB)) + coalesce((SELECT sum(length(CAST(script AS BLOB))) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision), 0) <= t.text_bytes
                AND NOT EXISTS (SELECT 1 FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision AND end_us > NEW.start_us)
                AND EXISTS (
                    SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.intervals') s
                    WHERE a.id = t.analysis_id AND a.revision = t.analysis_revision
                      AND NEW.start_us >= json_extract(s.value, '$.start_us') AND NEW.end_us <= json_extract(s.value, '$.end_us')
                )
                AND NOT EXISTS (
                    SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.gaps') g
                    WHERE a.id = t.analysis_id AND a.revision = t.analysis_revision
                      AND NEW.start_us < json_extract(g.value, '$.end_us') AND NEW.end_us > json_extract(g.value, '$.start_us')
                )
                AND EXISTS (
                    SELECT 1 FROM transcript_coverage c
                    WHERE c.transcript_id = t.id AND c.revision = t.revision
                      AND NEW.start_us >= c.start_us AND NEW.end_us <= c.end_us
                ))
            OR (t.kind = 'correction' AND t.outcome = 'text' AND length(CAST(NEW.script AS BLOB)) > 0
                AND NEW.ordinal < t.cue_count
                AND NEW.ordinal = (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision)
                AND length(CAST(NEW.script AS BLOB)) + coalesce((SELECT sum(length(CAST(script AS BLOB))) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision), 0) <= t.text_bytes
                AND NOT EXISTS (SELECT 1 FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision AND end_us > NEW.start_us)
                AND EXISTS (
                    SELECT 1 FROM transcript_cues p
                    WHERE p.transcript_id = t.id AND p.revision = t.parent_revision
                      AND p.ordinal = NEW.ordinal AND p.start_us = NEW.start_us AND p.end_us = NEW.end_us
                )))
    )
BEGIN SELECT RAISE(ABORT, 'invalid or sealed transcript cue'); END;
DROP TRIGGER IF EXISTS transcript_decision_complete;
CREATE TRIGGER transcript_decision_complete
BEFORE INSERT ON analysis_decisions
WHEN EXISTS (SELECT 1 FROM analysis_decisions WHERE transcript_id = NEW.transcript_id AND transcript_revision = NEW.transcript_revision)
    OR NOT EXISTS (
    SELECT 1 FROM transcripts t WHERE t.id = NEW.transcript_id AND t.revision = NEW.transcript_revision
      AND ((t.kind = 'legacy_placeholder' AND EXISTS (SELECT 1 FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision))
        OR (t.kind = 'recognition' AND NEW.created_ms = t.created_ms
            AND t.cue_count = (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision)
            AND t.text_bytes = coalesce((SELECT sum(length(CAST(script AS BLOB))) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision), 0)
            AND (SELECT count(*) FROM transcript_coverage WHERE transcript_id = t.id AND revision = t.revision) BETWEEN 1 AND 1024
            AND (SELECT min(ordinal) FROM transcript_coverage WHERE transcript_id = t.id AND revision = t.revision) = 0
            AND (SELECT max(ordinal) FROM transcript_coverage WHERE transcript_id = t.id AND revision = t.revision)
                = (SELECT count(*) - 1 FROM transcript_coverage WHERE transcript_id = t.id AND revision = t.revision)
            AND NOT EXISTS (
                SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.intervals') s
                WHERE a.id = t.analysis_id AND a.revision = t.analysis_revision AND (
                    (SELECT coalesce(sum(c.end_us - c.start_us), 0) FROM transcript_coverage c
                      WHERE c.transcript_id = t.id AND c.revision = t.revision
                        AND c.interval_ordinal = json_extract(s.value, '$.ordinal'))
                    != json_extract(s.value, '$.end_us') - json_extract(s.value, '$.start_us')
                    OR (SELECT min(c.start_us) FROM transcript_coverage c
                      WHERE c.transcript_id = t.id AND c.revision = t.revision
                        AND c.interval_ordinal = json_extract(s.value, '$.ordinal'))
                    != json_extract(s.value, '$.start_us')
                    OR (SELECT max(c.end_us) FROM transcript_coverage c
                      WHERE c.transcript_id = t.id AND c.revision = t.revision
                        AND c.interval_ordinal = json_extract(s.value, '$.ordinal'))
                    != json_extract(s.value, '$.end_us')
                )
            )
            AND NOT EXISTS (
                SELECT 1 FROM transcript_coverage c
                WHERE c.transcript_id = t.id AND c.revision = t.revision AND c.ordinal > 0
                  AND c.start_us != (
                    SELECT p.end_us FROM transcript_coverage p
                    WHERE p.transcript_id = c.transcript_id AND p.revision = c.revision
                      AND p.ordinal = c.ordinal - 1 AND p.interval_ordinal = c.interval_ordinal
                  )
                  AND EXISTS (
                    SELECT 1 FROM transcript_coverage p
                    WHERE p.transcript_id = c.transcript_id AND p.revision = c.revision
                      AND p.ordinal = c.ordinal - 1 AND p.interval_ordinal = c.interval_ordinal
                  )
            )
            AND EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.id = t.job_id AND j.kind = 'local_asr' AND j.state = 'running' AND j.generation = t.job_generation))
        OR (t.kind = 'correction' AND NEW.created_ms = t.created_ms
            AND t.cue_count = (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision)
            AND t.text_bytes = coalesce((SELECT sum(length(CAST(script AS BLOB))) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision), 0)
            AND t.cue_count = (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.parent_revision)
            AND NOT EXISTS (
                SELECT 1 FROM transcript_cues n
                WHERE n.transcript_id = t.id AND n.revision = t.revision
                  AND NOT EXISTS (
                      SELECT 1 FROM transcript_cues p
                      WHERE p.transcript_id = n.transcript_id AND p.revision = t.parent_revision
                        AND p.ordinal = n.ordinal AND p.start_us = n.start_us AND p.end_us = n.end_us
                  )
            )))
)
BEGIN SELECT RAISE(ABORT, 'transcript result is incomplete'); END;
DROP TRIGGER IF EXISTS transcript_correction_order;
CREATE TRIGGER transcript_correction_order
BEFORE INSERT ON transcripts
WHEN NEW.kind = 'correction' AND (
    NEW.parent_revision != (SELECT max(revision) FROM transcripts WHERE id = NEW.id)
    OR NOT EXISTS (
        SELECT 1 FROM transcripts p
        WHERE p.id = NEW.id AND p.revision = NEW.parent_revision AND p.outcome = 'text'
          AND p.analysis_id = NEW.analysis_id AND p.analysis_revision = NEW.analysis_revision
          AND p.recording_id = NEW.recording_id AND p.media_sha256 = NEW.media_sha256
          AND p.role = 'original'
    )
    OR NOT EXISTS (
        SELECT 1 FROM analysis_inputs a
        JOIN recordings r ON r.id = a.recording_id
        WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
          AND a.state = 'published'
          AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id)
          AND r.storage_state = 'retained'
          AND r.sha256 = NEW.media_sha256
    )
)
BEGIN SELECT RAISE(ABORT, 'transcript revision conflicts'); END;
PRAGMA user_version = 36;
