-- Preserve old rows before rebuilding their constrained tables. Foreign keys stay on.
CREATE TEMP TABLE migration26_jobs AS SELECT * FROM analysis_jobs;
CREATE TEMP TABLE migration26_transcripts AS SELECT * FROM transcripts;
CREATE TEMP TABLE migration26_cues AS SELECT * FROM transcript_cues;
CREATE TEMP TABLE migration26_decisions AS SELECT * FROM analysis_decisions;
CREATE TEMP TABLE migration26_languages AS SELECT * FROM language_evidence;
DROP TRIGGER analysis_read_lease_delete;
DROP TRIGGER analysis_read_lease_release;
DROP TABLE language_evidence;
DROP TABLE analysis_decisions;
DROP TABLE transcript_cues;
DROP TABLE transcripts;
DROP TABLE analysis_jobs;
CREATE TABLE analysis_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 64),
    analysis_id TEXT NOT NULL,
    analysis_revision INTEGER NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    profile TEXT NOT NULL CHECK(length(profile) BETWEEN 1 AND 128),
    kind TEXT NOT NULL DEFAULT 'verify' CHECK(kind IN ('verify', 'local_asr')),
    profile_sha256 TEXT CHECK(length(profile_sha256) = 64 AND profile_sha256 NOT GLOB '*[^0-9a-f]*'),
    expected_parent_revision INTEGER CHECK(expected_parent_revision BETWEEN 0 AND 63),
    state TEXT NOT NULL CHECK(state IN ('running', 'cancelling', 'verified', 'succeeded', 'cancelled', 'failed', 'interrupted')),
    expected_bytes INTEGER NOT NULL CHECK(expected_bytes BETWEEN 1 AND 536870912),
    expected_files INTEGER NOT NULL CHECK(expected_files BETWEEN 1 AND 1024),
    manifest_sha256 TEXT NOT NULL CHECK(length(manifest_sha256) = 64 AND manifest_sha256 NOT GLOB '*[^0-9a-f]*'),
    verified_bytes INTEGER CHECK(verified_bytes = expected_bytes),
    reason TEXT CHECK(length(reason) BETWEEN 1 AND 128),
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    finished_ms INTEGER CHECK(finished_ms >= created_ms),
    FOREIGN KEY(analysis_id, analysis_revision) REFERENCES analysis_inputs(id, revision),
    CHECK((kind = 'verify' AND profile = 'retained-sha256-v1' AND profile_sha256 IS NULL AND expected_parent_revision IS NULL AND state != 'succeeded')
       OR (kind = 'local_asr' AND profile NOT IN ('retained-sha256-v1', 'local-unmeasured') AND profile_sha256 IS NOT NULL AND expected_parent_revision IS NOT NULL AND expected_files = 1 AND expected_bytes <= 67108864 AND state != 'verified')),
    CHECK((state IN ('running', 'cancelling')) = (finished_ms IS NULL)),
    CHECK((state = 'verified') = (verified_bytes IS NOT NULL)),
    CHECK((state IN ('cancelled', 'failed', 'interrupted')) = (reason IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX analysis_one_worker ON analysis_jobs((1)) WHERE state IN ('running', 'cancelling');
CREATE INDEX analysis_job_lease ON analysis_jobs(recording_id, state);
CREATE TABLE transcripts (
    id TEXT NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 64),
    analysis_id TEXT NOT NULL,
    analysis_revision INTEGER NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    media_sha256 TEXT NOT NULL CHECK(length(media_sha256) = 64 AND media_sha256 NOT GLOB '*[^0-9a-f]*'),
    role TEXT NOT NULL CHECK(role = 'original'),
    profile TEXT NOT NULL CHECK(length(profile) BETWEEN 1 AND 128),
    kind TEXT NOT NULL DEFAULT 'legacy_placeholder' CHECK(kind IN ('legacy_placeholder', 'recognition')),
    outcome TEXT NOT NULL DEFAULT 'legacy' CHECK(outcome IN ('legacy', 'text', 'no_text')),
    parent_revision INTEGER CHECK(parent_revision BETWEEN 1 AND 63),
    job_id TEXT UNIQUE REFERENCES analysis_jobs(id),
    job_generation INTEGER CHECK(job_generation BETWEEN 1 AND 64),
    profile_sha256 TEXT CHECK(length(profile_sha256) = 64 AND profile_sha256 NOT GLOB '*[^0-9a-f]*'),
    cue_count INTEGER CHECK(cue_count BETWEEN 0 AND 256),
    text_bytes INTEGER CHECK(text_bytes BETWEEN 0 AND 65536),
    state TEXT NOT NULL CHECK(state = 'published'),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (id, revision),
    FOREIGN KEY (analysis_id, analysis_revision) REFERENCES analysis_inputs(id, revision),
    FOREIGN KEY (id, parent_revision) REFERENCES transcripts(id, revision),
    CHECK((kind = 'legacy_placeholder' AND outcome = 'legacy' AND profile = 'local-unmeasured' AND revision = 1 AND parent_revision IS NULL AND job_id IS NULL AND job_generation IS NULL AND profile_sha256 IS NULL AND cue_count IS NULL AND text_bytes IS NULL)
       OR (kind = 'recognition' AND profile NOT IN ('local-unmeasured', 'retained-sha256-v1') AND job_id IS NOT NULL AND job_generation IS NOT NULL AND profile_sha256 IS NOT NULL AND cue_count IS NOT NULL AND text_bytes IS NOT NULL
           AND ((revision = 1 AND parent_revision IS NULL) OR (parent_revision IS NOT NULL AND revision = parent_revision + 1))
           AND ((outcome = 'text' AND cue_count > 0 AND text_bytes > 0) OR (outcome = 'no_text' AND cue_count = 0 AND text_bytes = 0)))),
    CHECK(id = analysis_id)
) STRICT;
CREATE TABLE transcript_cues (
    transcript_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 1000000),
    start_us INTEGER NOT NULL CHECK(start_us >= 0),
    end_us INTEGER NOT NULL CHECK(end_us > start_us),
    script TEXT NOT NULL CHECK(length(CAST(script AS BLOB)) <= 4096),
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
INSERT INTO analysis_jobs(id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, manifest_sha256, verified_bytes, reason, amount_micros, created_ms, finished_ms)
SELECT id, generation, analysis_id, analysis_revision, recording_id, profile, state, expected_bytes, expected_files, manifest_sha256, verified_bytes, reason, amount_micros, created_ms, finished_ms FROM migration26_jobs;
INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, state, created_ms)
SELECT id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, state, created_ms FROM migration26_transcripts;
INSERT INTO transcript_cues SELECT * FROM migration26_cues;
INSERT INTO analysis_decisions SELECT * FROM migration26_decisions;
INSERT INTO language_evidence SELECT * FROM migration26_languages;
DROP TABLE migration26_languages;
DROP TABLE migration26_decisions;
DROP TABLE migration26_cues;
DROP TABLE migration26_transcripts;
DROP TABLE migration26_jobs;
CREATE TRIGGER analysis_job_input
BEFORE INSERT ON analysis_jobs
WHEN NEW.state != 'running' OR NEW.generation != 1 OR NOT EXISTS (
    SELECT 1 FROM analysis_inputs a JOIN recordings r ON r.id = a.recording_id
    WHERE a.id = NEW.analysis_id AND a.revision = NEW.analysis_revision
      AND a.recording_id = NEW.recording_id AND a.state = 'published'
      AND a.revision = (SELECT max(revision) FROM analysis_inputs WHERE id = a.id)
      AND r.storage_state = 'retained'
      AND r.sha256 = a.media_sha256 AND r.media_bytes = NEW.expected_bytes
)
BEGIN SELECT RAISE(ABORT, 'invalid analysis job input'); END;
CREATE TRIGGER analysis_job_request_immutable
BEFORE UPDATE OF id, analysis_id, analysis_revision, recording_id, profile, kind, profile_sha256, expected_parent_revision, expected_bytes, expected_files, manifest_sha256, amount_micros, created_ms ON analysis_jobs
BEGIN SELECT RAISE(ABORT, 'analysis request is immutable'); END;
CREATE TRIGGER analysis_job_transition
BEFORE UPDATE OF state, generation, reason, finished_ms, verified_bytes ON analysis_jobs
WHEN NOT (
    (OLD.state = 'running' AND NEW.state = 'cancelling' AND NEW.generation = OLD.generation)
    OR (OLD.state = 'running' AND NEW.state IN ('verified', 'succeeded', 'failed', 'cancelled') AND NEW.generation = OLD.generation)
    OR (OLD.state = 'cancelling' AND NEW.state = 'cancelled' AND NEW.generation = OLD.generation)
    OR (OLD.state IN ('running', 'cancelling') AND NEW.state = 'interrupted' AND NEW.generation = OLD.generation + 1)
)
BEGIN SELECT RAISE(ABORT, 'invalid analysis job transition'); END;
CREATE TRIGGER analysis_job_no_delete
BEFORE DELETE ON analysis_jobs
BEGIN SELECT RAISE(ABORT, 'analysis job history is retained'); END;
CREATE TRIGGER analysis_read_lease_delete
BEFORE UPDATE OF storage_state ON recordings
WHEN NEW.storage_state IN ('deleting', 'deleted') AND EXISTS (
    SELECT 1 FROM analysis_jobs j WHERE j.recording_id = OLD.id AND j.state IN ('running', 'cancelling')
)
BEGIN SELECT RAISE(ABORT, 'recording has an active analysis read lease'); END;
CREATE TRIGGER analysis_read_lease_release
BEFORE INSERT ON recording_releases
WHEN EXISTS (SELECT 1 FROM analysis_jobs j WHERE j.recording_id = NEW.recording_id AND j.state IN ('running', 'cancelling'))
BEGIN SELECT RAISE(ABORT, 'recording has an active analysis read lease'); END;

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
