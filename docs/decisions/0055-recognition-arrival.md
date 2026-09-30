# Recognition arrival

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC stay v35. This is the arrival comparison in roadmap operation 28 and increment 5 of the [scaling architecture](../design/scaling-architecture.md#increments). It reads admitted local recognition jobs and the busy totals behind [recognition pace](0051-recognition-pace.md). It does not read [recognition worker cost](0054-recognition-worker-cost.md). The [queued recognition](0053-queued-recognition.md) report is unchanged. Host budgets remain open. Nothing is stored. No language is qualified. Operation 28 is not exited.

## Decision

`sigy doctor` inserts one sentence after the queued-recognition sentence and before the worker-cost sentence. The check stays ok, so `doctor --strict` does not fail because the comparison is unmeasured. The sentence uses integers only. Nothing is stored, so catalog schema and local IPC stay v35. No command is suggested.

The arrival sample is the newest 256 local recognition jobs of any state, ordered by admission time and then by id. Audio is the retained audio on each pin. Gaps are not audio. A job with no retained audio counts in the job total and adds no audio. One job is one row, so a restart is not a second arrival and the admission clock does not move. Translation and verification are omitted. When more than 256 local recognition jobs exist, the sentence says the sample is the newest 256.

The span is the latest admission clock in that returned sample minus the earliest. A library with no local recognition jobs, a sample whose clocks are identical, or a sample whose retained audio is zero stays unmeasured. A negative admission clock is a storage error. The unmeasured sentence is "Recognition arrival is unmeasured on this library."

Completed work is the same sample the pace report uses: the newest 256 succeeded local recognition jobs with a positive busy wall time and positive retained audio, before per-profile rounding, including profiles the pace sentence does not name. The totals are the job count, the audio, and the busy wall time from claim to finish. Idle time between jobs is not included. An empty sample, or a sample whose audio or busy wall time is zero, has no completed work.

When arrivals exist and completed work does not, the sentence gives the arrival totals and says arrivals are not compared. It says the figure is not a host budget.

When both samples exist, arrival audio is multiplied by busy wall time, and the arrival span is multiplied by completed audio. Both products use checked arithmetic. A sum that overflows is a storage error. The product of the two 64-bit factors fits in a wider integer, the largest pair still compares, and the multiplication is checked so it is not wrapped. The larger product says which sample brought more audio per wall millisecond. Equal products say the same. The sentence names both samples by their own totals. It does not print a ratio or a drain time. It ends with "This is not a clock time for an empty queue and it is not a host budget."

The two samples are different populations. A job can be in both. Busy time excludes idle gaps, so a comparison that favors completed work is not a promise that the queue shrinks.

Admission, enqueue, and claim do not read the figure. No paid request is created.

## Evidence

Unit tests keep one clock, identical clocks, and zero audio unmeasured, and they reject a negative admission clock. Arrivals without completed work are not compared, and a sample past 256 jobs says so. Equal, greater, and lesser products select the same, more, and less audio per wall millisecond without rounding. An audio total past the integer limit is a storage error. The largest pair of 64-bit factors compares as equal. The busy totals sum the pace sample, and an empty sample has no busy work.

A storage fixture finishes one recognition in 1 ms of busy wall time on 1 second of audio, then queues a second job 1,000 ms later on the same pin. Doctor reports 2 jobs and 2,000,000 us over 1,000 ms, against 1,000,000 us in 1 ms across 1 job, and says admissions brought less audio per wall millisecond. The check stays ok and suggests no command. Claiming the queued job leaves that comparison in place.

The three-station fixture admits four recognition jobs and 3,500,000 us over 100 ms, against the completed job's 1,000,000 us in 3,600,000 ms. Doctor says admissions brought more audio per wall millisecond. That completed interval is the one-hour storage clock that keeps the queued jobs live at the seal. It is not a measured recognizer pace. The queued translation is omitted. The disconnect gap is not audio. Publishing further recordings does not change the sentence. No paid request is created.

On 2026-09-29, `cargo verify` passed 512 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder command line and HTTP acquisition are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix.

## Limitations

- The comparison is not a host CPU, memory, or GPU budget, and it does not admit work.
- The sentence does not give a time when the queue will be empty, and it does not say that the queue cannot catch up.
- The arrival window and the pace sample are different populations. A job can be in both.
- A zero span is unmeasured, including when the newest 256 jobs share one clock.
- Busy time excludes idle gaps, so matching completed work is not a promise that the queue shrinks.
- The three-station result uses the synthetic one-hour storage clock. It is not a measured recognizer pace and it is not three public stations.
- Worker-cost rows are not the completed-work side of this comparison.
- Translation and verification are omitted.
- Admission, enqueue, and claim ignore the figure.
- Linux delegated-cgroup containment and the suspended-spawn assignment window remain deferred.
- Operation 28 is not exited. D-28 stays open. Host budgets remain.
