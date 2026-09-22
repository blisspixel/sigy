-- A podcast subscription is user intent. It is not a favorite and not an audio revision.
CREATE TABLE podcast_subscriptions (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    feed_url TEXT NOT NULL CHECK(length(feed_url) BETWEEN 1 AND 2048),
    network_scope TEXT NOT NULL CHECK(network_scope IN ('public_internet', 'pinned_address')),
    pinned_address TEXT,
    redirect_policy TEXT NOT NULL CHECK(redirect_policy IN ('deny', 'same-origin', 'public')
        AND (redirect_policy != 'public' OR network_scope = 'public_internet')),
    created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
    polls TEXT NOT NULL CHECK(polls IN ('active', 'stopped')),
    CHECK((network_scope = 'public_internet' AND pinned_address IS NULL)
        OR (network_scope = 'pinned_address' AND pinned_address IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX one_active_podcast_feed ON podcast_subscriptions(feed_url) WHERE polls = 'active';
CREATE TRIGGER podcast_subscription_authority_immutable
BEFORE UPDATE OF id, feed_url, network_scope, pinned_address, redirect_policy, created_ms
ON podcast_subscriptions BEGIN
    SELECT RAISE(ABORT, 'podcast subscription authority is immutable');
END;
CREATE TRIGGER podcast_subscription_polls_only_stop
BEFORE UPDATE OF polls ON podcast_subscriptions
WHEN OLD.polls != 'active' OR NEW.polls != 'stopped' BEGIN
    SELECT RAISE(ABORT, 'podcast polling only stops');
END;
CREATE TRIGGER podcast_subscriptions_no_delete BEFORE DELETE ON podcast_subscriptions BEGIN
    SELECT RAISE(ABORT, 'podcast subscription is retained');
END;
PRAGMA user_version = 12;
