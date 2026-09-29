# Live recognition deadline

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC stay v34. This is the live-classification part of roadmap operation 28 and increment 5 of the [scaling architecture](../design/scaling-architecture.md#increments). It uses the pace from [recognition pace](0051-recognition-pace.md) and the rotation from [fair claim order](0050-fair-claim-order.md). Host budgets remain open. [Queued recognition](0053-queued-recognition.md) reports the processing time of audio already waiting at this pace. No language is qualified.

## Decision

A queued recognition job is live at the moment of a claim when every one of these holds:

- At least one unpaused monitor follows its source revision. A paused monitor does not count. One unpaused monitor is enough when another monitor of the same source is paused. Followed sources are the latest version plus applied add and remove actions. The claim query does not parse a monitor specification.
- The job's profile has a pace in the same newest 256 succeeded recognition jobs that `sigy doctor` uses. Profiles the doctor report does not name still count. A profile absent from that sample stays batch. A pace of zero is a measurement, meaning under 1 ms of wall time per audio second, and is not treated as missing.
- The recording has a seal. The seal is the latest row in `recording_segment_clocks`. A recording with no seal stays batch.
- The pin has retained audio. Gaps are not audio. A pin with no retained audio stays batch.
- The current time is at or before the deadline.

The deadline is the last seal plus the retained audio at that profile's pace. The product is rounded up to the next millisecond when it is not exact. A pace of zero adds no time, so the deadline is the seal, and a job queued after publication is then batch. If the product or the sum does not fit in the clock integer, the job stays batch.

Verification and translation stay batch. They have no recognition pace.

A live job is ordered by that deadline, then by enqueue time. A batch job is ordered by enqueue time, as before. Every fourth claim is still the oldest waiting batch job, including while a live job is waiting. Nothing about the class or the deadline is stored. A restart recomputes both from the catalog. The rotation cursor still lives in the service process and still resets.

The profile's 600-second deadline stays the worker safety cap. It is not this window.

The pace figure is already rounded down to a millisecond per audio second. This deadline uses that integer, then rounds the product up. It does not restore the fraction the pace figure dropped. It is not a host CPU, memory, or GPU budget. It does not compare an arrival rate, and it does not say how long a backlog will take. No paid request is created.

## Evidence

Unit tests cover an unfollowed source, a profile with no sample, a pin with no audio, a missing seal, a product that overflows, a deadline that does not fit, 1.5 seconds at 1 ms per audio second rounding up to 2 ms, an exact 3000 ms product, and a zero pace whose window closes at the seal. A nine-profile sample still carries a pace for the profile the doctor report only counts.

A storage fixture completes one recognition in 3600000 ms of wall time on one second of audio, so the pace is 3600000 ms per audio second. An older job on an unmonitored source stays first. After a monitor follows the newer source, a queued job on that source whose profile was not in the sample still stays behind, and the monitored job with the measured profile is first. It remains first at the exact deadline, one hour after its seal, and the older job is first one millisecond later. Pausing the only monitor restores arrival order. Creating a second, unpaused monitor of the same source makes the monitored job first again. The reserved fourth claim still takes the older batch job while the live job is waiting. A verification job on the same library stays batch. Asking for the oldest queued recognition, which closes any finite window, returns the older job. The classification only reads rows that already exist.

On 2026-09-29, `cargo verify` passed 483 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder and acquisition path are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix.

## Limitations

- Host CPU, memory, and GPU budgets are not applied. Caps stay one job of each kind.
- This deadline is not compared with an arrival rate. [Queued recognition](0053-queued-recognition.md) reports the processing time of audio already queued.
- The newest 256 jobs can leave an older run out, so a profile with only older completions stays batch.
- A pace of zero closes the window at the seal.
- The seal is the system clock at publication.
- One library's pace is not a capacity for another host or for a profile that has not completed work here.
- Operation 28 is not exited. The three-station fixture remains.
