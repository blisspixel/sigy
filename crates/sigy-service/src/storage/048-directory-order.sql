-- Disposable directory ordering projection. Canonical evidence is unchanged.
ALTER TABLE directory_stations ADD COLUMN name_ordered BLOB NOT NULL DEFAULT X''
    CHECK(length(name_ordered) <= 3072);
CREATE INDEX directory_name_order ON directory_stations(provider, name_ordered, id);
CREATE TABLE directory_catalog (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    namespace TEXT NOT NULL CHECK(length(namespace) = 32 AND namespace NOT GLOB '*[^0-9a-f]*'),
    revision INTEGER NOT NULL CHECK(revision >= 0)
) STRICT;
INSERT INTO directory_catalog VALUES(1, lower(hex(randomblob(16))), 0);
PRAGMA user_version = 48;
