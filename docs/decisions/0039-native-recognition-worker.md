# Native recognition worker

Date: 2026-09-24. Status: implemented and tested on Windows x86_64. This decision supersedes the "no production constructor" and "recovery refuses active native rows" parts of the [recognition storage API](0036-recognition-storage-api.md). Language quality is not qualified by this record.

## Decision

`analysis transcribe` runs one pinned local recognizer on one published analysis pin, supervised by the service. The user chose on 2026-09-24 to run hash-checked native recognizers under enforceable process bounds now and to record full operating-system network isolation as a limitation, not a precondition. See the [language pipeline decisions](../development/language-pipeline.md#2026-09-24-decisions).

## Profiles

`analysis profile add` hashes a local runtime directory, a model file and a speech-activity model into an immutable profile (catalog v27). The runtime hash covers every regular file directly inside the directory, in name order; subdirectories are ignored and links or reparse points are refused. The profile identity covers the engine, its fixed argument template, every file hash and size, the thread count, the committed-memory ceiling and the wall deadline. It does not cover file locations, so moving the same files keeps the identity. A second name for the same identity is refused. Profiles cannot be updated or deleted. At most 64 exist.

The only engine is `whisper-cpp-cli-v1`: `-l auto`, `--vad` with the profile's speech-activity model, `--no-gpu`, one processor, JSON output. A speech-activity model is required because the [three-clip calibration](../../research/experiments/local-asr/three-clip-calibration.md) showed text invented for digital silence without one.

The profile is a configuration of local executables, like the decoder path. It is available through the CLI and local IPC, which already require the same user. It is not exposed through `sigy mcp`.

## Execution

The service admits the job through the existing single analysis worker slot and durable read lease, then:

1. Hashes the retained input file and compares it with the pinned interval.
2. Re-hashes every profile file. Any change fails the job as `profile-unavailable` before a process starts.
3. Decodes the interval with the configured FFmpeg into 16 kHz mono PCM16, reading at most one second more than the pinned duration. FFmpeg runs in its own contained group (one process, 512 MiB committed memory, 60 s deadline) with the existing pipe-only protocol whitelist.
4. Writes that PCM as a private WAV in `analysis-scratch/<job>-g<generation>` inside the library.
5. Runs the recognizer in a contained group: a process-count limit of one, the profile's committed-memory ceiling, a CPU rate equal to its thread count, kill-on-close, the profile's wall deadline and cancellation. The environment is cleared except for the runtime search path (and `SystemRoot` on Windows). Standard streams are discarded; the only output read is one JSON file of at most 1 MiB.
6. Waits until the group reports no active process. Only then can the service construct the completion capability. If that cannot be shown within five seconds, the completion is not constructed, the lease is kept and the service stops; restart then interrupts the job.
7. Parses the JSON as untrusted data and publishes through the existing atomic transaction with a zero-USD decision.

Containment uses ProcessKit 3.3.4: a Windows Job Object with kill-on-close, or Linux cgroup v2. Where the host cannot enforce the limits, as on the macOS process-group mechanism, the job fails as `limits-unavailable` without starting a process.

## Output mapping

Offsets are milliseconds from the decoded interval start and are added to the pinned media clock. Surrounding whitespace is trimmed; a segment with no remaining text is not a cue. A segment end past the pinned interval end is bounded to that end, because the recognizer rounds to its own frame grid; this is the only adjustment. A negative, empty, reversed, overlapping or late segment, NUL text, more than 256 cues, more than 4,096 bytes in one cue, more than 65,536 text bytes, invalid UTF-8 or malformed JSON rejects the whole result as `invalid-worker-output`. Wording remains `uncertain`. A result with no cues is published as `no_text` coverage; it does not assert silence or a language.

## Recovery and replay

Exact replay of a job ID returns the stored job and never reruns the recognizer or selects a new model. A changed request under the same ID is refused. An omitted parent revision resolves to the current transcript revision at first admission. Cancellation stops hashing between reads, or ends the decoder or recognizer group, and the job becomes `cancelled`. On restart, running and cancelling recognition jobs become `interrupted` with a new generation, so a late completion from the old process is refused as stale, and stale scratch directories are removed. On Windows, kill-on-close ends the previous service's recognizer with that service; the media test observes this. On Linux this relies on the cgroup being torn down with the service and is not yet tested.

## Evidence

The native-media fixture drives the real CLI, service, FFmpeg and containment with a fault-injecting stand-in recognizer (`sigy-test-recognizer`, never installed). It covers published text on the media clock with a bounded end, exact and conflicting replay, `no_text`, malformed, overlapping and oversized output, a failing exit, a deadline, a refused grandchild process, a refused 4 GiB allocation under a 256 MiB ceiling, a changed profile file, cancellation, and a killed service whose recognizer does not outlive it followed by interrupted recovery. The environment check in the stand-in confirms that the test runner's variables do not reach the recognizer. Unit tests cover the output parser, profile identity, file-boundary checks, cancellable hashing and scratch cleanup.

A manual run on this host used whisper.cpp b5130 with `ggml-large-v3-turbo-q5_0` and the Silero VAD through the installed CLI: FLEURS Spanish and Arabic calibration recordings produced original-script text matching the calibration harness, and a silence recording produced `no_text`. This is a smoke run, not a language benchmark.

## Limitations

- No operating-system network denial is enforced. The runtime's static imports include no networking library, but that does not cover dynamic loading.
- ProcessKit creates the child suspended and then assigns it to the job. A service death in that window can leave a suspended child outside the job.
- Committed memory excludes mapped model pages and any GPU memory. GPU use is disabled in this engine template.
- Profile files are hashed before launch and could change between the hash and the load.
- One interval of at most 60 seconds per job; longer recordings are not yet chunked.
- Hashing a large model takes seconds on each run.
- Language evidence from the recognizer is not yet stored.
- There is no quality, capacity or platform claim.
