-- One stored citation. A model proposal cannot insert a row. The row is immutable.
CREATE TABLE IF NOT EXISTS monitor_findings (
    monitor_id TEXT NOT NULL REFERENCES monitors(id),
    id TEXT NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    transcript_id TEXT NOT NULL,
    transcript_revision INTEGER NOT NULL CHECK(transcript_revision BETWEEN 1 AND 64),
    translation_revision INTEGER NOT NULL CHECK(translation_revision BETWEEN 1 AND 64),
    cue_ordinal INTEGER NOT NULL CHECK(cue_ordinal BETWEEN 0 AND 255),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    original_state TEXT NOT NULL CHECK(original_state IN ('retained', 'expired', 'missing')),
    start_us INTEGER,
    end_us INTEGER,
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (monitor_id, id),
    FOREIGN KEY (transcript_id, transcript_revision) REFERENCES transcripts(id, revision),
    FOREIGN KEY (transcript_id, transcript_revision, translation_revision)
        REFERENCES translations(transcript_id, transcript_revision, revision),
    FOREIGN KEY (transcript_id, transcript_revision, cue_ordinal)
        REFERENCES transcript_cues(transcript_id, revision, ordinal),
    CHECK(
        (original_state = 'retained' AND start_us IS NOT NULL AND end_us IS NOT NULL AND end_us > start_us)
        OR (original_state IN ('expired', 'missing') AND start_us IS NULL AND end_us IS NULL)
    )
) STRICT;

DROP TRIGGER IF EXISTS monitor_finding_limit;
CREATE TRIGGER monitor_finding_limit
BEFORE INSERT ON monitor_findings
WHEN (SELECT count(*) FROM monitor_findings WHERE monitor_id = NEW.monitor_id) >= 1024
BEGIN
    SELECT RAISE(ABORT, 'finding limit');
END;

DROP TRIGGER IF EXISTS monitor_finding_no_update;
CREATE TRIGGER monitor_finding_no_update
BEFORE UPDATE ON monitor_findings
BEGIN
    SELECT RAISE(ABORT, 'findings are immutable');
END;

DROP TRIGGER IF EXISTS monitor_finding_no_delete;
CREATE TRIGGER monitor_finding_no_delete
BEFORE DELETE ON monitor_findings
BEGIN
    SELECT RAISE(ABORT, 'findings are retained');
END;

-- The citation has to be true at insert: one original text revision, one translation
-- cue, and either the cue's own retained interval or an explicit expired or missing
-- statement with no interval.
DROP TRIGGER IF EXISTS monitor_finding_citation;
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
            AND r.storage_state IN ('deleting', 'deleted')
            AND NEW.start_us IS NULL
            AND NEW.end_us IS NULL
        )
        OR (
            NEW.original_state = 'missing'
            AND r.storage_state NOT IN ('deleting', 'deleted')
            AND NEW.start_us IS NULL
            AND NEW.end_us IS NULL
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

PRAGMA user_version = 37;
