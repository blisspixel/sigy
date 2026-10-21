-- Manual claims. A row exists only after an explicit user command.
-- Transcript text, a monitor proposal, and a model cannot insert one.
-- There is no score and no anomaly record.

CREATE TABLE claims (
    id TEXT PRIMARY KEY NOT NULL CHECK(
        length(id) BETWEEN 1 AND 128 AND id NOT GLOB '*[^A-Za-z0-9_.:-]*'
    ),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;

CREATE TABLE claim_revisions (
    claim_id TEXT NOT NULL REFERENCES claims(id),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 64),
    text TEXT NOT NULL CHECK(length(text) BETWEEN 1 AND 2000),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (claim_id, revision)
) STRICT;

CREATE TABLE claim_sightings (
    claim_id TEXT NOT NULL REFERENCES claims(id),
    id TEXT NOT NULL CHECK(
        length(id) BETWEEN 1 AND 128 AND id NOT GLOB '*[^A-Za-z0-9_.:-]*'
    ),
    monitor_id TEXT NOT NULL,
    finding_id TEXT NOT NULL,
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    PRIMARY KEY (claim_id, id),
    FOREIGN KEY (monitor_id, finding_id) REFERENCES monitor_findings(monitor_id, id)
) STRICT;

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

DROP TRIGGER IF EXISTS claim_limit;
CREATE TRIGGER claim_limit
BEFORE INSERT ON claims
WHEN (SELECT count(*) FROM claims) >= 256
BEGIN
    SELECT RAISE(ABORT, 'claim limit');
END;

DROP TRIGGER IF EXISTS claim_revision_order;
CREATE TRIGGER claim_revision_order
BEFORE INSERT ON claim_revisions
WHEN NEW.revision != COALESCE(
    (SELECT max(revision) FROM claim_revisions WHERE claim_id = NEW.claim_id), 0
) + 1
BEGIN
    SELECT RAISE(ABORT, 'claim revision order');
END;

DROP TRIGGER IF EXISTS claim_revision_limit;
CREATE TRIGGER claim_revision_limit
BEFORE INSERT ON claim_revisions
WHEN (SELECT count(*) FROM claim_revisions WHERE claim_id = NEW.claim_id) >= 64
BEGIN
    SELECT RAISE(ABORT, 'claim revision limit');
END;

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

DROP TRIGGER IF EXISTS claims_no_update;
CREATE TRIGGER claims_no_update
BEFORE UPDATE ON claims
BEGIN
    SELECT RAISE(ABORT, 'claims are immutable');
END;

DROP TRIGGER IF EXISTS claims_no_delete;
CREATE TRIGGER claims_no_delete
BEFORE DELETE ON claims
BEGIN
    SELECT RAISE(ABORT, 'claims are retained');
END;

DROP TRIGGER IF EXISTS claim_revisions_no_update;
CREATE TRIGGER claim_revisions_no_update
BEFORE UPDATE ON claim_revisions
BEGIN
    SELECT RAISE(ABORT, 'claim revisions are immutable');
END;

DROP TRIGGER IF EXISTS claim_revisions_no_delete;
CREATE TRIGGER claim_revisions_no_delete
BEFORE DELETE ON claim_revisions
BEGIN
    SELECT RAISE(ABORT, 'claim revisions are retained');
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

PRAGMA user_version = 52;
