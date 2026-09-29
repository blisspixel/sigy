# Queued recognition

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC stay v34. This is the queue-report part of roadmap operation 28 and increment 5 of the [scaling architecture](../design/scaling-architecture.md#increments). It reads the pace from [recognition pace](0051-recognition-pace.md). Host budgets remain open. An arrival-rate comparison remains open. No language is qualified.

## Decision

`sigy doctor` appends one sentence to the recognition check. The check stays ok, so `doctor --strict` does not fail because recognition is waiting.

The sentence counts local recognition jobs that are running, stopping (`cancelling`), or queued. Verification and translation are omitted. A running job and a stopping job are counts. Their remaining audio is not estimated.

Queued audio is the retained audio on those pins. Gaps are not audio. A job with no retained audio adds to the job count and adds no wall time. For each profile that has a pace in the same newest 256 succeeded recognition jobs, including profiles the pace sentence does not name, that profile's queued audio is multiplied by the pace and rounded up to the next millisecond once. A pace of zero is reported as under 1 ms of wall time per audio second. Audio whose profile is absent from the sample is reported as having no measured pace, and those microseconds stay out of the wall-time total. The pace figure is already rounded down. This product uses that integer and rounds up. It does not restore the fraction the pace figure dropped.

The profile totals are added. That matches one recognition job at a time. The sum is the processing time of audio already queued, at the observed pace on this library. It is not a clock time when the queue will be empty. New audio can arrive, and claims still rotate across sources. The figure is not compared with an arrival rate. It does not set a host CPU, memory, or GPU budget, and it does not admit work. Nothing is stored. No paid request is created.

## Evidence

Unit tests cover an empty queue, one and two running jobs, one stopping job, a running job beside queued audio, 1.5 seconds at 1 ms per audio second rounding up to 2 ms, two 1-microsecond jobs on one profile rounding once to 1 ms, the same split on two profiles rounding to 2 ms, a zero pace kept apart from a missing pace, a job with no retained audio, a product that overflows, and both joins onto the pace sentence.

A storage fixture cancels one running recognition and reports it as stopping, then completes a second recognition in 2 ms of wall time on one second of audio. One queued job on that profile is 1 second of audio and 2 ms of wall time. A queued verification job leaves the sentence unchanged. A second queued job whose profile is outside the sample keeps its audio in the unmeasured part. Cancelling that queued job removes it. The report only reads rows that already exist.

On 2026-09-29, `cargo verify` passed 490 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder and acquisition path are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix.

## Limitations

- Host CPU, memory, and GPU budgets are not applied. Caps stay one job of each kind.
- The figure is not compared with an arrival rate, so it does not say whether waiting audio is growing faster than this pace.
- A running or stopping job contributes no remaining-audio estimate.
- The newest 256 completed jobs can leave an older run out, so a profile with only older completions has no measured pace.
- One library's pace is not a capacity for another host or for a profile that has not completed work here.
- Operation 28 is not exited. The three-station fixture remains.
