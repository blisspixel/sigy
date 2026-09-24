# 0026: Capture gaps

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

A gap is a hole in one capture's planned timeline. It records a cause and a microsecond range. It has no object, hash, or audio bytes. Silence is not written in its place.

The causes are disconnect, recovery, codec change, refused renewal, capture pause, and a backward clock. Each one covers the planned window that published audio does not. A codec change does not append the new format as the next continuous interval. An expired segment lease is not extended. A backward clock is not stored as the capture's new time. `record pause` interrupts the running capture. Service recovery does the same for an active attempt and uses the recovery cause.

`listen file` refuses a seek inside a gap before it starts the decoder. A seek in published audio is unchanged. This does not play many segments, and it does not exit stage 4.

Catalog schema is v18. Local IPC is v18.

Amended on 2026-09-24 by [live HLS](0042-live-hls.md): a live HLS capture ends at a skipped media sequence, a discontinuity, or a failed or stalled reload and records the rest of its plan as a `sequence_skip`, `discontinuity`, or `reload_failure` gap. A failed segment after audio is a `disconnect` gap and a media type change is a `codec_change` gap. The published file ends where the gap starts. Catalog schema and local IPC are v30.
