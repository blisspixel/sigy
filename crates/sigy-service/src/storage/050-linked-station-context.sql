CREATE INDEX station_source_links ON source_directory_links(provider, station_id, source_revision);
CREATE INDEX captures_by_source_revision ON capture_jobs(source_revision, id);
PRAGMA user_version = 50;
