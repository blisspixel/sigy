# 0014: Format claims follow decoded runs

Date: 2026-09-21. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

The recording header gate recognizes `audio/mpeg`, `audio/aac`, `audio/flac`, `audio/ogg`, and `audio/wav`, including the aliases already accepted by acquisition. A recognized type is the label stored with a publication. The README names a format only after a run on that operating system, with that FFmpeg build, decoded it.

The local ladder is three steps on the existing acquisition path. Direct `record start` fetches one audio URL. `source playlist resolve` reads one playlist document, and `source playlist accept` registers one chosen entry with no further I/O during accept. A permitted redirect is the revision's existing policy, at most three hops, checked at every hop. The decoder still receives a local file or a private pipe. It does not receive a source URL, a playlist URL, or `.m3u8`.

On Windows x86_64, with the installed FFmpeg 9.0.1, one ignored service fixture generated the clips with that executable and served them on `127.0.0.1`. Direct recording decoded WAV, MP3, AAC (ADTS bytes served as `audio/aac`), FLAC, and Ogg Vorbis. Each publication reports that format and a positive decoded duration. The same server answers `/list` with one same-origin redirect to `/final.m3u`. That document has one relative entry, the WAV clip. Accepting index 0, recording the new revision, and `listen source --destination null` complete as WAV with advancing progress and a positive playhead. The fixture records only those local paths.

Catalog schema stays v11. Local IPC stays v12. No migration and no application dependency were added. A public station check waits for separate authorization and remains inside the existing capture caps. One smoke session is not a support matrix, and it does not qualify another operating system, another FFmpeg build, or every file that shares one of these content types.
