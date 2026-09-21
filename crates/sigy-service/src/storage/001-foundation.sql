CREATE TABLE budgets (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    limit_micros INTEGER NOT NULL CHECK(limit_micros >= 0),
    settled_micros INTEGER NOT NULL DEFAULT 0 CHECK(settled_micros >= 0),
    reserved_micros INTEGER NOT NULL DEFAULT 0 CHECK(reserved_micros >= 0),
    frozen INTEGER NOT NULL DEFAULT 0 CHECK(frozen IN (0, 1)),
    CHECK(frozen = 1 OR (settled_micros <= limit_micros AND reserved_micros <= limit_micros - settled_micros))
) STRICT;

INSERT INTO budgets(id, limit_micros) VALUES ('global', 0);

CREATE TABLE requests (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    context TEXT NOT NULL CHECK(length(context) BETWEEN 1 AND 128),
    maximum_micros INTEGER NOT NULL CHECK(maximum_micros > 0),
    state TEXT NOT NULL CHECK(state IN ('reserved', 'submitted', 'uncertain', 'settled', 'released')),
    actual_micros INTEGER CHECK(actual_micros >= 0),
    evidence TEXT CHECK(length(evidence) BETWEEN 1 AND 128),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    CHECK((state = 'settled' AND actual_micros IS NOT NULL AND evidence IS NOT NULL)
        OR (state != 'settled' AND actual_micros IS NULL AND evidence IS NULL))
) STRICT;

CREATE TABLE request_budgets (
    request_id TEXT NOT NULL REFERENCES requests(id),
    budget_id TEXT NOT NULL REFERENCES budgets(id),
    PRIMARY KEY(request_id, budget_id)
) STRICT;

CREATE TABLE ledger_events (
    sequence INTEGER PRIMARY KEY,
    request_id TEXT REFERENCES requests(id),
    budget_id TEXT REFERENCES budgets(id),
    kind TEXT NOT NULL CHECK(kind IN ('limit', 'reserved', 'submitted', 'uncertain', 'settled', 'released')),
    amount_micros INTEGER CHECK(amount_micros >= 0),
    recorded_ms INTEGER NOT NULL CHECK(recorded_ms >= 0)
) STRICT;

CREATE TRIGGER ledger_events_no_update BEFORE UPDATE ON ledger_events BEGIN
    SELECT RAISE(ABORT, 'ledger events are append-only');
END;
CREATE TRIGGER ledger_events_no_delete BEFORE DELETE ON ledger_events BEGIN
    SELECT RAISE(ABORT, 'ledger events are append-only');
END;

CREATE INDEX request_budgets_by_budget ON request_budgets(budget_id, request_id);
CREATE INDEX requests_by_state ON requests(state);
PRAGMA user_version = 1;
PRAGMA application_id = 1397311321;
