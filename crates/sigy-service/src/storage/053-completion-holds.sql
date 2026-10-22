-- A publication body written before recognition or translation finish commits.
-- The row is not an executor envelope and cannot by itself prove containment.
-- Restart may publish it only through the existing finish checks.
-- A refused finish deletes the row. A crash before this insert still has no body.

CREATE TABLE completion_holds (
    family TEXT NOT NULL CHECK(family IN ('analysis', 'translation')),
    job_id TEXT NOT NULL CHECK(
        length(job_id) BETWEEN 1 AND 128 AND job_id NOT GLOB '*[^A-Za-z0-9_.:-]*'
    ),
    generation INTEGER NOT NULL CHECK(generation >= 0),
    attempt INTEGER NOT NULL CHECK(attempt BETWEEN 1 AND 3),
    lease_owner TEXT NOT NULL CHECK(length(lease_owner) BETWEEN 1 AND 128),
    body BLOB NOT NULL CHECK(length(body) BETWEEN 2 AND 262144),
    body_sha256 TEXT NOT NULL CHECK(
        length(body_sha256) = 64 AND body_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (family, job_id)
) STRICT;

DROP TRIGGER IF EXISTS completion_holds_no_update;
CREATE TRIGGER completion_holds_no_update
BEFORE UPDATE ON completion_holds
BEGIN
    SELECT RAISE(ABORT, 'completion holds are immutable');
END;

PRAGMA user_version = 53;
