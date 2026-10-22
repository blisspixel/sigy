# 0108: Durable completion hold

Date: 2026-10-22

Status: implemented publication hold for one recognition or translation finish. Schema is v53. Local IPC stays v54. Process exit before the hold commit still drops the outcome. S-03B stays open. Containment qualification, reconstruction of a worker envelope, and a stage exit are not claimed.

## Decision

[Decision 0097](0097-unresolved-completion-hold.md) keeps the drained outcome in memory and leaves the `running` or `cancelling` job row as the durable lease. [Decision 0098](0098-completed-settlement-slots.md) retries that live slot. This increment commits the publication body before the first finish, in its own immediate transaction. The job row stays `running` or `cancelling`. Original-reader protection follows that row, as before.

The table is `completion_holds`. Family is `analysis` or `translation`. The primary key is family and job id. The row stores the committed generation, an attempt from 1 to 3, and the lease owner. The body is 2 to 262,144 bytes of canonical JSON at document version 1, with the lowercase SHA-256 of those bytes. An update is refused. A delete is allowed. The body is the input the existing finish path already accepts. Restore builds that publication input. It does not construct `ReapedLocalAsr` or `TranslationOutcome`, and it does not treat the row as containment evidence.

A recognition body carries the outcome, profile and manifest hashes, coverages, cues, languages, and observations. A successful translation body carries the translated cues. A failed or cancelled body omits that success payload. The same bytes commit again as the same row. A different hash is a storage-integrity fault and leaves the stored row in place.

On restart, recovery leaves a hold that matches the running or cancelling claim. It does not write `job_attempts`, requeue, or interrupt that job. A hold for a different claim is deleted, and the existing recovery policy then runs. The actor loads a readable hold into a completed slot before schedule reconciliation. A failure to load stops the service the same way a failed reconcile does. Finish uses the existing publication checks and deletes the hold inside that transaction. An unreadable body, or a running claim whose input can no longer be resumed, fails that job with reason `held-completion-unreadable` and inserts no cue. A job that is already terminal only loses the hold. A resumed claim that disagrees with the hold stops the service.

The live slot keeps the Decision 0098 budget: three tries and 15 seconds on the existing five-second catalog lock wait. A busy or locked hold write spends one try and does not call finish. Uncertain I/O reads the row again. The same body may finish. A different body stops the service and leaves that row. An absent body waits without spending another try. A changed claim is `stale-worker`. A row that is already terminal releases the slot. The deadline drops the in-memory slot and leaves the running or cancelling job. A refused finish deletes the matching hold, then stops the service. Exhaustion drops the in-memory slot. A hold that already committed stays. The next start receives a fresh slot budget, starts no recognizer or translator, and does not increment `job.attempt`.

Death after the hold commit and before finish leaves the row and the running or cancelling job. The next start publishes that body through the existing checks. Death before the hold commit, or an uncertain hold commit that did not land, still drops the outcome. A permanently busy catalog can retry that publication across restarts. A permanent refusal has deleted the hold, so the next start does not publish it again.

Backup copies the catalog, so the table is included. No IPC message changes. Doctor does not name the hold. No command-line or MCP surface is added.

## Alternatives

Rebuilding `ReapedLocalAsr` or `TranslationOutcome` from catalog bytes was rejected. Those values are constructed from an executor envelope. A second transcript table, a hash-only marker, and a wider `job_attempts` row were already rejected by Decision 0097. This hold stores the publication body those finish checks already accept, and it supersedes the no-payload choice only for that body.

## Evidence

`cargo verify` exited 0 on this Windows host. It passed vendor and script checks, the changelog heading check, formatting, warnings-denied Clippy, workspace tests, the build, and `cargo audit --deny warnings` against 1349 advisories and 382 dependencies. The `sigy` binary tests passed 381, with 7 ignored, in 27.44 seconds. The `sigy-service` library tests passed 756, with 2 ignored, in 188.88 seconds. The command completed in 574.17 seconds. These service tests ran on Windows, where a running recognition without a hold is requeued. The non-Windows interrupt branch is present and was not executed on this host. Coverage was not run. `cargo verify-media` was not required. `cargo visual-qa` was not run. External spend remains USD 0. This receipt was written after those commands.

## Limits

Process exit before the hold commit still drops the outcome. S-03B stays open. The hold does not prove native drain, scratch removal, or containment. Language, platform, and acoustic qualification are unchanged. Issues 13 and 14 stay open. External spend is USD 0. This slice does not change recording, playback, or HTTP acquisition, so `cargo verify-media` is not required.
