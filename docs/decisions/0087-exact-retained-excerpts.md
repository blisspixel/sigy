# 0087: Exact retained excerpts

Date: 2026-10-04. Status: implemented increment; verification and limitations belong in [active work](../development/progress.md). This advances DV-01B and S-07 without completing an interactive DVR or a release stage.

## Admission and identity

Keep the existing whole-object protected reader and add explicit range and finding admission. `listen file --seek-us START --end-us END` requests a half-open recording-timeline interval. `listen finding MONITOR FINDING --request ID` resolves that exact immutable citation inside the same immediate transaction that acquires original-file protection. Neither operation starts capture, recognition, translation or source contact.

Require `segment_start <= start < end <= segment_end`, one sealed unreleased segment, and no gap overlapping `[start,end)`. The end may equal the segment end. Crossing even adjacent segments refuses. A finding must have stored retained bounds and exact original/translation revisions, cue, recording checksum and source lineage. Current media must still cover the interval. An expired or missing finding supplies no inferred interval; a newer transcript or translation never substitutes for the cited revision.

Catalog v51 adds nullable excerpt and citation columns to the existing reader table; local IPC is v52. Legacy receipts omit the new JSON field and retain the unchanged v1 digest recipe. A v2 excerpt carries its exclusive timeline end and optional complete citation identity under a separate digest domain. Worker generation remains distinct from specification version. Both versions share the same request namespace, four unresolved-reader and 4,096 lifetime-receipt limits, retention guards, recovery and completion owner.

Existing timeline start/end, file duration, encoded bytes, object key and checksum still identify the complete sealed object. Decoder bounds derive `file_start = start - segment_start` and `file_end = end - segment_start`; no field is repurposed and encoded input is not prorated. Exact request replay returns history before fresh retention, clock, capacity or decoder checks and dispatches nothing. Changed range end, request mode or finding identity conflicts. Historical receipts remain readable after media deletion.

Backup audits retained-reader identity and lineage before creating its destination. Restore also validates those semantics after file and manifest hashes; a self-consistent outer hash cannot authorize a malformed excerpt or citation.

## Sample selection and completion

Explicit excerpts use interleaved little-endian floating-point PCM. At the declared output rate `R`, select sample onsets from `ceil(file_start * R / 1000000)` up to, excluding, `ceil(file_end * R / 1000000)`. Checked absolute endpoints determine the frame count; rounding the duration alone can select an extra frame. An interval containing no sample onset refuses.

Resample to the declared rate with timestamp compensation disabled, trim by sample count, then reset output timestamps. Completion requires every expected frame, whole-frame alignment, finite samples, successful decoder exit and final progress agreeing with that count to integer-microsecond rounding. The legacy 100 ms progress tolerance remains unchanged for old requests; it cannot establish new excerpt completion. The reported selection resolution is one output frame. It is not a claim of acoustic timing or universal compressed-format accuracy. [Primary-source research](../../research/45-excerpt-boundaries-and-evidence.md) records the clock convention and qualification boundaries.

Silent excerpts use a fixed 48 kHz stereo profile and open no device. They reuse the bounded native group, two-chunk PCM queue and original service stream. Requested process, memory and CPU limits must all be available. Windows has local evidence; other hosts refuse unavailable containment rather than weakening it. Windows system excerpts use the existing negotiated output helper. Other system-output platforms remain unimplemented for this new path. Legacy playback retains its prior platform paths.

Original checksum verification and encoded transfer retain the full-object ceilings. The decoder's work horizon includes the full file duration rounded down to seconds plus 30 seconds; silent connection/execution has a plus-35-second outer bound. Windows output retains its existing startup, feed and presentation-drain bounds. Cold hashing or a long prefix can exhaust those bounds and refuse. Admission ceilings establish no measured seek latency or simultaneous capacity.

Decoder, operation failure, output, native group and original-reader outcomes remain separate. An excerpt can finish before the encoded reader reaches EOF. Broken pipe, requested stop or decoder exit cannot certify original-file closure. The service joins its original reader before releasing protection; checksum or size failure remains an independent failure. Native termination requests and process-group Drop do not establish closure. Null-path failures preserve completed decoder evidence and observed or unproven native closure independently. If the client cannot observe original-reader closure, its result keeps the last admitted receipt and already observed decoding/native outcomes, reports `reader_closure_failure` separately and fails the command without calling that reader closed.

## Interface and remaining work

The CLI exposes deliberate actions and reusable receipts. TUI selection, finding inspection and Enter remain read-only. No new MCP mutation is exposed. Integrated terminal playback controls, segment handoff, pause/seek of actual output and latest-sealed following remain later increments through the same service authority.

The independent first oracle is a one-second stereo 48 kHz WAV. `[125000,375000)` selects frames 6,000 through 17,999: 12,000 frames and 96,000 f32 bytes. Compare every sample and distinct sentinels outside both boundaries. Additional witnesses cover fractional one-frame selections, empty selections, nonzero segment origins, exact end, adjacent ranges, early EOF, split scalars, immutable replay, legacy migration, citation staleness and retention refusal.

These fixtures establish results for their declared inputs and bounds only. Compressed priming, other sample rates and resampling content, cold-file performance, cancellation under pressure, abrupt parent death, hostile-decoder network isolation, acoustic delivery and sustained multi-client capacity remain explicit evidence gates. Originals and citation history retain their existing uncertainty and revision semantics.
