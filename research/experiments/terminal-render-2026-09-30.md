# Terminal buffer and capture check

Measured: 2026-09-30. Host: the Windows 11 Ryzen 7 7840U development machine. This checks rendered buffers and one finite local capture. It does not qualify terminal latency, accessibility, sustained capacity or roadmap operation 36.

## Rendering

The actual Ratatui buffers were inspected for monitor summary, coverage and original/English passage views at 80 by 24 and 132 by 40, and recording timelines at those sizes and 40 by 10. The compact recording view keeps the media-clock bar and five-state legend visible. Terminal controls are removed before rendering; original scripts remain. Private JSON cells, text frames and rasterizations are in `.agents/monitor-ui/`. Rasterizing individual cells does not establish terminal font shaping, bidirectional layout or screen-reader behavior.

Three further limit frames at 40 by 10, 80 by 24 and 132 by 40 test an omitted publication beyond the plan. They explicitly show partial counts and an unavailable timeline, without a fabricated bar, clock extent or legend. The compact limit frame was also rasterized and inspected. Its truncation behavior differs deliberately from a complete metadata snapshot.

Monochrome flat-map buffers at 40 by 10, 80 by 24 and 160 by 48 retain a selected crowded station as `@` and a separate two-station group as `2`. Twenty warm samples per size timed only the widget render, excluding buffer allocation and terminal I/O:

| Buffer | 95th percentile, microseconds | Maximum, microseconds |
| --- | --- | --- |
| 40 by 10 | 1,669 | 1,692 |
| 80 by 24 | 2,230 | 2,420 |
| 160 by 48 | 6,219 | 7,281 |

The samples used four synthetic directory rows, not a large catalog. They are supporting measurements, not a terminal frame-rate promise. Receipts and actual cells are in `.agents/globe-review/`.

## Concurrent capture

A dependency-free Rust fixture served one deterministic 30-second mono WAV over `127.0.0.1`, paced at about real time. A fresh private library used the existing service and FFmpeg 9.0.1 with a 45-second, 2 MiB recording admission. The finite capture stayed active during all buffer samples; its journal state was `starting` before and after rendering, as this small finite path publishes after reception and decode. It then completed with 960,044 bytes, 30,000,000 decoded microseconds, and SHA-256 `8f03c116958b5765a0f7d6d751e8eee9fbd4f29c03c9e73fb686a22d1527f31d`, matching the independently hashed reference exactly. No public stream or paid service was contacted.

The first review attempts incorrectly waited for a `running` journal state on that finite path and were stopped after five seconds. They establish no rendering result. The successful run waited for the actual finite-path state and verified final publication. All fixture services were stopped afterward. No existing library was modified.

Actual terminal input/output measurement during capture, representative large pages, reduced-motion accessibility checks and sustained simultaneous source capacity remain open. External spend: USD 0.

## Finding navigation followup

Actual finding buffers were rasterized and inspected at 132 by 40, 80 by 24, 40 by 10 and 20 by 8. The wide view displays the named citation, original and translation revisions, staleness, retained interval, original Japanese and uncertain English together. Scrolling the normal view reaches both scripts; the compact view reaches English while preserving lookup and quit controls. The minimum-size view retains one scrollable detail line. Regression tests separately traverse it to both scripts and an error without discarding the citation.

Editing frames at 80 by 10 and 20 by 10 retain the end of a full 257-byte identity pair, including in linear mode with its focus prefix. Help says Escape ends editing. Review repaired an invisible citation at the minimum height, clipped query tails in linear mode, incorrect quit help during editing, and wide-glyph continuation handling in the private rasterizer. Receipts are under `.agents/finding-ui/`. These synthetic citation fixtures inspect actual application buffers; they do not measure terminal I/O, font shaping, accessibility or real transcription quality. Navigation reads metadata and does not play the cited audio.
