-- A sighting may cite one stored finding or one retained cue. Old finding rows stay.
-- Foreign keys stay on. Evidence and status are copied around the sighting rebuild.

CREATE TEMP TABLE migration54_sightings AS
SELECT claim_id, id, monitor_id, finding_id, created_ms FROM claim_sightings;
CREATE TEMP TABLE migration54_evidence AS
SELECT claim_id, id, sighting_id, kind, origin, note, relation, created_ms FROM claim_evidence;
CREATE TEMP TABLE migration54_status AS
SELECT claim_id, revision, status, evidence_id, setter, created_ms FROM claim_status;

DROP TABLE claim_status;
DROP TABLE claim_evidence;
DROP TABLE claim_sightings;

CREATE TABLE claim_sightings (
    claim_id TEXT NOT NULL REFERENCES claims(id),
    id TEXT NOT NULL CHECK(
        length(id) BETWEEN 1 AND 128 AND id NOT GLOB '*[^A-Za-z0-9_.:-]*'
    ),
    monitor_id TEXT,
    finding_id TEXT,
    transcript_id TEXT,
    transcript_revision INTEGER,
    translation_revision INTEGER,
    cue_ordinal INTEGER,
    recording_id TEXT,
    original_state TEXT,
    start_us INTEGER,
    end_us INTEGER,
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (claim_id, id),
    FOREIGN KEY (monitor_id, finding_id) REFERENCES monitor_findings(monitor_id, id),
    FOREIGN KEY (transcript_id, transcript_revision, cue_ordinal)
        REFERENCES transcript_cues(transcript_id, revision, ordinal),
    FOREIGN KEY (transcript_id, transcript_revision, translation_revision)
        REFERENCES translations(transcript_id, transcript_revision, revision),
    FOREIGN KEY (recording_id) REFERENCES recordings(id),
    CHECK (
        (
            monitor_id IS NOT NULL AND finding_id IS NOT NULL
            AND transcript_id IS NULL AND transcript_revision IS NULL
            AND translation_revision IS NULL AND cue_ordinal IS NULL
            AND recording_id IS NULL AND original_state IS NULL
            AND start_us IS NULL AND end_us IS NULL
            AND length(monitor_id) BETWEEN 1 AND 128
            AND monitor_id NOT GLOB '*[^A-Za-z0-9_.:-]*'
            AND length(finding_id) BETWEEN 1 AND 128
            AND finding_id NOT GLOB '*[^A-Za-z0-9_.:-]*'
        )
        OR (
            monitor_id IS NULL AND finding_id IS NULL
            AND transcript_id IS NOT NULL
            AND transcript_revision BETWEEN 1 AND 64
            AND translation_revision BETWEEN 1 AND 64
            AND cue_ordinal BETWEEN 0 AND 255
            AND recording_id IS NOT NULL
            AND original_state = 'retained'
            AND start_us IS NOT NULL AND end_us IS NOT NULL AND end_us > start_us
            AND length(transcript_id) BETWEEN 1 AND 128
            AND transcript_id NOT GLOB '*[^A-Za-z0-9_.:-]*'
        )
    )
) STRICT;

INSERT INTO claim_sightings(
    claim_id, id, monitor_id, finding_id, transcript_id, transcript_revision,
    translation_revision, cue_ordinal, recording_id, original_state, start_us, end_us, created_ms
)
SELECT
    claim_id, id, monitor_id, finding_id, NULL, NULL,
    NULL, NULL, NULL, NULL, NULL, NULL, created_ms
FROM migration54_sightings;

CREATE TABLE claim_evidence (
    claim_id TEXT NOT NULL REFERENCES claims(id),
    id TEXT NOT NULL CHECK(
        length(id) BETWEEN 1 AND 128 AND id NOT GLOB '*[^A-Za-z0-9_.:-]*'
    ),
    sighting_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('supports', 'disputes', 'context', 'unresolved')),
    origin TEXT NOT NULL CHECK(origin = 'user'),
    note TEXT NOT NULL CHECK(length(note) BETWEEN 1 AND 500),
    relation TEXT NOT NULL CHECK(relation IN (
        'unspecified', 'quotation', 'negation', 'sarcasm', 'changed-date',
        'translation', 'shared-press-release', 'correction'
    )),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (claim_id, id),
    FOREIGN KEY (claim_id, sighting_id) REFERENCES claim_sightings(claim_id, id)
) STRICT;

INSERT INTO claim_evidence(
    claim_id, id, sighting_id, kind, origin, note, relation, created_ms
)
SELECT claim_id, id, sighting_id, kind, origin, note, relation, created_ms
FROM migration54_evidence;

CREATE TABLE claim_status (
    claim_id TEXT NOT NULL REFERENCES claims(id),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 32),
    status TEXT NOT NULL CHECK(status IN ('disputed', 'corroborated', 'retracted')),
    evidence_id TEXT NOT NULL,
    setter TEXT NOT NULL CHECK(setter = 'user'),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (claim_id, revision),
    FOREIGN KEY (claim_id, evidence_id) REFERENCES claim_evidence(claim_id, id)
) STRICT;

INSERT INTO claim_status(
    claim_id, revision, status, evidence_id, setter, created_ms
)
SELECT claim_id, revision, status, evidence_id, setter, created_ms
FROM migration54_status;

DROP TRIGGER IF EXISTS claim_sighting_limit;
CREATE TRIGGER claim_sighting_limit
BEFORE INSERT ON claim_sightings
WHEN (SELECT count(*) FROM claim_sightings WHERE claim_id = NEW.claim_id) >= 32
BEGIN
    SELECT RAISE(ABORT, 'claim sighting limit');
END;

DROP TRIGGER IF EXISTS claim_evidence_limit;
CREATE TRIGGER claim_evidence_limit
BEFORE INSERT ON claim_evidence
WHEN (SELECT count(*) FROM claim_evidence WHERE claim_id = NEW.claim_id) >= 64
BEGIN
    SELECT RAISE(ABORT, 'claim evidence limit');
END;

DROP TRIGGER IF EXISTS claim_status_order;
CREATE TRIGGER claim_status_order
BEFORE INSERT ON claim_status
WHEN NEW.revision != COALESCE(
    (SELECT max(revision) FROM claim_status WHERE claim_id = NEW.claim_id), 0
) + 1
BEGIN
    SELECT RAISE(ABORT, 'claim status order');
END;

DROP TRIGGER IF EXISTS claim_status_limit;
CREATE TRIGGER claim_status_limit
BEFORE INSERT ON claim_status
WHEN (SELECT count(*) FROM claim_status WHERE claim_id = NEW.claim_id) >= 32
BEGIN
    SELECT RAISE(ABORT, 'claim status limit');
END;

DROP TRIGGER IF EXISTS claim_sightings_no_update;
CREATE TRIGGER claim_sightings_no_update
BEFORE UPDATE ON claim_sightings
BEGIN
    SELECT RAISE(ABORT, 'claim sightings are immutable');
END;

DROP TRIGGER IF EXISTS claim_sightings_no_delete;
CREATE TRIGGER claim_sightings_no_delete
BEFORE DELETE ON claim_sightings
BEGIN
    SELECT RAISE(ABORT, 'claim sightings are retained');
END;

DROP TRIGGER IF EXISTS claim_evidence_no_update;
CREATE TRIGGER claim_evidence_no_update
BEFORE UPDATE ON claim_evidence
BEGIN
    SELECT RAISE(ABORT, 'claim evidence is immutable');
END;

DROP TRIGGER IF EXISTS claim_evidence_no_delete;
CREATE TRIGGER claim_evidence_no_delete
BEFORE DELETE ON claim_evidence
BEGIN
    SELECT RAISE(ABORT, 'claim evidence is retained');
END;

DROP TRIGGER IF EXISTS claim_status_no_update;
CREATE TRIGGER claim_status_no_update
BEFORE UPDATE ON claim_status
BEGIN
    SELECT RAISE(ABORT, 'claim status is immutable');
END;

DROP TRIGGER IF EXISTS claim_status_no_delete;
CREATE TRIGGER claim_status_no_delete
BEFORE DELETE ON claim_status
BEGIN
    SELECT RAISE(ABORT, 'claim status is retained');
END;

-- The retained branch matches the cue classifier: checksum, one unreleased covering
-- interval, and no overlapping gap. Expired and missing cues stay on findings.
DROP TRIGGER IF EXISTS claim_passage_citation;
CREATE TRIGGER claim_passage_citation
BEFORE INSERT ON claim_sightings
WHEN NEW.transcript_id IS NOT NULL
 AND NOT EXISTS (
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
      AND NEW.original_state = 'retained'
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
          SELECT 1 FROM recording_intervals AS i
          JOIN recording_releases AS rel
            ON rel.recording_id = i.recording_id AND rel.segment_ordinal = i.ordinal
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
BEGIN
    SELECT RAISE(ABORT, 'claim passage citation');
END;

DROP TABLE migration54_sightings;
DROP TABLE migration54_evidence;
DROP TABLE migration54_status;

PRAGMA user_version = 54;
