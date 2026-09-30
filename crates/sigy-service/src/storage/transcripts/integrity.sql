SELECT
EXISTS (
    SELECT 1 FROM transcripts t WHERE t.id != t.analysis_id OR t.role != 'original' OR t.state != 'published'
    OR NOT EXISTS (SELECT 1 FROM analysis_inputs a WHERE a.id = t.analysis_id AND a.revision = t.analysis_revision AND a.state = 'published' AND a.recording_id = t.recording_id AND a.media_sha256 = t.media_sha256)
    OR NOT EXISTS (SELECT 1 FROM analysis_decisions d WHERE d.transcript_id = t.id AND d.transcript_revision = t.revision)
)
OR EXISTS (SELECT 1 FROM transcripts GROUP BY id HAVING min(revision) != 1 OR max(revision) != count(*))
OR EXISTS (SELECT 1 FROM analysis_decisions WHERE amount_micros != 0 OR request_id IS NOT NULL)
OR EXISTS (
    SELECT 1 FROM transcripts t WHERE t.kind = 'legacy_placeholder' AND (
        t.profile != 'local-unmeasured' OR t.revision != 1
        OR NOT EXISTS (SELECT 1 FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision)
        OR EXISTS (SELECT 1 FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision AND script != '')
        OR EXISTS (SELECT 1 FROM transcript_coverage WHERE transcript_id = t.id AND revision = t.revision)
    )
)
OR EXISTS (
    SELECT 1 FROM transcripts t WHERE t.kind = 'recognition' AND (
        t.cue_count != (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision)
        OR t.text_bytes != coalesce((SELECT sum(length(CAST(script AS BLOB))) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision), 0)
        OR NOT EXISTS (SELECT 1 FROM transcript_coverage WHERE transcript_id = t.id AND revision = t.revision)
        OR NOT EXISTS (
            SELECT 1 FROM analysis_jobs j JOIN analysis_decisions d ON d.transcript_id = t.id AND d.transcript_revision = t.revision
            WHERE j.id = t.job_id AND j.kind = 'local_asr' AND j.state = 'succeeded' AND j.generation = t.job_generation
              AND j.profile = t.profile AND j.profile_sha256 = t.profile_sha256
              AND j.analysis_id = t.analysis_id AND j.analysis_revision = t.analysis_revision AND j.recording_id = t.recording_id
              AND j.expected_parent_revision = coalesce(t.parent_revision, 0)
              AND t.created_ms >= j.created_ms AND j.finished_ms >= t.created_ms AND d.created_ms = t.created_ms
        )
    )
)
OR EXISTS (
    SELECT 1 FROM analysis_jobs j WHERE j.kind = 'local_asr'
    AND ((j.state = 'succeeded') != EXISTS (SELECT 1 FROM transcripts WHERE job_id = j.id))
)
OR EXISTS (
    SELECT 1 FROM transcript_cues c JOIN transcripts t ON t.id = c.transcript_id AND t.revision = c.revision
    WHERE c.wording != 'uncertain' OR (t.kind = 'recognition' AND (
        length(CAST(c.script AS BLOB)) NOT BETWEEN 1 AND 4096 OR c.ordinal >= t.cue_count
        OR NOT EXISTS (
            SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.intervals') s
            WHERE a.id = t.analysis_id AND a.revision = t.analysis_revision
              AND c.start_us >= json_extract(s.value, '$.start_us') AND c.end_us <= json_extract(s.value, '$.end_us')
        )
        OR EXISTS (SELECT 1 FROM transcript_cues p WHERE p.transcript_id = c.transcript_id AND p.revision = c.revision AND p.ordinal < c.ordinal AND p.end_us > c.start_us)
        OR EXISTS (
            SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.gaps') g
            WHERE a.id = t.analysis_id AND a.revision = t.analysis_revision
              AND c.start_us < json_extract(g.value, '$.end_us') AND c.end_us > json_extract(g.value, '$.start_us')
        )
        OR NOT EXISTS (
            SELECT 1 FROM transcript_coverage v
            WHERE v.transcript_id = c.transcript_id AND v.revision = c.revision
              AND c.start_us >= v.start_us AND c.end_us <= v.end_us
        )
    ))
)
OR EXISTS (
    SELECT 1 FROM transcript_coverage c JOIN transcripts t ON t.id = c.transcript_id AND t.revision = c.revision
    WHERE t.kind != 'recognition'
       OR NOT EXISTS (
            SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.intervals') s
            WHERE a.id = t.analysis_id AND a.revision = t.analysis_revision
              AND c.interval_ordinal = json_extract(s.value, '$.ordinal')
              AND c.source_sha256 = json_extract(s.value, '$.sha256')
              AND c.start_us >= json_extract(s.value, '$.start_us')
              AND c.end_us <= json_extract(s.value, '$.end_us')
       )
       OR EXISTS (
            SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.gaps') g
            WHERE a.id = t.analysis_id AND a.revision = t.analysis_revision
              AND c.start_us < json_extract(g.value, '$.end_us') AND c.end_us > json_extract(g.value, '$.start_us')
       )
)
OR EXISTS (
    SELECT 1 FROM transcripts t WHERE t.kind = 'recognition' AND (
        (SELECT count(*) FROM transcript_coverage c WHERE c.transcript_id = t.id AND c.revision = t.revision) NOT BETWEEN 1 AND 1024
        OR (SELECT min(ordinal) FROM transcript_coverage c WHERE c.transcript_id = t.id AND c.revision = t.revision) != 0
        OR (SELECT max(ordinal) FROM transcript_coverage c WHERE c.transcript_id = t.id AND c.revision = t.revision)
           != (SELECT count(*) - 1 FROM transcript_coverage c WHERE c.transcript_id = t.id AND c.revision = t.revision)
        OR EXISTS (
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
        OR EXISTS (
            SELECT 1 FROM transcript_coverage c
            WHERE c.transcript_id = t.id AND c.revision = t.revision AND c.ordinal > 0
              AND EXISTS (
                SELECT 1 FROM transcript_coverage p
                WHERE p.transcript_id = c.transcript_id AND p.revision = c.revision
                  AND p.ordinal = c.ordinal - 1 AND p.interval_ordinal = c.interval_ordinal
                  AND p.end_us != c.start_us
              )
        )
    )
)
OR EXISTS (
    SELECT 1 FROM transcripts WHERE kind NOT IN ('legacy_placeholder', 'recognition', 'correction')
)
OR EXISTS (
    SELECT 1 FROM transcripts t WHERE t.kind = 'correction' AND (
        t.profile != 'user-correction-v1' OR t.outcome != 'text'
        OR t.parent_revision IS NULL OR t.revision != t.parent_revision + 1
        OR t.job_id IS NOT NULL OR t.job_generation IS NOT NULL OR t.profile_sha256 IS NOT NULL
        OR EXISTS (SELECT 1 FROM transcript_coverage WHERE transcript_id = t.id AND revision = t.revision)
        OR t.cue_count != (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision)
        OR t.text_bytes != coalesce((SELECT sum(length(CAST(script AS BLOB))) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision), 0)
        OR t.cue_count != (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.parent_revision)
        OR NOT EXISTS (
            SELECT 1 FROM transcripts p
            WHERE p.id = t.id AND p.revision = t.parent_revision AND p.outcome = 'text'
              AND p.analysis_id = t.analysis_id AND p.analysis_revision = t.analysis_revision
              AND p.recording_id = t.recording_id AND p.media_sha256 = t.media_sha256
        )
        OR EXISTS (
            SELECT 1 FROM transcript_cues n
            WHERE n.transcript_id = t.id AND n.revision = t.revision
              AND (
                  length(CAST(n.script AS BLOB)) NOT BETWEEN 1 AND 4096
                  OR NOT EXISTS (
                      SELECT 1 FROM transcript_cues p
                      WHERE p.transcript_id = n.transcript_id AND p.revision = t.parent_revision
                        AND p.ordinal = n.ordinal AND p.start_us = n.start_us AND p.end_us = n.end_us
                  )
              )
        )
    )
)
