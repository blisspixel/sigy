-- New findings reflect released sealed segments. Older immutable statements stay history.
DROP TRIGGER monitor_finding_citation;
CREATE TRIGGER monitor_finding_citation
BEFORE INSERT ON monitor_findings
WHEN NOT EXISTS (
    SELECT 1
    FROM transcripts AS t
    JOIN recordings AS r ON r.id = t.recording_id
    JOIN transcript_cues AS c
      ON c.transcript_id = t.id AND c.revision = t.revision AND c.ordinal = NEW.cue_ordinal
    JOIN translation_cues AS tc
      ON tc.transcript_id = t.id AND tc.transcript_revision = t.revision
     AND tc.revision = NEW.translation_revision AND tc.ordinal = NEW.cue_ordinal
    WHERE t.id = NEW.transcript_id
      AND t.revision = NEW.transcript_revision
      AND t.recording_id = NEW.recording_id
      AND t.role = 'original'
      AND t.outcome = 'text'
      AND t.kind IN ('recognition', 'correction')
      AND (
        (
            NEW.original_state = 'retained'
            AND r.storage_state = 'retained'
            AND r.sha256 = t.media_sha256
            AND NEW.start_us = c.start_us
            AND NEW.end_us = c.end_us
            AND EXISTS (
                SELECT 1 FROM recording_intervals AS i
                WHERE i.recording_id = r.id
                  AND i.decoded_start_us <= c.start_us
                  AND i.decoded_end_us >= c.end_us
                  AND NOT EXISTS (
                      SELECT 1 FROM recording_releases AS x
                      WHERE x.recording_id = i.recording_id AND x.segment_ordinal = i.ordinal
                  )
            )
            AND NOT EXISTS (
                SELECT 1 FROM recording_gaps AS g
                WHERE g.recording_id = r.id
                  AND g.start_us < c.end_us
                  AND g.end_us > c.start_us
            )
        )
        OR (
            NEW.original_state = 'expired'
            AND (
                r.storage_state IN ('deleting', 'deleted')
                OR EXISTS (
                    SELECT 1 FROM recording_intervals AS i
                    JOIN recording_releases AS x
                      ON x.recording_id = i.recording_id AND x.segment_ordinal = i.ordinal
                    WHERE i.recording_id = r.id
                      AND i.decoded_start_us <= c.start_us
                      AND i.decoded_end_us >= c.end_us
                )
            )
            AND NEW.start_us IS NULL
            AND NEW.end_us IS NULL
        )
        OR (
            NEW.original_state = 'missing'
            AND r.storage_state NOT IN ('deleting', 'deleted')
            AND NEW.start_us IS NULL
            AND NEW.end_us IS NULL
            AND NOT EXISTS (
                SELECT 1 FROM recording_intervals AS i
                JOIN recording_releases AS x
                  ON x.recording_id = i.recording_id AND x.segment_ordinal = i.ordinal
                WHERE i.recording_id = r.id
                  AND i.decoded_start_us <= c.start_us
                  AND i.decoded_end_us >= c.end_us
            )
            AND (
                NOT EXISTS (
                    SELECT 1 FROM recording_intervals AS i
                    WHERE i.recording_id = r.id
                      AND i.decoded_start_us <= c.start_us
                      AND i.decoded_end_us >= c.end_us
                )
                OR EXISTS (
                    SELECT 1 FROM recording_gaps AS g
                    WHERE g.recording_id = r.id
                      AND g.start_us < c.end_us
                      AND g.end_us > c.start_us
                )
            )
        )
      )
)
BEGIN
    SELECT RAISE(ABORT, 'finding citation');
END;
PRAGMA user_version = 42;
