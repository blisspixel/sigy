CREATE TABLE language_evidence (
    id TEXT NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 64),
    analysis_id TEXT NOT NULL,
    analysis_revision INTEGER NOT NULL,
    transcript_id TEXT,
    transcript_revision INTEGER,
    payload_json TEXT NOT NULL CHECK(length(CAST(payload_json AS BLOB)) BETWEEN 2 AND 65536 AND json_valid(payload_json)),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (id, revision),
    FOREIGN KEY (analysis_id, analysis_revision) REFERENCES analysis_inputs(id, revision),
    FOREIGN KEY (transcript_id, transcript_revision) REFERENCES transcripts(id, revision),
    CHECK((transcript_id IS NULL) = (transcript_revision IS NULL)),
    CHECK(json_extract(payload_json, '$.id') IS id),
    CHECK(json_extract(payload_json, '$.revision') IS revision),
    CHECK(json_extract(payload_json, '$.analysis_id') IS analysis_id),
    CHECK(json_extract(payload_json, '$.analysis_revision') IS analysis_revision),
    CHECK(json_extract(payload_json, '$.transcript.id') IS transcript_id),
    CHECK(json_extract(payload_json, '$.transcript.revision') IS transcript_revision),
    CHECK(json_type(payload_json, '$.spans') IS 'array'),
    CHECK(json_array_length(payload_json, '$.spans') <= 1024)
) STRICT;
CREATE INDEX language_evidence_input ON language_evidence(analysis_id, analysis_revision, id, revision);
CREATE TRIGGER language_evidence_input_current
BEFORE INSERT ON language_evidence
WHEN NOT EXISTS (
    SELECT 1 FROM analysis_inputs a
    WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
      AND a.state = 'published'
      AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id)
)
BEGIN
    SELECT RAISE(ABORT, 'language input revision is stale or unpublished');
END;
CREATE TRIGGER language_evidence_transcript_matches
BEFORE INSERT ON language_evidence
WHEN NEW.transcript_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM transcripts t
    WHERE t.id = NEW.transcript_id AND t.revision = NEW.transcript_revision
      AND t.analysis_id = NEW.analysis_id AND t.analysis_revision = NEW.analysis_revision
)
BEGIN
    SELECT RAISE(ABORT, 'language transcript revision does not match input');
END;
CREATE TRIGGER language_evidence_revision_order
BEFORE INSERT ON language_evidence
WHEN NEW.revision != coalesce((SELECT max(revision) + 1 FROM language_evidence WHERE id = NEW.id), 1)
    OR EXISTS (SELECT 1 FROM language_evidence WHERE id = NEW.id AND analysis_id != NEW.analysis_id)
BEGIN
    SELECT RAISE(ABORT, 'language evidence revision conflicts');
END;
CREATE TRIGGER language_evidence_no_update
BEFORE UPDATE ON language_evidence
BEGIN
    SELECT RAISE(ABORT, 'language evidence revision is immutable');
END;
CREATE TRIGGER language_evidence_no_delete
BEFORE DELETE ON language_evidence
BEGIN
    SELECT RAISE(ABORT, 'language evidence revision is retained');
END;
PRAGMA user_version = 24;
