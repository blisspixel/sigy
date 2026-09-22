# 0022: Publisher text

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`podcast text` fetches one stored transcript or chapter document when asked. Refresh still does not fetch it. The request uses the subscription's network scope, address pin, and redirect policy on the shared acquirer. One request reads one asset. Replay of the same id does not fetch again. A failed document leaves the previous good text for that asset.

The accepted types are WebVTT, SRT (`application/srt` and `application/x-subrip`), podcast JSON transcripts, and podcast JSON chapters (`application/json` and `application/json+chapters`). A `utf-8` or `us-ascii` charset parameter is accepted and is not stored. HTML and plain text are rejected before connect. The decoded document is at most 1 MiB. The compressed body is at most 256 KiB. The expansion ratio stays at most 16. At most 1,000 cues are accepted. Chapter assets are indexes 0 through 3. Nested image and link URLs inside a chapter file are not requested.

The stored record has the final origin, the response type, the feed language hint, the SHA-256 of the document bytes, alignment `unverified`, and attribution `publisher`. Cue times are `publisher_start_ms`. They are not media time. The text is not an ASR row. Ordinary episode lists still show only asset counts and omit URLs. Inspection through `podcast text-show` shows the cues and omits paths and queries.

The bytes are not a recording and do not change charged or reserved quota. One fetch may run. Service restart marks a running fetch interrupted and leaves the previous snapshot in place. A late failure cannot overwrite that interrupted row. Catalog schema advances to v15. Local IPC advances to v16. This does not exit stage 4.
