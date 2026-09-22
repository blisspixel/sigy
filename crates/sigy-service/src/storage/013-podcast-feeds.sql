-- One RSS refresh reuses the directory lifecycle: one active request, exact replay,
-- and retention of the last good snapshot. Episodes are not deleted when omitted.
CREATE TABLE podcast_refreshes (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    subscription_id TEXT NOT NULL REFERENCES podcast_subscriptions(id),
    state TEXT NOT NULL CHECK(state IN ('running', 'completed', 'failed', 'interrupted')),
    started_ms INTEGER NOT NULL CHECK(started_ms >= 0),
    completed_ms INTEGER CHECK(completed_ms IS NULL OR completed_ms >= started_ms),
    committed_items INTEGER NOT NULL DEFAULT 0 CHECK(committed_items BETWEEN 0 AND 500),
    truncated INTEGER NOT NULL DEFAULT 0 CHECK(truncated IN (0, 1)),
    live_count INTEGER NOT NULL DEFAULT 0 CHECK(live_count >= 0),
    skipped_items INTEGER NOT NULL DEFAULT 0 CHECK(skipped_items >= 0),
    document_sha256 TEXT CHECK(document_sha256 IS NULL OR length(document_sha256) = 64),
    failure TEXT CHECK(failure IS NULL OR length(failure) BETWEEN 1 AND 256),
    CHECK(failure IS NULL OR state IN ('failed', 'interrupted')),
    CHECK(state != 'completed' OR document_sha256 IS NOT NULL),
    CHECK(state != 'running' OR (completed_ms IS NULL AND document_sha256 IS NULL AND failure IS NULL))
) STRICT;
CREATE UNIQUE INDEX one_running_podcast_refresh ON podcast_refreshes(state) WHERE state = 'running';
CREATE TRIGGER podcast_refresh_request_immutable
BEFORE UPDATE OF id, subscription_id, started_ms ON podcast_refreshes BEGIN
    SELECT RAISE(ABORT, 'podcast refresh request is immutable');
END;
CREATE TRIGGER podcast_refreshes_retained BEFORE DELETE ON podcast_refreshes BEGIN
    SELECT RAISE(ABORT, 'podcast refresh is retained');
END;
CREATE TABLE podcast_snapshots (
    subscription_id TEXT PRIMARY KEY NOT NULL REFERENCES podcast_subscriptions(id),
    refresh_id TEXT NOT NULL REFERENCES podcast_refreshes(id),
    observed_ms INTEGER NOT NULL CHECK(observed_ms >= 0),
    document_sha256 TEXT NOT NULL CHECK(length(document_sha256) = 64),
    committed_items INTEGER NOT NULL CHECK(committed_items BETWEEN 0 AND 500),
    truncated INTEGER NOT NULL CHECK(truncated IN (0, 1)),
    live_count INTEGER NOT NULL CHECK(live_count >= 0)
) STRICT;
CREATE TRIGGER podcast_snapshots_retained BEFORE DELETE ON podcast_snapshots BEGIN
    SELECT RAISE(ABORT, 'podcast snapshot is retained');
END;
CREATE TABLE podcast_episodes (
    subscription_id TEXT NOT NULL REFERENCES podcast_subscriptions(id),
    episode_id TEXT NOT NULL CHECK(length(episode_id) = 64),
    identity_kind TEXT NOT NULL CHECK(identity_kind IN ('publisher_guid', 'derived_enclosure')),
    guid TEXT CHECK(guid IS NULL OR length(guid) BETWEEN 1 AND 2048),
    title TEXT CHECK(title IS NULL OR length(title) <= 512),
    published_ms INTEGER,
    enclosure_url TEXT CHECK(enclosure_url IS NULL OR length(enclosure_url) BETWEEN 1 AND 2048),
    enclosure_type TEXT CHECK(enclosure_type IS NULL OR length(enclosure_type) <= 128),
    enclosure_length INTEGER CHECK(enclosure_length IS NULL OR enclosure_length >= 0),
    assets_json TEXT NOT NULL CHECK(length(assets_json) BETWEEN 2 AND 32768),
    first_observed_ms INTEGER NOT NULL CHECK(first_observed_ms >= 0),
    last_observed_ms INTEGER NOT NULL CHECK(last_observed_ms >= first_observed_ms),
    latest_refresh_id TEXT NOT NULL REFERENCES podcast_refreshes(id),
    in_latest INTEGER NOT NULL CHECK(in_latest IN (0, 1)),
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    PRIMARY KEY(subscription_id, episode_id),
    CHECK((identity_kind = 'publisher_guid' AND guid IS NOT NULL)
        OR (identity_kind = 'derived_enclosure' AND guid IS NULL
            AND enclosure_url IS NOT NULL AND published_ms IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX podcast_episode_sequence ON podcast_episodes(subscription_id, sequence);
CREATE TRIGGER podcast_episode_identity_immutable
BEFORE UPDATE OF subscription_id, episode_id, identity_kind, guid, sequence, first_observed_ms
ON podcast_episodes BEGIN
    SELECT RAISE(ABORT, 'podcast episode identity is immutable');
END;
CREATE TRIGGER podcast_episodes_retained BEFORE DELETE ON podcast_episodes BEGIN
    SELECT RAISE(ABORT, 'podcast episode is retained');
END;
PRAGMA user_version = 13;
