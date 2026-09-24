-- Provider routes, dated price evidence, and paid attempt records. A route
-- stores the name of an environment variable, never a credential. Rows are
-- immutable; a changed route or price is a new row under a new ID.
CREATE TABLE provider_routes (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    provider TEXT NOT NULL CHECK(provider IN ('openrouter', 'ollama')),
    endpoint_origin TEXT NOT NULL CHECK(length(endpoint_origin) BETWEEN 1 AND 256),
    model TEXT NOT NULL CHECK(length(model) BETWEEN 1 AND 128),
    upstreams_json TEXT NOT NULL CHECK(
        json_valid(upstreams_json)
        AND json_type(upstreams_json) = 'array'
        AND json_array_length(upstreams_json) <= 16
    ),
    task TEXT NOT NULL CHECK(task IN ('translate-text')),
    secret_env TEXT CHECK(
        secret_env IS NULL
        OR (length(secret_env) BETWEEN 1 AND 128
            AND secret_env NOT GLOB '*[^A-Za-z0-9_]*'
            AND substr(secret_env, 1, 1) NOT GLOB '[0-9]')
    ),
    allow_fallbacks INTEGER NOT NULL DEFAULT 0 CHECK(allow_fallbacks = 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    CHECK(
        (provider = 'openrouter' AND secret_env IS NOT NULL
            AND json_array_length(upstreams_json) >= 1
            AND endpoint_origin GLOB 'https://*')
        OR (provider = 'ollama' AND secret_env IS NULL
            AND json_array_length(upstreams_json) = 0
            AND endpoint_origin GLOB 'http://*')
    )
) STRICT;
CREATE TRIGGER provider_route_limit BEFORE INSERT ON provider_routes
WHEN (SELECT count(*) FROM provider_routes) >= 256
BEGIN SELECT RAISE(ABORT, 'provider route limit'); END;
CREATE TRIGGER provider_route_no_update BEFORE UPDATE ON provider_routes
BEGIN SELECT RAISE(ABORT, 'provider routes are immutable'); END;
CREATE TRIGGER provider_route_no_delete BEFORE DELETE ON provider_routes
BEGIN SELECT RAISE(ABORT, 'provider routes are retained'); END;

-- A language pair is authorized for a route but starts unvalidated. Promotion
-- needs measured evidence and a later migration; nothing here can set it.
CREATE TABLE provider_route_pairs (
    route_id TEXT NOT NULL REFERENCES provider_routes(id),
    source_language TEXT NOT NULL CHECK(length(source_language) BETWEEN 2 AND 128),
    target_language TEXT NOT NULL CHECK(length(target_language) BETWEEN 2 AND 128),
    validation TEXT NOT NULL DEFAULT 'unvalidated' CHECK(validation = 'unvalidated'),
    PRIMARY KEY(route_id, source_language, target_language),
    CHECK(source_language != target_language)
) STRICT;
CREATE TRIGGER provider_route_pair_limit BEFORE INSERT ON provider_route_pairs
WHEN (SELECT count(*) FROM provider_route_pairs WHERE route_id = NEW.route_id) >= 16
BEGIN SELECT RAISE(ABORT, 'provider language pair limit'); END;
CREATE TRIGGER provider_route_pair_no_update BEFORE UPDATE ON provider_route_pairs
BEGIN SELECT RAISE(ABORT, 'provider language pairs are immutable'); END;
CREATE TRIGGER provider_route_pair_no_delete BEFORE DELETE ON provider_route_pairs
BEGIN SELECT RAISE(ABORT, 'provider language pairs are retained'); END;

-- Exact per-unit USD rates as canonical decimal text, with the retrieval time,
-- a bounded validity window, and a source note.
CREATE TABLE provider_price_snapshots (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    route_id TEXT NOT NULL REFERENCES provider_routes(id),
    retrieved_ms INTEGER NOT NULL CHECK(retrieved_ms >= 0),
    valid_until_ms INTEGER NOT NULL CHECK(
        valid_until_ms > retrieved_ms AND valid_until_ms - retrieved_ms <= 2592000000
    ),
    prompt TEXT NOT NULL CHECK(length(prompt) BETWEEN 1 AND 64 AND prompt NOT GLOB '*[^0-9.]*'),
    completion TEXT NOT NULL CHECK(length(completion) BETWEEN 1 AND 64 AND completion NOT GLOB '*[^0-9.]*'),
    request TEXT NOT NULL CHECK(length(request) BETWEEN 1 AND 64 AND request NOT GLOB '*[^0-9.]*'),
    internal_reasoning TEXT NOT NULL CHECK(length(internal_reasoning) BETWEEN 1 AND 64 AND internal_reasoning NOT GLOB '*[^0-9.]*'),
    input_cache_read TEXT NOT NULL CHECK(length(input_cache_read) BETWEEN 1 AND 64 AND input_cache_read NOT GLOB '*[^0-9.]*'),
    input_cache_write TEXT NOT NULL CHECK(length(input_cache_write) BETWEEN 1 AND 64 AND input_cache_write NOT GLOB '*[^0-9.]*'),
    image TEXT NOT NULL CHECK(length(image) BETWEEN 1 AND 64 AND image NOT GLOB '*[^0-9.]*'),
    audio TEXT NOT NULL CHECK(length(audio) BETWEEN 1 AND 64 AND audio NOT GLOB '*[^0-9.]*'),
    web_search TEXT NOT NULL CHECK(length(web_search) BETWEEN 1 AND 64 AND web_search NOT GLOB '*[^0-9.]*'),
    unrecognized TEXT NOT NULL CHECK(length(unrecognized) BETWEEN 1 AND 64 AND unrecognized NOT GLOB '*[^0-9.]*'),
    source_note TEXT NOT NULL CHECK(
        length(CAST(source_note AS BLOB)) BETWEEN 1 AND 256
        AND source_note NOT GLOB '*[^ -~]*'
    ),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0)
) STRICT;
CREATE INDEX provider_price_snapshots_by_route ON provider_price_snapshots(route_id, retrieved_ms, id);
CREATE TRIGGER provider_price_snapshot_route BEFORE INSERT ON provider_price_snapshots
WHEN (SELECT provider FROM provider_routes WHERE id = NEW.route_id) IS NOT 'openrouter'
BEGIN SELECT RAISE(ABORT, 'only a paid route has a price snapshot'); END;
CREATE TRIGGER provider_price_snapshot_limit BEFORE INSERT ON provider_price_snapshots
WHEN (SELECT count(*) FROM provider_price_snapshots WHERE route_id = NEW.route_id) >= 64
BEGIN SELECT RAISE(ABORT, 'provider price snapshot limit'); END;
CREATE TRIGGER provider_price_snapshot_no_update BEFORE UPDATE ON provider_price_snapshots
BEGIN SELECT RAISE(ABORT, 'price snapshots are immutable'); END;
CREATE TRIGGER provider_price_snapshot_no_delete BEFORE DELETE ON provider_price_snapshots
BEGIN SELECT RAISE(ABORT, 'price snapshots are retained'); END;

-- One paid attempt per ledger request. The outcome and generation id are
-- written once after the transport returns; the ledger keeps the liability.
CREATE TABLE provider_attempts (
    request_id TEXT PRIMARY KEY NOT NULL REFERENCES requests(id),
    route_id TEXT NOT NULL REFERENCES provider_routes(id),
    snapshot_id TEXT NOT NULL REFERENCES provider_price_snapshots(id),
    source_language TEXT NOT NULL CHECK(length(source_language) BETWEEN 2 AND 128),
    target_language TEXT NOT NULL CHECK(length(target_language) BETWEEN 2 AND 128),
    input_sha256 TEXT NOT NULL CHECK(length(input_sha256) = 64 AND input_sha256 NOT GLOB '*[^0-9a-f]*'),
    prompt_tokens INTEGER NOT NULL CHECK(prompt_tokens BETWEEN 1 AND 1048576),
    completion_tokens INTEGER NOT NULL CHECK(completion_tokens BETWEEN 1 AND 32768),
    retry_of TEXT REFERENCES provider_attempts(request_id),
    outcome TEXT CHECK(outcome IS NULL OR outcome IN (
        'completed', 'usage_missing', 'timeout', 'refused', 'retry_after', 'failed'
    )),
    generation_id TEXT CHECK(generation_id IS NULL OR length(generation_id) BETWEEN 1 AND 128),
    retry_after_ms INTEGER CHECK(retry_after_ms IS NULL OR retry_after_ms BETWEEN 0 AND 86400000),
    outcome_ms INTEGER CHECK(outcome_ms IS NULL OR outcome_ms >= 0),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    CHECK((outcome IS NULL) = (outcome_ms IS NULL)),
    CHECK(outcome IS NOT NULL OR generation_id IS NULL),
    CHECK((retry_after_ms IS NOT NULL) = (outcome IS 'retry_after')),
    CHECK(retry_of IS NULL OR retry_of != request_id)
) STRICT;
CREATE UNIQUE INDEX provider_attempts_one_retry ON provider_attempts(retry_of);
CREATE TRIGGER provider_attempt_outcome_once BEFORE UPDATE ON provider_attempts
WHEN OLD.outcome IS NOT NULL
    OR NEW.request_id IS NOT OLD.request_id
    OR NEW.route_id IS NOT OLD.route_id
    OR NEW.snapshot_id IS NOT OLD.snapshot_id
    OR NEW.source_language IS NOT OLD.source_language
    OR NEW.target_language IS NOT OLD.target_language
    OR NEW.input_sha256 IS NOT OLD.input_sha256
    OR NEW.prompt_tokens IS NOT OLD.prompt_tokens
    OR NEW.completion_tokens IS NOT OLD.completion_tokens
    OR NEW.retry_of IS NOT OLD.retry_of
    OR NEW.created_ms IS NOT OLD.created_ms
BEGIN SELECT RAISE(ABORT, 'provider attempt outcome is written once'); END;
CREATE TRIGGER provider_attempt_no_delete BEFORE DELETE ON provider_attempts
BEGIN SELECT RAISE(ABORT, 'provider attempts are retained'); END;
PRAGMA user_version = 28;
