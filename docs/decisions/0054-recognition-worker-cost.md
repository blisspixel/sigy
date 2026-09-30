# Recognition worker cost

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC are v35. This is the empty-group measurement in roadmap operation 28 and increment 5 of the [scaling architecture](../design/scaling-architecture.md#increments). Claim order, pace, live classification, and the [queued recognition](0053-queued-recognition.md) report are unchanged. Host budgets remain open. No language is qualified. Amended the same day by [recognition arrival](0055-recognition-arrival.md): doctor compares admitted audio with completed busy time. These rows are not that comparison. Operation 28 is not exited.

## Decision

A recognition decode group and a recognition recognize group each drain on their own. The snapshot that proves the group empty is the one that is stored. A later `stats` call is a different observation and is not read.

On a Windows job object, peak committed memory is the job's peak commit charge, and total CPU time is the cumulative user and kernel time of every process that belonged to the job, including processes that have exited. Both fields are kept. `None` means that mechanism does not account for the field. A stored zero is a measured zero.

On Linux cgroup v2 those counters sum only members that are still alive. An empty cgroup therefore has neither number. The row is still stored, with both measures null. When no job-object group is complete, doctor counts how many groups had no job-object accounting. When a job-object figure exists, those rows are the groups left out of it. A POSIX process group and a FreeBSD process reaper do not account for either field, so their rows are null as well. A job-object row missing either measure is left out of the job-object figure. Those rows are not blended into the peak or the CPU sum, because a cgroup figure would be a different metric.

`sigy doctor` appends one sentence to the recognition check. The check stays ok, so `doctor --strict` does not fail because the cost is unmeasured. The sentence uses integers only. Local IPC moves with the catalog schema. The sentence stays inside the existing recognition string.

With no complete job-object group, the sentence is "Recognition worker cost is unmeasured on this library." When other groups were stored, it adds how many had no job-object accounting.

With one or more complete job-object groups, the sentence names that count, the highest peak committed memory in bytes, and the sum of their CPU time in microseconds. Omitted groups are counted in a separate clause. The sentence ends with "This is not a host budget."

The peak is the maximum of the complete peaks. CPU time is their checked sum. A count or a sum that overflows is a storage error. A measure that does not fit in a signed 64-bit integer is refused rather than truncated. A negative stored integer fails the read.

Each drained group is one immutable row, keyed by job, generation, role, and window ordinal. The roles are `decode` and `recognize`. The mechanisms are `job_object`, `cgroup_v2`, `process_group`, and `process_reaper`. Ordinals run from 0 through 1,023. Generations run from 1 through 64. The two roles may share an ordinal. Update and delete abort.

The rows are written in the same transaction as the move from running or cancelling to succeeded, failed, or cancelled, after a successful transcript is inserted and before the state update. An empty account list writes nothing. A job that never drains, including limits unavailable, a refusal before a group, and a spawn failure, has no row. If the drain itself fails, the attempt returns that error and no snapshot from that attempt is kept. A restart drops a snapshot that was not committed. Interrupted jobs have no rows. Exact replay of a finished job returns the stored job and does not write again, so the first observations remain when a replay carries different numbers.

A row whose job is still running, or whose job is not a terminal local recognition job of the same generation, fails the next catalog open. Admission, enqueue, and claim do not read the table. The rows do not set a slot count, a host CPU budget, a memory budget, a GPU budget, a load time, a real-time factor, or a catch-up multiple. No paid request is created.

Decode and recognize run one after another, so the higher of their peaks is the job's peak commit charge across those groups. The CPU figure adds historical counters. It is not a rate and it is not the host's current load.

Translation groups are drained, and that snapshot is discarded. A translation job lives in another table, so this recognition table does not reference it.

Backup copies the catalog, and restore brings the rows back. A v34 backup opened by this binary migrates to v35. An interrupted migration that finds the new table name already taken leaves the catalog at v34.

## Evidence

Unit tests fold two complete job-object rows to the higher peak and the CPU sum, and they leave a cgroup row and a job-object row that is missing CPU time out of that figure. One complete group and one process-group row stay in separate clauses. An empty library, one cgroup row, and two unaccounted rows say the cost is unmeasured, with the group count when rows exist. A CPU sum past the integer limit is a storage error.

A storage fixture finishes one recognition with a decode peak of 1,000 bytes and 10 us and a recognize peak of 5,000 bytes and 40 us. Doctor reports 2 groups, a highest peak of 5,000 bytes, and 50 us, and the check stays ok. Replay with different numbers leaves the first two rows. Update and delete abort. A failed decode keeps its one row and the job is failed. A stored peak of 9,000,000,000,000 bytes does not block the next claim. An unknown mechanism, a repeated decode ordinal, an ordinal of 1,024, and a peak above the signed 64-bit limit each roll the finish back, leave the job running, and write no row. A cgroup row with both measures null stays in the unmeasured sentence until a later job-object row moves it to the omitted clause. A row inserted on a still-running job fails the next open. A v34 catalog migrates to the current schema, keeps its transcript, and starts with no observation rows. A table of the same name created before the migration aborts the open and leaves user version 34.

One backup of a recognized chunk restores a recognize row of 4,096 bytes and 12 us.

On this Windows host, one contained `cmd /d /c exit 0` under a job object drained to an empty group. That snapshot reported `job_object`, a peak committed memory above zero, and a CPU time. Zero CPU time would have been accepted. The process is not a recognizer. The storage fixtures inject the structs the executor returns. They are not a whisper.cpp measurement and they are not a host budget.

On 2026-09-29, `cargo verify` passed 506 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder command line and HTTP acquisition are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix.

## Limitations

- The figure is not a host CPU, memory, or GPU budget, and it does not admit work.
- The figure is not the arrival comparison. [Recognition arrival](0055-recognition-arrival.md) uses completed busy time and does not read these rows.
- Committed memory excludes mapped model pages and GPU memory. GPU use stays off in this profile.
- CPU time across groups is a sum of historical counters. It is not a rate and it is not current load. Decode and recognize peaks stay separate. Doctor reports the highest of those peaks.
- An empty cgroup, process group, or process reaper stores no numbers. Those groups are counted apart from the job-object figure.
- Process-count peak and I/O counters are not stored.
- Translation snapshots are discarded.
- A restart drops a snapshot that was not committed with the terminal transition.
- The contained `cmd` process proves the empty job-object snapshot on this host. It does not measure whisper.cpp.
- Operation 28 is not exited. D-28 stays open. Host budgets remain.
