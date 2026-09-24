-- An HLS master playlist resolves to candidates. Their attributes are declared
-- publisher text, not measurements. Existing M3U and PLS rows stay plain entries.
-- The recordings, recording_intervals and recording_gaps checks are widened in
-- Rust before this file runs, because SQLite cannot alter a CHECK constraint.
ALTER TABLE playlist_entries ADD COLUMN kind TEXT NOT NULL DEFAULT 'entry'
    CHECK(kind IN ('entry', 'hls_variant', 'hls_audio'));
ALTER TABLE playlist_entries ADD COLUMN bandwidth INTEGER
    CHECK(bandwidth IS NULL OR bandwidth BETWEEN 1 AND 10000000000);
ALTER TABLE playlist_entries ADD COLUMN codecs TEXT
    CHECK(codecs IS NULL OR length(codecs) BETWEEN 1 AND 256);
ALTER TABLE playlist_entries ADD COLUMN audio_only INTEGER
    CHECK(audio_only IS NULL OR audio_only IN (0, 1));
PRAGMA user_version = 30;
