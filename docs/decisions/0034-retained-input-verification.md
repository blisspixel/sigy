# 0034: Supervised verification of retained analysis input

Date: 2026-09-22. Status: implemented; validation is recorded in [progress](../development/progress.md). This is input preparation, not recognition.

## Decision

`analysis verify JOB --input PIN --revision N` admits one local checksum job on the existing service supervisor. `analysis job JOB` reads its durable state. `analysis cancel JOB --generation G` requests cancellation of that exact worker generation. The worker receives only retained object keys, byte counts, and hashes from a currently published pin. It receives no source URL and dispatches no decoder, model, or network request.

One worker may run, with no waiting queue and at most 256 immutable job records. Admission bounds input to 512 MiB, 1024 files, and a 64 KiB manifest. Reading uses a 64 KiB buffer and checks cancellation and a 60-second deadline between reads. A blocked filesystem call can delay those checks. This is a cooperative deadline, not a hard operating-system I/O deadline. The slot, read lease, and library ownership remain held until the read actually returns. These limits do not qualify future native inference isolation.

The immutable request binds the pin revision, recording, profile, expected bytes/file count, and manifest checksum. Successful completion must match those values and the current published pin. Every job records exactly zero USD; there is no paid reservation or fallback. Exact replay returns historical state without dispatch, including after retention expiry. Reusing an ID for another input fails.

Running and cancelling jobs protect their recording from whole-file deletion, segment release, age pruning, processing-receipt cleanup, and quota pressure. Cancellation accepted before result publication wins over a queued success. The worker holds the library lock inside its blocking closure, including when its asynchronous supervisor is aborted. Restart increments the generation and records interrupted work before recovering deletions. It never resumes the read automatically.

Expected input errors fail only the job. A terminal catalog failure keeps the durable read lease and stops admission. The already-finished worker leaves its in-memory slot so shutdown can drain; restart then records interruption. A stale completion cannot clear the current worker. Replacing an unpublished analysis pin retires the old revision and inserts its successor in one transaction, so a failed insert leaves the previous pin publishable.

Deleting the last retained segment uses the existing staged whole-recording deletion. Earlier segment release receipts remain. The last file is represented by the recording's deleted state, preserving positive published byte history and avoiding an invalid zero-byte retained row.

## Transcript honesty

`analysis transcribe` now fails before admitting work because no measured recognizer is configured. It cannot create more empty transcript rows. Existing `local-unmeasured` revisions remain readable and are labeled as legacy placeholders with no recognized speech. [Decision 0032](0032-local-transcripts.md) records that historical persistence experiment. Actual recognition remains an open operation 23 gate.

## Verification and consequences

Local tests cover real checksum reads, corruption and missing files, bounded manifests, cancellation and deadline checks, active blocking work, supervisor abort and panic, exclusive ownership, replay and conflicting admission, stale completions, restart, retention exclusion, spawn failure, and terminal-transaction rollback. A native-media fixture exercises the CLI and service against retained decoded audio and proves that transcription remains unavailable. Refer to progress for commands and outcomes, rather than treating this list as a platform qualification.

Catalog schema is v25 and local IPC is v26. Stop an older service before replacing its binary. Model assets, inference, language quality, GPU capacity, native resource isolation, and translation remain unqualified. Continue the [language pipeline](../development/language-pipeline.md) with a measured runtime boundary and reproducible evaluation.
