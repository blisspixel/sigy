# 0019: One episode enclosure download

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`podcast download` fetches one stored enclosure. The request is explicit. Subscribe and refresh do not start it, and the list explorer does not download. Feed text does not grant a network scope, a retention change, or playback.

The command registers one immutable `http_audio` revision for the enclosure URL. The revision uses the subscription's network scope, address pin, and redirect policy. The source name is `Episode`, not the item title. The recording is bound to that revision and uses the existing publication path: the shared acquirer, the capture journal, the library lock, and the configured FFmpeg decoder. The decoder receives the local published file, not the enclosure URL.

Catalog schema advances to v14. Local IPC advances to v15. Stop an older controller before replacing the executable. Migration from v13 rebuilds recording rows as radio profile and adds download bindings. A radio recording keeps its duration and byte ceiling. A failed migration rolls back.

Before connect, the service reserves the full episode ceiling: 512 MiB and a transfer deadline of 30 minutes. Radio attempts stay 15 minutes and 256 MiB. The reservation is the ceiling, not the declared length. An RSS `length` above 512 MiB is rejected before connect, and that rejection creates neither a source revision nor a recording. An HTTP `Content-Length` above the ceiling is rejected before any body byte is written.

A body that reaches the ceiling without a clean end is not playable. Duration and user stops are not playable either. Publication requires `end_of_body`, a successful decode, and a SHA-256 of the published bytes. Replay of the same recording id does not download again. A second recording id is refused while that enclosure still has a reserved or retained recording. Deleting the recording keeps its history and allows a later explicit id. Unsubscribe does not delete the recording.

Ordinary recording and retention commands show the result. The tested retention is temporary, then `record keep`. The export stays envelope v2 and omits the enclosure path and query. Transcript and chapter URLs are not fetched. The download does not request ICY metadata.

Playback of that retained file is [retained episode playback](0020-retained-episode-playback.md). This does not exit stage 4. Range resume, Atom-only input, and automatic polling remain out of scope.

Verification covers the fixed ceilings, v13 migration and rollback, a private enclosure URL denied before a recording exists, an RSS length above 512 MiB with no second connection, an HTTP `Content-Length` above the ceiling with an empty sink, refusal to publish a byte-limit end, and one local WAV fixture. That fixture contacts only 127.0.0.1. The published bytes match the fixture, the recording is retained under the enclosure revision, replay and a second id do not fetch again, and `record list`, `record path`, `record keep`, and `dvr status` see it.
