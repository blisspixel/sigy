# 0013: ICY titles are observations, not audio

Date: 2026-09-21. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`record start` requests interleaved ICY metadata only when `--icy` is set. That request sends `icy-metadata: 1`. Without the flag, the request sends `icy-metadata: 0`, and an `icy-metaint` response fails before any body byte is written. `listen source` and `record hls` stay on that default and do not accept interleaved metadata.

When metadata is requested, the service removes each block before the bytes reach the recording file. Only audio bytes are written, hashed, and published. The recording byte ceiling counts those audio bytes. FFmpeg receives the local audio file and does not receive the metadata text.

A stored block is the UTF-8 text left after trailing NUL padding is removed. Control characters and bidirectional overrides are rejected. At most 1024 blocks are kept, and each block is at most 4080 bytes. `audio_offset` is the number of audio bytes before the block. It is not a decoded timestamp. The text is untrusted: it does not rename the source, grant a URL, or authorize another request. A URL inside the text is not fetched.

The metadata flag is part of the recording's exact replay identity. Observations are inserted in the same transaction as publication. An export stays schema version 2 when the recording has no observations. A recording with observations exports schema version 3 and `capture.icy_observations`. Catalog schema advances to v11. Local IPC advances to v12. Stop an older controller before replacing the executable. No dependency was added.

A local fixture publishes a WAV equal to the audio with the metadata removed. The export contains one hostile title and stream URL, the source name is unchanged, and the fixture records no request for that URL. The same server without `--icy` fails before publication. This does not qualify public ICY stations or non-UTF-8 metadata.
