-- Favorites are user state. Refreshes cannot replace them or evict their cache rows.
CREATE TABLE station_favorites (
    provider TEXT NOT NULL CHECK(provider = 'radio_browser'),
    station_id TEXT NOT NULL,
    PRIMARY KEY(provider, station_id),
    FOREIGN KEY(provider, station_id) REFERENCES directory_stations(provider, id) ON DELETE RESTRICT
) STRICT;
PRAGMA user_version = 7;
