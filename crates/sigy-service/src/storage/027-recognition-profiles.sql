-- Immutable local recognizer profiles. Paths locate files; hashes identify them.
CREATE TABLE recognition_profiles (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128 AND id NOT IN ('local-unmeasured', 'retained-sha256-v1')),
    engine TEXT NOT NULL CHECK(engine = 'whisper-cpp-cli-v1'),
    runtime_dir TEXT NOT NULL CHECK(length(CAST(runtime_dir AS BLOB)) BETWEEN 1 AND 1024),
    executable TEXT NOT NULL CHECK(length(CAST(executable AS BLOB)) BETWEEN 1 AND 128),
    runtime_sha256 TEXT NOT NULL CHECK(length(runtime_sha256) = 64 AND runtime_sha256 NOT GLOB '*[^0-9a-f]*'),
    runtime_files INTEGER NOT NULL CHECK(runtime_files BETWEEN 1 AND 256),
    runtime_bytes INTEGER NOT NULL CHECK(runtime_bytes BETWEEN 1 AND 1073741824),
    model_path TEXT NOT NULL CHECK(length(CAST(model_path AS BLOB)) BETWEEN 1 AND 1024),
    model_sha256 TEXT NOT NULL CHECK(length(model_sha256) = 64 AND model_sha256 NOT GLOB '*[^0-9a-f]*'),
    model_bytes INTEGER NOT NULL CHECK(model_bytes BETWEEN 1 AND 8589934592),
    vad_path TEXT NOT NULL CHECK(length(CAST(vad_path AS BLOB)) BETWEEN 1 AND 1024),
    vad_sha256 TEXT NOT NULL CHECK(length(vad_sha256) = 64 AND vad_sha256 NOT GLOB '*[^0-9a-f]*'),
    vad_bytes INTEGER NOT NULL CHECK(vad_bytes BETWEEN 1 AND 67108864),
    threads INTEGER NOT NULL CHECK(threads BETWEEN 1 AND 64),
    memory_bytes INTEGER NOT NULL CHECK(memory_bytes BETWEEN 268435456 AND 68719476736),
    deadline_ms INTEGER NOT NULL CHECK(deadline_ms BETWEEN 1000 AND 3600000),
    profile_sha256 TEXT NOT NULL UNIQUE CHECK(length(profile_sha256) = 64 AND profile_sha256 NOT GLOB '*[^0-9a-f]*'),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;
CREATE TRIGGER recognition_profile_limit BEFORE INSERT ON recognition_profiles
WHEN (SELECT count(*) FROM recognition_profiles) >= 64
BEGIN SELECT RAISE(ABORT, 'recognition profile limit'); END;
CREATE TRIGGER recognition_profile_no_update BEFORE UPDATE ON recognition_profiles
BEGIN SELECT RAISE(ABORT, 'recognition profiles are immutable'); END;
CREATE TRIGGER recognition_profile_no_delete BEFORE DELETE ON recognition_profiles
BEGIN SELECT RAISE(ABORT, 'recognition profiles are retained'); END;
PRAGMA user_version = 27;
