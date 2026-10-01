# Native task bounds

Date: 2026-09-30. Status: implemented and verified on Windows x86_64. This hardens the existing [task contract](0043-task-contract-and-job-pool.md) before local recognition or translation starts. It does not change catalog or IPC definitions, per-kind slots, paid policy, or immutable published history. Host resource admission remains open.

## Decision

The canonical task hash proves that fields agree with a digest. It does not by itself prove that a resource bound or execution parameter is valid. Both task sealing and the executor's preflight now validate those fields before resolving stage paths, preparing scratch space, reading assets, or creating a process group.

Every native task requests exactly one process. Its memory is within the existing profile range, 256 MiB through 64 GiB. Threads stay within the existing range of 1 through 64, and the whole-processor CPU quota must equal that thread count. Recognition deadlines remain 1,000 through 3,600,000 ms; translation cue deadlines remain 1,000 through 600,000 ms. Output caps must be positive and at most the existing 1 MiB recognition or 64 KiB translation cap. This prevents an overflowing `output_bytes + 1` read ceiling. Producers and validators use the same constants.

Recognition requires the existing whisper.cpp engine and fixed argument template, the known FFmpeg decoder template, and its 16 kHz sample rate. A recognition window stays within 30 seconds. Its optional offset must be positive, fit before its media-clock start, and add to the window duration without overflow. A supplied chunk ordinal stays within the existing 1,024-window bound. Decoder memory and wall limits must be positive and no larger than the existing 512 MiB and 60,000 ms limits. A missing decoder bound is refused.

Translation requires the existing llama.cpp engine and fixed HY-MT2 template, an English target, canonical declared profile languages, and no decoder stage. A recognizer's source label uses the same bounded lowercase block-label check as recognition output. An undeclared label can still take the existing `unsupported-language` path. An absent label can still translate; this validation adds no measured language claim.

The recognition decoder now requests `cpu_quota(1.0)` as well as one process and its memory bound. Its one-thread command-line settings remain in place. Libraries can create other threads, so the command-line thread setting alone was not an operating-system CPU cap.

Review also found unchecked addition when the parser converted a nonempty PCM sample count into a decoded end on an extreme media clock. That calculation now checks multiplication and addition and returns `InvalidOutput` on overflow, including empty transcript output. Cue offsets already used checked arithmetic. Pure parser and executor fixtures cover overflowing sample counts, near-maximum clocks and a representable extreme cue. This changes no valid task digest or transcript authority.

## CPU semantics reviewed

The pinned ProcessKit 3.3.4 source maps a quota to a Windows Job Object hard cap. It divides the requested processors by `available_parallelism`, scales by 10,000, rounds, and clamps to 1 through 10,000. This is an approximate rate limit. It is not a reservation or a statement that those processors are free. [Group options](https://docs.rs/processkit/3.3.4/processkit/struct.ProcessGroupOptions.html), [Windows setup, lines 516 through 529](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/sys/windows.rs#L516), [conversion, lines 2156 through 2161](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/sys/windows.rs#L2156).

Windows stops a job's threads for the remainder of a scheduling interval after its hard cap is reached. A rate in a nested job is relative to a parent whose CPU rate is controlled; otherwise it is relative to the system. Processor-count estimation has platform and affinity limitations. These facts prevent treating the requested quota as measured host capacity. [Microsoft CPU-rate contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information), [Rust processor-count estimate](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html).

## Compatibility and evidence

No native `TaskSpec` JSON is persisted in the catalog. Runtime specs still use `sigy-task-spec-v1`, the same fields and declaration order, and the same optional chunk-field defaults. Valid specs retain their canonical digest. The existing golden recognition fixture remains `661c0d90c83132c2e1fb7e5f7b6264c0268a401f989440c8bcb2889fbb5a9e08`.

Focused regression fixtures rebuild the digest of malformed tasks, serialize and parse them, and require both validation and sealing to refuse them. Cases cover zero, overflow, expanded limits, inconsistent CPU and threads, unknown engine and template values, decoder requirements, sample rate, window and offset bounds, translation target and language declarations. Profile minimum and maximum boundaries still produce valid specs. A staged malformed task leaves a sentinel unchanged and returns no group accounts. A decoder-options fixture checks process, memory, and CPU bounds together. Existing recognizer-label fixtures use the shared label check without changing their accepted labels.

On this Windows host, `cargo verify`, `cargo verify-media` and `cargo verify-coverage` pass, including the validation fixtures, unchanged golden serialization, sample-time overflow regressions, warnings-denied Clippy and all 15 native fixtures. [Active work](../development/progress.md) records exact test and coverage counts. Unit fixtures exercise the contract; they are not native capacity measurements.

## Remaining limits

- Recognition and translation can still overlap under their independent one-job slots. Their combined CPU and memory reservations are not admitted against a host budget.
- A 64 GiB profile ceiling is a configuration bound, not available memory. Job committed-memory accounting excludes mapped model pages and GPU memory. [Worker cost](0054-recognition-worker-cost.md) remains historical evidence.
- Linux native execution still fails closed without a working delegated-cgroup boundary. This change supplies no delegated topology or Linux task-versus-process enforcement. Follow the [Linux source audit](../../research/experiments/native-boundary/delegated-cgroups.md).
- No native network isolation, language qualification, simultaneous capacity, or supported platform matrix is established here.
