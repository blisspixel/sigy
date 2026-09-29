# Recognition pace

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC stay v34. This is the measurement part of roadmap operation 28 and increment 5 of the [scaling architecture](../design/scaling-architecture.md#increments). It does not change the rotation in [fair claim order](0050-fair-claim-order.md). Amended the same day by [live recognition deadline](0052-live-recognition-deadline.md): this pace is the delay used when a monitored recognition job is classified live. Admission and a backlog prediction still ignore it. Host budgets and a catch-up report remain open. No language is qualified.

## Decision

`sigy doctor` reports the pace of completed local recognition on this library. The check stays ok whether or not a pace exists, so `doctor --strict` does not fail for an unmeasured library.

A pace is the total wall time of the newest 256 succeeded recognition jobs, divided by the retained audio on their pins, rounded down to a millisecond per second of audio. Wall time starts when the successful attempt is claimed and ends when it finishes. Time spent queued is not included. Gaps on the pin are not audio. Failed, cancelled, interrupted, and verification jobs are omitted. A job with no positive duration is omitted. At most eight profiles are named, the ones with the most retained audio first, and any further profiles are counted. When more than 256 jobs qualify, the report says it used the newest 256.

The sentence ends with "Observed on this library." The figure is those jobs. It is not a host capacity, not a real-time factor for another machine, and not a prediction of how long a backlog will take. The doctor check does not admit work. [Live recognition deadline](0052-live-recognition-deadline.md) reads this same pace, including profiles this report does not name, when it classifies a monitored recognition job. No paid request is created.

## Evidence

Unit tests cover an empty set, a zero-duration run, two jobs on one profile at 6000 ms of wall time per audio second, a separate profile under 1 ms, and a ninth profile that is counted rather than named. A storage fixture whose successful attempt spans 1 ms and whose pin holds 1 second of audio reports 1 ms of wall time per audio second. A fresh library's doctor check says the pace is unmeasured and does not suggest a command.

On 2026-09-29, `cargo verify` passed 479 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder and acquisition path are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix.

## Limitations

- The pace is not used for admission or a deficit report. [0052](0052-live-recognition-deadline.md) uses it to classify a monitored recognition job at claim time.
- Translation jobs are not included.
- The newest 256 jobs can leave an older run out of the figure.
- Rounding is down to 1 ms per audio second. A faster result is reported as under 1 ms.
- One library's jobs are not a measurement of another host, another device, or a profile that has not completed work here.
