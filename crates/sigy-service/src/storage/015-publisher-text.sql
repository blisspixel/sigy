CREATE TABLE podcast_text_fetches (
    id TEXT PRIMARY KEY CHECK(length(id) BETWEEN 1 AND 128),
    subscription_id TEXT NOT NULL,
    episode_id TEXT NOT NULL CHECK(length(episode_id) = 64),
    kind TEXT NOT NULL CHECK(kind IN ('transcript', 'chapters')),
    asset_index INTEGER NOT NULL CHECK(asset_index >= 0 AND asset_index <= 7),
    state TEXT NOT NULL CHECK(state IN ('running', 'completed', 'failed', 'interrupted')),
    started_ms INTEGER NOT NULL CHECK(started_ms >= 0),
    completed_ms INTEGER CHECK(completed_ms IS NULL OR completed_ms >= started_ms),
    origin TEXT,
    media_type TEXT,
    language_hint TEXT,
    document_sha256 TEXT,
    cues_json TEXT,
    failure TEXT CHECK(failure IS NULL OR length(failure) BETWEEN 1 AND 256),
    CHECK(kind = 'transcript' OR asset_index <= 3),
    CHECK(
        (state = 'running' AND completed_ms IS NULL AND origin IS NULL AND media_type IS NOT NULL AND document_sha256 IS NULL AND cues_json IS NULL AND failure IS NULL)
        OR (state = 'completed' AND completed_ms IS NOT NULL AND origin IS NOT NULL AND media_type IS NOT NULL AND length(document_sha256) = 64 AND cues_json IS NOT NULL AND failure IS NULL)
        OR (state = 'failed' AND completed_ms IS NOT NULL AND failure IS NOT NULL AND document_sha256 IS NULL AND cues_json IS NULL)
        OR (state = 'interrupted' AND completed_ms IS NULL AND failure IS NOT NULL AND document_sha256 IS NULL AND cues_json IS NULL)
    ),
    FOREIGN KEY(subscription_id) REFERENCES podcast_subscriptions(id)
) STRICT;
CREATE TRIGGER podcast_text_fetches_retained BEFORE DELETE ON podcast_text_fetches BEGIN
    SELECT RAISE(ABORT, 'publisher text is retained');
END;

CREATE TABLE podcast_text_snapshots (
    subscription_id TEXT NOT NULL,
    episode_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    asset_index INTEGER NOT NULL,
    fetch_id TEXT NOT NULL,
    PRIMARY KEY (subscription_id, episode_id, kind, asset_index),
    FOREIGN KEY(fetch_id) REFERENCES podcast_text_fetches(id)
) STRICT;
CREATE TRIGGER podcast_text_snapshots_retained BEFORE DELETE ON podcast_text_snapshots BEGIN
    SELECT RAISE(ABORT, 'publisher text snapshot is retained');
END;

CREATE UNIQUE INDEX one_running_publisher_text ON podcast_text_fetches(state) WHERE state = 'running';
CREATE TRIGGER podcast_text_request_immutable
BEFORE UPDATE OF id, subscription_id, episode_id, kind, asset_index, started_ms ON podcast_text_fetches BEGIN
    SELECT RAISE(ABORT, 'publisher text request is immutable');
END;

PRAGMA user_version = 15;
