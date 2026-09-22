# 0009: Playlist resolution stays on the shared acquirer

Date: 2026-09-21. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

Resolving a playlist is one document read, not playback and not a recording. The service uses the existing HTTP acquirer, the revision's redirect policy, identity encoding, no cookies, and no retry. The body limit is 64 KiB and the deadline is the directory document deadline. The final content type must be `audio/x-mpegurl`, `audio/mpegurl`, `application/x-mpegurl`, `application/vnd.apple.mpegurl`, or `audio/x-scpls`.

At most 32 entries are kept. Relative references resolve against the final response URL and then the parent network scope and redirect policy. Nested playlist URLs are not fetched. An HLS tag, or a directory station linked to the parent with the HLS flag set, fails the resolve and stores no candidates. The audio acquisition path still rejects those playlist media types.

Accepting an index registers one new immutable `http_audio` revision and records the parent revision, request id, index, and document hash. The parent revision is not modified. Accept performs no network I/O. Replaying a resolve or an accept does not fetch or register again.

The catalog and local IPC advance to v8. Stop older controllers before replacing the executable. Migration from v7 adds the playlist tables without changing source, recording, budget, or directory rows. Ordinary playlist views show origins only. Entry paths and queries remain in the private catalog. No dependency changes are needed.

This operation does not play the accepted entry, fetch HLS media segments, split ICY metadata, or report a directory click. Finite HLS media recording is the separate `record hls` command in [HLS media playlists](0012-hls-media-playlist.md). ICY metadata remains a later build-order operation.
