# 0011: A direct listen is a pipe, not a recording

Date: 2026-09-21. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`listen source` plays one immutable `http_audio` revision. The service reads it through the shared acquirer, using that revision's redirect policy and the existing 15-minute and 256 MiB limits. The shared attempt slots are unchanged. A playlist content type or an `icy-metaint` header fails before any byte is offered to the decoder. The body is not stored.

Accepted bytes are written to one private local pipe. On Windows the pipe uses the same owner and SYSTEM grant as the control endpoint, and remote clients are rejected. On Unix it is a socket in the library directory, removed when the session ends. The name is a random nonce delivered only on the authenticated control channel. It is not a source URL and it is not catalog state.

The configured FFmpeg executable runs in the client. Its input is `pipe:0` under the pipe protocol whitelist. The service does not open an audio device and does not pass a source URL to the decoder. `--destination null` is the tested playback path. `--destination system` uses the same local output selection as retained-file playback.

The catalog stores a listen receipt so the same request ID does not open the source again. The receipt is not a recording, does not reserve quota, and does not publish a file. Restart marks a running listen interrupted and does not resume it. A revision already held by an active recording or listen is refused. Stopping a listen does not stop a recording. Pausing a live buffer is a later segmented-playback operation.

Catalog schema and local IPC advance to v10. Stop older controllers before replacing the executable. Migration from v9 adds the listen table without changing stations, sources, recordings, or budgets. No dependency changes are needed.
