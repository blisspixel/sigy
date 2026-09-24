# 0012: A finite HLS media playlist is one recording

Date: 2026-09-21. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`record hls` records one immutable `http_audio` revision when that revision's URL is an HLS media playlist. The service reads the playlist with the existing playlist acquisition: 64 KiB, the directory document deadline, the revision's redirect policy, no cookies, and no retry. It then fetches the media segments through the same acquirer into one staging file. Segment bytes and elapsed time share the recording ceiling. The default ceiling is 60 seconds and 64 MiB, using the same flags as `record start`. The playlist document is not part of the published file.

The document must contain `#EXTM3U` and `#EXT-X-ENDLIST`. At most 32 media URLs are accepted. Relative URLs resolve against the final playlist URL and then the parent network scope and redirect policy. The allowed tags are `#EXTM3U`, `#EXTINF`, `#EXT-X-ENDLIST`, `#EXT-X-INDEPENDENT-SEGMENTS`, `#EXT-X-VERSION`, `#EXT-X-TARGETDURATION`, `#EXT-X-MEDIA-SEQUENCE`, and `#EXT-X-PLAYLIST-TYPE`.

A master playlist fails before any variant URL is authorized or fetched. The master tags are `#EXT-X-STREAM-INF`, `#EXT-X-I-FRAME-STREAM-INF`, `#EXT-X-MEDIA`, `#EXT-X-SESSION-DATA`, and `#EXT-X-SESSION-KEY`. A document without `#EXT-X-ENDLIST` fails as a live playlist before segment authorization. `#EXT-X-KEY`, `#EXT-X-MAP`, `#EXT-X-BYTERANGE`, `#EXT-X-DISCONTINUITY`, `#EXT-X-PART`, and any other tag fail closed. Segments must declare one audio type. `record start` still rejects playlist media types. `source playlist resolve` still rejects an HLS marker or a directory HLS flag and stores no candidates.

Publication is the existing recording path: the local staging file is flushed and synced, the configured FFmpeg decoder checks that file, and the catalog records the SHA-256, exact length, and quota. FFmpeg does not receive a playlist URL or a segment URL. Segments do not become source revisions. The stored HTTP route is the first segment's response chain. Later segments are not appended, because a recording route remains one chain. Paths and queries stay omitted. The recording envelope remains v2.

Catalog schema stays v10. Local IPC advances to v11 because the recording operation gained an HLS action. There is no migration. Stop an older controller before replacing the executable. No dependency was added.

A local fixture publishes two WAV halves as one file equal to their concatenation, and a master playlist fails without a request path containing `variant`. Parser tests cover the rejections above, including a cross-origin segment and a 33rd segment. This does not qualify public HLS stations, encrypted media, or interleaved ICY metadata.

Amended on 2026-09-24 by [live HLS and master playlist candidates](0042-live-hls.md): `record hls --live` records a live media playlist until a ceiling or the first gap, segments may be MPEG transport streams (`video/mp2t`, format `mpegts`) or ADTS AAC, comments and informational tags such as `#EXT-X-PROGRAM-DATE-TIME` are ignored, and every segment needs `#EXTINF`. A finite playlist still rejects discontinuities and keeps the 32-segment limit. Playlist resolution now lists the variants of a master playlist. Catalog schema and local IPC are v30.
