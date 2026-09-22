CREATE TABLE transcripts (
    id TEXT NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 64),
    analysis_id TEXT NOT NULL,
    analysis_revision INTEGER NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    media_sha256 TEXT NOT NULL CHECK(length(media_sha256) = 64 AND media_sha256 NOT GLOB '*[^0-9a-f]*'),
    role TEXT NOT NULL CHECK(role = 'original'),
    profile TEXT NOT NULL CHECK(profile = 'local-unmeasured'),
    state TEXT NOT NULL CHECK(state = 'published'),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (id, revision),
    FOREIGN KEY (analysis_id, analysis_revision) REFERENCES analysis_inputs(id, revision),
    UNIQUE (analysis_id, analysis_revision),
    CHECK(id = analysis_id)
) STRICT;
CREATE TABLE transcript_cues (
    transcript_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 1000000),
    start_us INTEGER NOT NULL CHECK(start_us >= 0),
    end_us INTEGER NOT NULL CHECK(end_us > start_us),
    script TEXT NOT NULL CHECK(script = ''),
    wording TEXT NOT NULL CHECK(wording = 'uncertain'),
    PRIMARY KEY (transcript_id, revision, ordinal),
    FOREIGN KEY (transcript_id, revision) REFERENCES transcripts(id, revision)
) STRICT;
CREATE TABLE analysis_decisions (
    transcript_id TEXT NOT NULL,
    transcript_revision INTEGER NOT NULL,
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    request_id TEXT CHECK(request_id IS NULL),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (transcript_id, transcript_revision),
    FOREIGN KEY (transcript_id, transcript_revision) REFERENCES transcripts(id, revision)
) STRICT;
CREATE TRIGGER transcripts_no_update
BEFORE UPDATE ON transcripts
BEGIN
    SELECT RAISE(ABORT, 'transcript revision is immutable');
END;
CREATE TRIGGER transcripts_no_delete
BEFORE DELETE ON transcripts
BEGIN
    SELECT RAISE(ABORT, 'transcript revision is retained');
END;
CREATE TRIGGER transcript_cues_no_update
BEFORE UPDATE ON transcript_cues
BEGIN
    SELECT RAISE(ABORT, 'transcript revision is immutable');
END;
CREATE TRIGGER transcript_cues_no_delete
BEFORE DELETE ON transcript_cues
BEGIN
    SELECT RAISE(ABORT, 'transcript revision is retained');
END;
CREATE TRIGGER analysis_decisions_no_update
BEFORE UPDATE ON analysis_decisions
BEGIN
    SELECT RAISE(ABORT, 'analysis decision is immutable');
END;
CREATE TRIGGER analysis_decisions_no_delete
BEFORE DELETE ON analysis_decisions
BEGIN
    SELECT RAISE(ABORT, 'analysis decision is retained');
END;
PRAGMA user_version = 23;
