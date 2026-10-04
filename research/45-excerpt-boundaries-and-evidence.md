# Excerpt boundaries and evidence

Reviewed: 2026-10-04. Scope: deterministic retained PCM excerpts, independent boundary witnesses and distinct completion lifetimes. Implementation contract: [Decision0087](../docs/decisions/0087-exact-retained-excerpts.md).

## Primary sources and alternatives

[FFmpeg CLI documentation](https://ffmpeg.org/ffmpeg.html) distinguishes input seek from output-side decoding/discard and bounds output with `-t`. Those options support the legacy remainder player, but timestamp progress alone does not prove individual sample selection.

[FFmpeg filter documentation](https://ffmpeg.org/ffmpeg-filters.html#atrim) defines sample-count trimming separately from timestamp trimming; `end_sample` identifies the first excluded sample. Trimming does not reset timestamps, so the selected path resets them afterwards. [Resampler documentation](https://ffmpeg.org/ffmpeg-resampler.html) describes rate/layout conversion, filter behavior and timestamp compensation. Disable compensation explicitly for the chosen decoded-sample sequence. This is a declared media-clock convention, not proof that arbitrary container timestamps are continuous or accurate.

Select counted samples after conversion to the declared output rate. Checked endpoint ceilings make adjacent passages share exactly one boundary without double inclusion: `first = ceil(start * R / 1000000)`, `after = ceil(end * R / 1000000)`, `count = after - first`. A positive microsecond duration may contain zero sample onsets. Computing `ceil((end-start) * R / 1000000)` instead can invent one. Reject empty selections rather than invent audio.

## Independent qualification

Use a hand-generated 48 kHz stereo PCM source and independent expected samples. The quarter-second passage `[125000,375000)` contains 12,000 frames and 96,000 f32 bytes. Distinct samples immediately before and at the exclusive end expose off-by-one leakage; compare the complete output, not only duration or energy. Preserve the legacy 875 ms remainder oracle separately.

Test split scalar/channel boundaries, signed zero, finite out-of-range values, NaN/infinity, partial frames, early EOF, exact end, nonzero original timeline origin and fractional one-frame requests. These are independent input/output witnesses and malformed-input controls. A successful decoder must still agree with exact byte count, process exit and bounded progress.

Compressed codecs introduce priming and padding; a declared container/decoder profile needs its own reference and tolerance. Resampling uses a finite filter support, so a sharp excluded source sentinel can influence retained output values. A separate band-limited reference should measure other-rate content and timing rather than assuming sample equality with unresampled input. The first same-rate WAV oracle qualifies neither of those broader behaviors.

Finally, distinguish sample completion from native-process closure, callback presentation and original-file closure. The service hashes the full retained object before streaming and joins its actual blocking reader before releasing protection. A stopped encoded transfer after a successful excerpt is observable history, not a successful complete-object read. Parent-death, network isolation, blocked filesystem I/O and cold/contended capacity need separate real-host evidence. No external service fee, source survey or language qualification belongs to this boundary experiment.
