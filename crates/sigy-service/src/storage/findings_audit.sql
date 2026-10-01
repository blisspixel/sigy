SELECT
EXISTS (
    SELECT 1 FROM monitor_findings AS f
    WHERE NOT EXISTS (
        SELECT 1
        FROM transcripts AS t
        JOIN recordings AS r ON r.id = t.recording_id
        JOIN transcript_cues AS c
          ON c.transcript_id = t.id AND c.revision = t.revision AND c.ordinal = f.cue_ordinal
        JOIN translation_cues AS tc
          ON tc.transcript_id = t.id AND tc.transcript_revision = t.revision
         AND tc.revision = f.translation_revision AND tc.ordinal = f.cue_ordinal
        WHERE t.id = f.transcript_id
          AND t.revision = f.transcript_revision
          AND t.recording_id = f.recording_id
          AND t.role = 'original'
          AND t.outcome = 'text'
          AND t.kind IN ('recognition', 'correction')
          AND (
            (
                f.original_state = 'retained'
                -- Retention changes availability, not the publication's immutable history.
                AND r.storage_state IN ('retained', 'deleting', 'deleted')
                AND r.sha256 = t.media_sha256
                AND f.start_us = c.start_us
                AND f.end_us = c.end_us
                AND EXISTS (
                    SELECT 1 FROM recording_intervals AS i
                    WHERE i.recording_id = r.id
                      AND i.decoded_start_us <= c.start_us
                      AND i.decoded_end_us >= c.end_us
                )
                AND NOT EXISTS (
                    SELECT 1 FROM recording_gaps AS g
                    WHERE g.recording_id = r.id
                      AND g.start_us < c.end_us
                      AND g.end_us > c.start_us
                )
            )
            OR (
                f.original_state = 'expired'
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
                AND f.start_us IS NULL
                AND f.end_us IS NULL
            )
            OR (
                f.original_state = 'missing'
                AND f.start_us IS NULL
                AND f.end_us IS NULL
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
)
OR EXISTS (
    SELECT 1 FROM monitor_findings GROUP BY monitor_id HAVING count(*) > 1024
);
