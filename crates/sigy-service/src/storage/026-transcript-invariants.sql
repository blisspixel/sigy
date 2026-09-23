CREATE TABLE transcript_coverage (
    transcript_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    interval_ordinal INTEGER NOT NULL CHECK(interval_ordinal BETWEEN 0 AND 1000000),
    start_us INTEGER NOT NULL CHECK(start_us >= 0),
    end_us INTEGER NOT NULL CHECK(end_us > start_us AND end_us - start_us <= 60000000),
    source_sha256 TEXT NOT NULL CHECK(length(source_sha256) = 64 AND source_sha256 NOT GLOB '*[^0-9a-f]*'),
    decoded_sha256 TEXT NOT NULL CHECK(length(decoded_sha256) = 64 AND decoded_sha256 NOT GLOB '*[^0-9a-f]*'),
    sample_rate INTEGER NOT NULL CHECK(sample_rate BETWEEN 1 AND 384000),
    sample_count INTEGER NOT NULL CHECK(sample_count BETWEEN 1 AND 23040000),
    PRIMARY KEY(transcript_id, revision),
    FOREIGN KEY(transcript_id, revision) REFERENCES transcripts(id, revision),
    CHECK(sample_count <= sample_rate * 60)
) STRICT;
CREATE TRIGGER analysis_job_id_retained BEFORE INSERT ON analysis_jobs
WHEN EXISTS (SELECT 1 FROM analysis_jobs WHERE id = NEW.id) OR (SELECT count(*) FROM analysis_jobs) >= 256
BEGIN SELECT RAISE(ABORT, 'analysis job history is retained'); END;
CREATE TRIGGER transcript_id_retained BEFORE INSERT ON transcripts
WHEN EXISTS (SELECT 1 FROM transcripts WHERE id = NEW.id AND revision = NEW.revision)
BEGIN SELECT RAISE(ABORT, 'transcript revision is retained'); END;
CREATE TRIGGER analysis_asr_input
BEFORE INSERT ON analysis_jobs
WHEN NEW.kind = 'local_asr' AND (
    NEW.expected_parent_revision != coalesce((SELECT max(revision) FROM transcripts WHERE id = NEW.analysis_id), 0)
    OR NOT EXISTS (
        SELECT 1 FROM analysis_inputs a, json_each(a.timeline_json, '$.intervals') s
        WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
          AND json_array_length(a.timeline_json, '$.intervals') = 1
          AND json_extract(s.value, '$.end_us') - json_extract(s.value, '$.start_us') BETWEEN 1 AND 60000000
    )
)
BEGIN SELECT RAISE(ABORT, 'invalid recognition request'); END;
CREATE TRIGGER transcript_recognition_input
BEFORE INSERT ON transcripts
WHEN NEW.kind = 'recognition' AND (
    NEW.revision != coalesce((SELECT max(revision) + 1 FROM transcripts WHERE id = NEW.id), 1)
    OR NOT EXISTS (
        SELECT 1 FROM analysis_jobs j JOIN analysis_inputs a ON a.id = j.analysis_id AND a.revision = j.analysis_revision
        JOIN recordings r ON r.id = j.recording_id
        WHERE j.id = NEW.job_id AND j.kind = 'local_asr' AND j.state = 'running'
          AND j.generation = NEW.job_generation AND j.profile = NEW.profile AND j.profile_sha256 = NEW.profile_sha256
          AND j.expected_parent_revision = coalesce(NEW.parent_revision, 0)
          AND j.analysis_id = NEW.analysis_id AND j.analysis_revision = NEW.analysis_revision
          AND j.recording_id = NEW.recording_id AND a.media_sha256 = NEW.media_sha256
          AND a.state = 'published' AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id)
          AND r.storage_state = 'retained' AND r.sha256 = a.media_sha256 AND r.media_bytes = j.expected_bytes
          AND NEW.created_ms >= j.created_ms
    )
)
BEGIN SELECT RAISE(ABORT, 'recognition result identity conflicts'); END;
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
                )))
    )
BEGIN SELECT RAISE(ABORT, 'invalid or sealed transcript cue'); END;
CREATE TRIGGER transcript_coverage_insert
BEFORE INSERT ON transcript_coverage
WHEN EXISTS (SELECT 1 FROM analysis_decisions WHERE transcript_id = NEW.transcript_id AND transcript_revision = NEW.revision)
    OR EXISTS (SELECT 1 FROM transcript_coverage WHERE transcript_id = NEW.transcript_id AND revision = NEW.revision)
    OR NOT EXISTS (
        SELECT 1 FROM transcripts t JOIN analysis_inputs a ON a.id = t.analysis_id AND a.revision = t.analysis_revision,
             json_each(a.timeline_json, '$.intervals') s
        WHERE t.id = NEW.transcript_id AND t.revision = NEW.revision AND t.kind = 'recognition'
          AND NEW.interval_ordinal = json_extract(s.value, '$.ordinal')
          AND NEW.start_us = json_extract(s.value, '$.start_us') AND NEW.end_us = json_extract(s.value, '$.end_us')
          AND NEW.source_sha256 = json_extract(s.value, '$.sha256')
    )
BEGIN SELECT RAISE(ABORT, 'invalid or sealed transcript coverage'); END;
CREATE TRIGGER transcript_decision_complete
BEFORE INSERT ON analysis_decisions
WHEN EXISTS (SELECT 1 FROM analysis_decisions WHERE transcript_id = NEW.transcript_id AND transcript_revision = NEW.transcript_revision)
    OR NOT EXISTS (
    SELECT 1 FROM transcripts t WHERE t.id = NEW.transcript_id AND t.revision = NEW.transcript_revision
      AND ((t.kind = 'legacy_placeholder' AND EXISTS (SELECT 1 FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision))
        OR (t.kind = 'recognition' AND NEW.created_ms = t.created_ms
            AND t.cue_count = (SELECT count(*) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision)
            AND t.text_bytes = coalesce((SELECT sum(length(CAST(script AS BLOB))) FROM transcript_cues WHERE transcript_id = t.id AND revision = t.revision), 0)
            AND EXISTS (SELECT 1 FROM transcript_coverage WHERE transcript_id = t.id AND revision = t.revision)
            AND EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.id = t.job_id AND j.kind = 'local_asr' AND j.state = 'running' AND j.generation = t.job_generation)))
)
BEGIN SELECT RAISE(ABORT, 'transcript result is incomplete'); END;
CREATE TRIGGER analysis_asr_terminal
BEFORE UPDATE OF state ON analysis_jobs
WHEN NEW.kind = 'local_asr' AND (
    (NEW.state = 'succeeded' AND NOT EXISTS (
        SELECT 1 FROM transcripts t JOIN analysis_decisions d ON d.transcript_id = t.id AND d.transcript_revision = t.revision
        JOIN analysis_inputs a ON a.id = t.analysis_id AND a.revision = t.analysis_revision
        JOIN recordings r ON r.id = t.recording_id
        WHERE t.job_id = NEW.id AND t.job_generation = NEW.generation AND t.kind = 'recognition'
          AND a.state = 'published' AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id)
          AND r.storage_state = 'retained' AND r.sha256 = a.media_sha256 AND r.media_bytes = NEW.expected_bytes
          AND NEW.finished_ms >= t.created_ms
    ))
    OR (NEW.state != 'succeeded' AND EXISTS (SELECT 1 FROM transcripts WHERE job_id = NEW.id))
)
BEGIN SELECT RAISE(ABORT, 'recognition completion conflicts'); END;
CREATE TRIGGER transcript_coverage_no_update BEFORE UPDATE ON transcript_coverage
BEGIN SELECT RAISE(ABORT, 'transcript coverage is immutable'); END;
CREATE TRIGGER transcript_coverage_no_delete BEFORE DELETE ON transcript_coverage
BEGIN SELECT RAISE(ABORT, 'transcript coverage is retained'); END;
PRAGMA user_version = 26;
