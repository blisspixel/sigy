-- Local English translation of one exact transcript revision. Immutable history.
CREATE TABLE translation_profiles (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    engine TEXT NOT NULL CHECK(engine = 'llama-cpp-completion-v1'),
    template TEXT NOT NULL CHECK(template IN ('hy-mt2-plain-v1')),
    runtime_dir TEXT NOT NULL CHECK(length(CAST(runtime_dir AS BLOB)) BETWEEN 1 AND 1024),
    executable TEXT NOT NULL CHECK(length(CAST(executable AS BLOB)) BETWEEN 1 AND 128),
    runtime_sha256 TEXT NOT NULL CHECK(length(runtime_sha256) = 64 AND runtime_sha256 NOT GLOB '*[^0-9a-f]*'),
    runtime_files INTEGER NOT NULL CHECK(runtime_files BETWEEN 1 AND 256),
    runtime_bytes INTEGER NOT NULL CHECK(runtime_bytes BETWEEN 1 AND 1073741824),
    model_path TEXT NOT NULL CHECK(length(CAST(model_path AS BLOB)) BETWEEN 1 AND 1024),
    model_sha256 TEXT NOT NULL CHECK(length(model_sha256) = 64 AND model_sha256 NOT GLOB '*[^0-9a-f]*'),
    model_bytes INTEGER NOT NULL CHECK(model_bytes BETWEEN 1 AND 17179869184),
    languages TEXT NOT NULL CHECK(length(languages) BETWEEN 2 AND 512),
    threads INTEGER NOT NULL CHECK(threads BETWEEN 1 AND 64),
    memory_bytes INTEGER NOT NULL CHECK(memory_bytes BETWEEN 268435456 AND 68719476736),
    cue_deadline_ms INTEGER NOT NULL CHECK(cue_deadline_ms BETWEEN 1000 AND 600000),
    profile_sha256 TEXT NOT NULL UNIQUE CHECK(length(profile_sha256) = 64 AND profile_sha256 NOT GLOB '*[^0-9a-f]*'),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;
CREATE TRIGGER translation_profile_limit BEFORE INSERT ON translation_profiles
WHEN (SELECT count(*) FROM translation_profiles) >= 64
BEGIN SELECT RAISE(ABORT, 'translation profile limit'); END;
CREATE TRIGGER translation_profile_no_update BEFORE UPDATE ON translation_profiles
BEGIN SELECT RAISE(ABORT, 'translation profiles are immutable'); END;
CREATE TRIGGER translation_profile_no_delete BEFORE DELETE ON translation_profiles
BEGIN SELECT RAISE(ABORT, 'translation profiles are retained'); END;

CREATE TABLE translation_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 64),
    transcript_id TEXT NOT NULL,
    transcript_revision INTEGER NOT NULL,
    profile TEXT NOT NULL REFERENCES translation_profiles(id),
    profile_sha256 TEXT NOT NULL CHECK(length(profile_sha256) = 64 AND profile_sha256 NOT GLOB '*[^0-9a-f]*'),
    state TEXT NOT NULL CHECK(state IN ('running', 'cancelling', 'succeeded', 'cancelled', 'failed', 'interrupted')),
    reason TEXT CHECK(length(reason) BETWEEN 1 AND 128),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    finished_ms INTEGER CHECK(finished_ms >= created_ms),
    FOREIGN KEY (transcript_id, transcript_revision) REFERENCES transcripts(id, revision),
    CHECK((state IN ('running', 'cancelling')) = (finished_ms IS NULL)),
    CHECK((state IN ('cancelled', 'failed', 'interrupted')) = (reason IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX translation_one_worker ON translation_jobs((1)) WHERE state IN ('running', 'cancelling');
CREATE TRIGGER translation_job_limit BEFORE INSERT ON translation_jobs
WHEN (SELECT count(*) FROM translation_jobs) >= 256 OR NEW.state != 'running' OR NEW.generation != 1
BEGIN SELECT RAISE(ABORT, 'translation job admission'); END;
CREATE TRIGGER translation_job_no_delete BEFORE DELETE ON translation_jobs
BEGIN SELECT RAISE(ABORT, 'translation job history is retained'); END;

CREATE TABLE translations (
    transcript_id TEXT NOT NULL,
    transcript_revision INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 64),
    job_id TEXT NOT NULL UNIQUE REFERENCES translation_jobs(id),
    job_generation INTEGER NOT NULL CHECK(job_generation BETWEEN 1 AND 64),
    profile TEXT NOT NULL,
    profile_sha256 TEXT NOT NULL CHECK(length(profile_sha256) = 64 AND profile_sha256 NOT GLOB '*[^0-9a-f]*'),
    target TEXT NOT NULL CHECK(target = 'en'),
    cue_count INTEGER NOT NULL CHECK(cue_count BETWEEN 1 AND 256),
    translated_count INTEGER NOT NULL CHECK(translated_count BETWEEN 0 AND cue_count),
    amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (transcript_id, transcript_revision, revision),
    FOREIGN KEY (transcript_id, transcript_revision) REFERENCES transcripts(id, revision)
) STRICT;
CREATE TABLE translation_cues (
    transcript_id TEXT NOT NULL,
    transcript_revision INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 255),
    state TEXT NOT NULL CHECK(state IN ('translated', 'untranslated')),
    english TEXT CHECK(length(CAST(english AS BLOB)) BETWEEN 1 AND 4096),
    reason TEXT CHECK(length(reason) BETWEEN 1 AND 128),
    PRIMARY KEY (transcript_id, transcript_revision, revision, ordinal),
    FOREIGN KEY (transcript_id, transcript_revision, revision) REFERENCES translations(transcript_id, transcript_revision, revision),
    FOREIGN KEY (transcript_id, transcript_revision, ordinal) REFERENCES transcript_cues(transcript_id, revision, ordinal),
    CHECK((state = 'translated' AND english IS NOT NULL AND reason IS NULL) OR (state = 'untranslated' AND english IS NULL AND reason IS NOT NULL))
) STRICT;
CREATE TRIGGER translation_no_update BEFORE UPDATE ON translations
BEGIN SELECT RAISE(ABORT, 'translations are immutable'); END;
CREATE TRIGGER translation_no_delete BEFORE DELETE ON translations
BEGIN SELECT RAISE(ABORT, 'translations are retained'); END;
CREATE TRIGGER translation_cue_no_update BEFORE UPDATE ON translation_cues
BEGIN SELECT RAISE(ABORT, 'translation cues are immutable'); END;
CREATE TRIGGER translation_cue_no_delete BEFORE DELETE ON translation_cues
BEGIN SELECT RAISE(ABORT, 'translation cues are retained'); END;
-- A translation is published only by its running job, in one transaction with its cues.
CREATE TRIGGER translation_publish BEFORE INSERT ON translations
WHEN NOT EXISTS (
    SELECT 1 FROM translation_jobs j WHERE j.id = NEW.job_id AND j.generation = NEW.job_generation
      AND j.state = 'running' AND j.transcript_id = NEW.transcript_id
      AND j.transcript_revision = NEW.transcript_revision AND j.profile = NEW.profile
      AND j.profile_sha256 = NEW.profile_sha256)
  OR NEW.revision != coalesce((SELECT max(revision) + 1 FROM translations WHERE transcript_id = NEW.transcript_id AND transcript_revision = NEW.transcript_revision), 1)
  OR NEW.cue_count != (SELECT count(*) FROM transcript_cues WHERE transcript_id = NEW.transcript_id AND revision = NEW.transcript_revision)
BEGIN SELECT RAISE(ABORT, 'translation publication'); END;
CREATE TRIGGER translation_job_terminal BEFORE UPDATE OF state ON translation_jobs
WHEN NEW.state = 'succeeded' AND NOT EXISTS (
    SELECT 1 FROM translations t WHERE t.job_id = NEW.id AND t.job_generation = NEW.generation
      AND t.cue_count = (SELECT count(*) FROM translation_cues c WHERE c.transcript_id = t.transcript_id AND c.transcript_revision = t.transcript_revision AND c.revision = t.revision)
      AND t.translated_count = (SELECT count(*) FROM translation_cues c WHERE c.transcript_id = t.transcript_id AND c.transcript_revision = t.transcript_revision AND c.revision = t.revision AND c.state = 'translated'))
BEGIN SELECT RAISE(ABORT, 'translation job completion'); END;
PRAGMA user_version = 29;
