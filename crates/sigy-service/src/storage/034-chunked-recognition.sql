-- Chunked recognition. The coverage table is rebuilt in Rust before this file runs,
-- so a migrated row can still equal a whole interval of up to 60 seconds. New rows
-- are capped at 30 seconds by the insert trigger below.
DROP TRIGGER analysis_asr_input;
CREATE TRIGGER analysis_asr_input
BEFORE INSERT ON analysis_jobs
WHEN NEW.kind = 'local_asr' AND (
    NEW.expected_parent_revision != coalesce((SELECT max(revision) FROM transcripts WHERE id = NEW.analysis_id), 0)
    OR NOT EXISTS (
        SELECT 1 FROM analysis_inputs a
        WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
          AND json_array_length(a.timeline_json, '$.intervals') BETWEEN 1 AND 1024
          AND json_array_length(a.timeline_json, '$.intervals') = NEW.expected_files
          AND NOT EXISTS (
              SELECT 1 FROM json_each(a.timeline_json, '$.intervals') s
              WHERE json_extract(s.value, '$.end_us') - json_extract(s.value, '$.start_us') < 1
          )
    )
)
BEGIN SELECT RAISE(ABORT, 'invalid recognition request'); END;
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
                )))
    )
BEGIN SELECT RAISE(ABORT, 'invalid or sealed transcript cue'); END;
CREATE TRIGGER transcript_coverage_insert
BEFORE INSERT ON transcript_coverage
WHEN EXISTS (SELECT 1 FROM analysis_decisions WHERE transcript_id = NEW.transcript_id AND transcript_revision = NEW.revision)
    OR NEW.end_us - NEW.start_us > 30000000
    OR NEW.sample_count > NEW.sample_rate * 30
    OR EXISTS (
        SELECT 1 FROM transcript_coverage c
        WHERE c.transcript_id = NEW.transcript_id AND c.revision = NEW.revision
          AND c.start_us < NEW.end_us AND NEW.start_us < c.end_us
    )
    OR (
        SELECT count(*) FROM transcripts t
        JOIN analysis_inputs a ON a.id = t.analysis_id AND a.revision = t.analysis_revision,
             json_each(a.timeline_json, '$.intervals') s
        WHERE t.id = NEW.transcript_id AND t.revision = NEW.revision AND t.kind = 'recognition'
          AND NEW.interval_ordinal = json_extract(s.value, '$.ordinal')
          AND NEW.source_sha256 = json_extract(s.value, '$.sha256')
          AND NEW.start_us >= json_extract(s.value, '$.start_us')
          AND NEW.end_us <= json_extract(s.value, '$.end_us')
    ) != 1
    OR EXISTS (
        SELECT 1 FROM transcripts t
        JOIN analysis_inputs a ON a.id = t.analysis_id AND a.revision = t.analysis_revision,
             json_each(a.timeline_json, '$.gaps') g
        WHERE t.id = NEW.transcript_id AND t.revision = NEW.revision
          AND NEW.start_us < json_extract(g.value, '$.end_us')
          AND NEW.end_us > json_extract(g.value, '$.start_us')
    )
BEGIN SELECT RAISE(ABORT, 'invalid or sealed transcript coverage'); END;
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
            AND EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.id = t.job_id AND j.kind = 'local_asr' AND j.state = 'running' AND j.generation = t.job_generation)))
)
BEGIN SELECT RAISE(ABORT, 'transcript result is incomplete'); END;
DROP TRIGGER monitor_step_no_delete;
DELETE FROM monitor_steps
WHERE stage = 'recognition' AND decision = 'skipped' AND reason = 'recognition-input-limit';
CREATE TRIGGER monitor_step_no_delete BEFORE DELETE ON monitor_steps BEGIN SELECT RAISE(ABORT, 'monitor steps are retained'); END;
PRAGMA user_version = 34;
