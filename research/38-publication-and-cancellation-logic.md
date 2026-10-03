# Exact publication and shared cancellation logic

Reviewed: 2026-10-03. Status: source inspection and proposed implementation reasoning for S-02 and S-03. No builds, tests, model checker, native worker experiment or formal proof were performed for this note. Existing fixtures below are inspected evidence, not newly executed results. The current Windows verification baseline is recorded in [active work](../docs/development/progress.md).

The narrow next improvement is to publish a bounded snapshot of a task's own evidence, then let a task withdraw its interest in shared processing without cancelling another owner's work. Neither package needs a planner, broker, new database or memory framework. The proposed contracts remain subject to the [implementation plan](../docs/development/reliability-and-scale.md) and [task workflow design](../docs/design/task-workflows.md).

## Current source authority

Only the root [repository instructions](../AGENTS.md) were found. The service owns the catalog and mutations. Source inspection takes precedence over older prose.

| Owner | Inspected behavior |
| --- | --- |
| [Run admission](../crates/sigy-service/src/storage/tasks/run/admission.rs) | One lifetime publication run per task. Identical request/spec replays the stored run, including after policy drift; changed intent conflicts. Fresh admission validates a stored broad monitor checkpoint and creates grant/intents in an immediate transaction. |
| [Run advance](../crates/sigy-service/src/storage/tasks/run/advance.rs) | Expected run generation, frozen grant, current scope and clock checks fence progress. A canonical finding and its receipt commit together. Expected citation refusal becomes a durable skip through a savepoint. Integrity faults propagate. Briefing membership consists exactly of committed run findings. |
| [Run types](../crates/sigy-service/src/task/run.rs) | The finite `literal-briefing-v1` template permits at most 64 finding attempts. Completed, partial, cancelled and revoked are distinct states. Finishing this template does not establish semantic understanding. |
| [Processing admission](../crates/sigy-service/src/storage/tasks/processing/admission.rs) | Canonical job, exact task receipt, immutable interest and lifetime audio charge commit together before scheduling. Sharing avoids another physical job but each authority consumes its own finite allowance. Exact replay adds neither charge nor interest. |
| [Processing grants/cancellation](../crates/sigy-service/src/storage/tasks/processing.rs) | Existing cancellation fences future task admissions and freezes committed step membership. It does not withdraw interests or cancel admitted jobs. One task grant has zero paid allowance and finite audio allowance. |
| [Evidence reconciliation](../crates/sigy-service/src/storage/tasks/evidence.rs) | Reads exact collected occurrence/recording and task processing receipts, then the actual recognition/translation job's published revisions. It is separate from the broader checkpoint path. Current matching collects selected cue rows before applying the citation-page cap; a result cap does not bound query work or allocation. |
| [Interests](../crates/sigy-service/src/storage/interests.rs) and [v44 schema](../crates/sigy-service/src/storage/044-task-processing.sql) | Admission interests are immutable. Sharing counts historical rows. Direct authority has an empty owner key, so distinct direct callers are not independently cancellable. Migration cannot reconstruct every earlier direct interest; absence is not proof of absence. Reopen audits bind jobs, receipts and interests. |
| [Recognition cancellation](../crates/sigy-service/src/storage/recognition/jobs.rs), [publication](../crates/sigy-service/src/storage/recognition/publish.rs) and [translation storage](../crates/sigy-service/src/storage/translations.rs) | Queued cancellation ends a job before launch. Running cancellation sets `cancelling` at the same generation. Finalization checks generation and current state in an immediate transaction; cancelling wins over successful output. Recognition additionally validates exact input, manifest and parent revision. Current translation storage publishes target `en`. |
| [Analysis actor](../crates/sigy-service/src/control/actor/analysis.rs) | Existing explicit analysis cancellation reaches the canonical job supervisor. Completion capabilities and actual worker cleanup remain prerequisites for finalization/resource release. This administrative command is separate from proposed task-interest withdrawal. |

There are different generations for publication runs, processing cancellation and physical job attempts. A state transition to `cancelling` is already a publication fence even when job generation does not change. Avoid describing all fences as generation increments. A stale callback can arrive later than the deciding transaction; callback arrival time is not the linearization point.

## Primary-source facts and implications

SQLite serializes writers; `BEGIN IMMEDIATE` obtains a write transaction or reports busy. Readers retain their transaction snapshot. Some full-disk, I/O, interrupt and memory errors can roll back a statement or the whole transaction. Savepoints, rather than nested `BEGIN`, support bounded effect refusal within a larger operation. Recommendation: freeze and admit in one short transaction, propagate integrity/transaction faults, and resolve uncertain outcomes by durable request identity before retrying. Keep native execution, media reads and network calls outside that transaction. [SQLite transaction control](https://www.sqlite.org/lang_transaction.html), [SQLite isolation](https://www.sqlite.org/isolation.html), reviewed 2026-10-03.

Caller request identifiers can distinguish retries from new intent. Reusing an identifier with different parameters should conflict; a late retry should receive the original semantic result. Recommendation: retain Sigy's existing exact request/spec replay rather than recomputing from today's revisions or media. This is an application contract, not an assurance that external execution occurs exactly once. [Making retries safe with idempotent APIs](https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/), reviewed 2026-10-03.

State-machine specifications and checking finite models can expose ordering mistakes and distinguish invariants from eventual progress. Recommendation: use a small independent Rust reference transition model first. TLA+ is an optional later specification technique, not a required dependency or a proof already obtained. [Specifying Systems](https://lamport.azurewebsites.net/tla/book.html), reviewed 2026-10-03.

SQLite's testing methodology includes injected I/O errors, crash simulations and compound recovery failures. Recommendation: extend Sigy's existing transaction/reopen fault fixtures with failure at each new manifest/withdrawal boundary. Application fault injection does not qualify filesystem power-loss behavior or NAS durability. [How SQLite is tested](https://www.sqlite.org/testing.html), reviewed 2026-10-03.

## Encoding and query interruption

RFC 8785 describes an informational JSON canonicalization scheme. It disallows duplicate properties, constrains numeric values to double precision, recommends strings for longer integers, and preserves Unicode strings without normalization. Recommendation: evaluate any new cross-language manifest encoding against exact integer and original-script requirements, with frozen byte fixtures. Do not adopt it implicitly or rewrite old hashes. Current [task storage](../crates/sigy-service/src/storage/tasks.rs) hashes stored typed serialization with versioned domain prefixes; the new snapshot encoding remains an explicit implementation decision. [JSON Canonicalization Scheme](https://www.rfc-editor.org/rfc/rfc8785.html), reviewed 2026-10-03.

The pinned database binding exposes a safe progress callback that can interrupt queries, gated by its `hooks` feature. The [workspace manifest](../Cargo.toml) currently enables only `bundled` for this binding, so the callback is not an implemented query bound. Recommendation: review the feature change and complete locked dependency graph, then qualify scoped interruption, transaction rollback and cleanup before using a finite VM-work budget. Approximate instruction callbacks do not impose a hard deadline on blocked filesystem calls or bound text already allocated. No dependency feature changed in this research increment. [Connection progress callback](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Connection.html#method.progress_handler), reviewed 2026-10-03.

## S-02: freeze one exact bounded result

The following predicates are recommendations, not implemented schema. Let `T` be a task, `S(T)` its immutable accepted scope, `M` a manifest, `R` a run and `g` its expected generation. Let `receipts(T)` mean exact owned processing receipts, not all jobs or recordings sharing the monitor's sources.

```text
Exact(M,T) :=
  M.scope_digest = S(T).digest
  and every M.recording follows T's exact collected occurrence
  and every M.job follows one exact owned processing receipt
  and every M.transcript follows that job's published revision
  and every M.translation follows its exact original revision/job/profile/target
  and every M.citation names one of those revisions and a valid cue interval

FreshAdmit(T,M) :=
  Exact(M,T) and CurrentScope(T) and ClockAcceptable(T)
  and NoLifetimeRun(T) and WithinAllManifestBounds(M)
```

For the initial slice, accept **freeze now** only. Work still pending becomes explicit frozen partial coverage. Freeze the scope/grant digests, ordered receipt membership, occurrence/recording/input/job/revision identities, actual `en` target, profile identities, coverage and missing reasons, requested finding bound and observation mode in one consistent transaction. A timestamp alone is not a reproducible cutoff. The manifest's stored exact membership and revisions define the observation. Its canonical bounded serialization and digest detect identity/integrity changes; the digest does not prove truth or grant permission.

All authority, replay and lifetime-run checks belong in that transaction. Preserve one lifetime run allowance across the legacy checkpoint and exact-manifest admission origins. A different origin must not become a second publication allowance. Historical checkpoint execution and exact replay preserve their existing meaning. A new task is currently required for another accepted run; multiple observations under one task need a separate future contract.

Bound **scanned rows, text bytes, allocations, SQL work, wall deadline and serialized page/manifest bytes**, as well as finding count. Traverse bounded pages without collecting the whole cue set. An exhausted resource bound yields partial/truncated coverage with a reason. It cannot produce an exhaustive no-literal-match claim. If admission cannot finish its bounded consistent scan, choose explicit refusal or a finite partial manifest; do not commit a hidden continuation that mixes later observations into the same snapshot.

Proposed effect invariants:

```text
effect_key := H(version, grant_digest, effect_kind, intent_ordinal)
CommittedEffect(R,k) <=> SuccessfulReceipt(R,k) names that owned effect
BriefingMembers(R) = { finding_id | successful owned finding receipt in R }
FrozenManifest(R, later_time) = FrozenManifest(R, admission_time)
```

The biconditional concerns task-owned effects, not a colliding independent artifact. Keep today's collision refusal instead of claiming that artifact. Current retention validation can refuse a fresh citation and produce a skip receipt; it cannot alter the manifest or an already stored finding. A later correction does not substitute its text for the task job's published revision. A same-source unrelated recording is outside membership even if newer or easier to decode.

Use explicit interval domains: planned civil capture time, retained decoded media time and processed media coverage are different quantities. Existing reconciliation computes saturating uncovered/unprocessed summaries. Those summaries do not justify assuming planned wall duration always exceeds decoded duration. Store/check valid per-recording media intervals and their union; distinguish a genuine uncovered interval, missing media, failed work, no recognized text, unsupported language and truncation. Saturation must not conceal malformed or overlapping coverage.

Publication partial means the finite result contains declared missing outcomes. It can be terminal while its frozen evidence includes jobs observed pending. Completed means the requested finite literal template finished without its declared partial conditions; neither state establishes language qualification, topic truth, source independence or goal satisfaction. No-literal-match applies only to the explicitly scanned evidence, and only when the scan completed within bounds.

## S-03: withdraw authority without pretending work stopped

Let `I(j)` be immutable admissions to job `j`; `W(j)` be exact append-only withdrawals. Conservatively retained migrated authority is part of the effective authority decision.

```text
Active(j) = { i in I(j) | no valid withdrawal in W(j) for i }
Withdraw(T,j) removes only T's effective interests
StopForWithdrawal(j) => Active(j) is empty
ReleasePhysicalResources(j) => SupervisorProvesCompletion(j)
```

One immediate transaction must fence future admissions under the new cancellation intent, append exact withdrawal receipts, determine effective remaining owners, and either preserve the canonical job or set its cancellation fence. The supervisor then acts on committed state. Notification delivery is an optimization; restart reconciliation must find the durable cancellation. A receipt for requested cancellation is different from proven physical completion.

For an unshared queued job, end it before launch. For an unshared running job, enter the existing cancelling path. While native reads can remain active, preserve leases and physical liability. Successful worker output after the cancellation transaction must not publish. If successful finalization commits first, preserve that published history; later withdrawal cannot undo it. Replacement attempt generations reject old callbacks independently of the cancellation state check.

Attaching a new owner and checking cancellation must be atomic with its receipt/interest/charge admission. If attachment commits before last withdrawal, the new owner keeps the job alive. If last withdrawal commits first and makes the job cancelling, hold or refuse the attachment without charge. Current canonical enqueue replay can return an existing job regardless of its state; returning that row is not sufficient permission to attach or revive it. Never turn a cancelling/failed/cancelled job back into live work merely by inserting an interest.

Successful ended-job reuse may be useful when the complete immutable identity and fresh authority match, but the initial slice should state its exact supported case. Recompute after cancelled/failed work needs explicit new attempt authority and resource accounting; request replay is not retry authority. Job identity must include input/revision, transform/profile and actual target, and future sharing must respect privacy/destination/context boundaries. S-03 does not expand the current English translation contract.

Compatibility is consequential: existing processing cancellation promised that admitted jobs continue. Do not retroactively withdraw its interests during migration or reinterpret its exact request replay. Introduce a versioned withdrawal intent/receipt with a distinct explicit request. Earlier cancellation remains historical admission fencing. A new withdrawal can operate over earlier admitted interests only with that new authority and documented replay semantics. Retain legacy uncertainty conservatively. A missing reconstructed direct row is not permission to stop a job previously held by unknown direct authority.

Collection cancellation, processing admission cancellation, new interest withdrawal and publication cancellation have separate effects. Monitor pause keeps its existing treatment of admitted work. Direct interests do not identify independent callers. Existing explicit whole-job analysis cancellation needs its current administrative meaning documented separately; it should not accidentally acquire task-only semantics or be used by a delegated task to override another owner.

## Charges and physical work are different measures

For task `T`, let `G_T` be its finite processing allowance and `Q_T` the sum of audio charges on its unique committed recognition receipts:

```text
0 <= Q_T <= G_T
Q_T(next) >= Q_T(now)
Replay(receipt) adds 0
Withdrawal(receipt) refunds 0
```

Sharing one canonical job across current task/direct/monitor interests does not combine their authority: each owner's declared charge is exact and independent. Two-task same-job sharing is only an abstract future-contract example. Current [collection schema](../crates/sigy-service/src/storage/043-task-collection.sql) makes owned rules and occurrences globally unique, and processing binds each task to its own occurrence/recording, so that same-job case is unreachable through valid current APIs. Summed owner audio charges are not measured host CPU use or unique processed audio. Conversely one charged admission can have several bounded physical attempts; retry CPU/deadline/lease bounds still apply. Zero USD does not mean zero CPU, memory, storage or wall time. Neither cancellation nor policy revision refills a lifetime grant.

## Event histories and linearization

Here `<` means committed ordering, not when a client receives a response or a callback enters a queue. Actors are task client `T`, another owner `O`, service catalog `C`, native worker `N` and supervisor `S`.

| History | Required result |
| --- | --- |
| `C.finding_commit < C.run_cancel` | The finding and receipt remain; future effects stop. |
| `C.run_cancel < C.finding_attempt` | No new task effect; exact cancel replay returns the original receipt. |
| `C.attach(O) < C.withdraw(last prior owner)` | `O` preserves canonical work; withdrawing task loses only its own interest. |
| `C.withdraw(last) < C.attach(O)` | Attachment observes cancelling/cancelled/failed state, holds or refuses without partial admission or charge. Successful immutable reuse is a separate qualified case. |
| `C.finish_success < C.withdraw(last)` | Published result remains immutable. No claim that withdrawal stopped already completed work. |
| `C.withdraw(last) < S.observe(N.success) < C.finish` | Cancellation state wins in finalization; no successful publication. Callback arrival cannot override committed state. |
| `C.admit_commit < process crash < response delivery` | Reopen and exact request replay find the original manifest/job/charge/receipt. |
| `transaction fault before commit < reopen` | No partial manifest/grant/effect/receipt/withdrawal. Reconcile any uncertain commit by request identity. |
| `C.withdraw_commit < crash before OS stop` | Restart preserves cancellation fence and remaining liability; it does not assume the worker stopped because service memory disappeared. |
| `C.new_attempt_generation < old callback` | Old callback cannot publish or release the new attempt's resources. |

## Safety, liveness and acceptance evidence

Safety says nothing bad happens: no unowned evidence substitution, duplicate owned effect/charge, lost committed history, unauthorized attach, stale publication or release before proven completion. Liveness says an eligible finite run eventually advances under fair service ticks, available catalog/media and a completing supervisor. It is conditional. Scope drift, regressed time, unavailable media, blocked filesystem operations or failed containment can legitimately hold/refuse progress. A cancellation request is not an unconditional wall-time stop guarantee.

Recommendation: implement a pure deterministic reference state machine using sets and typed transitions, independent of SQL helpers. Start with two tasks owning distinct jobs, direct/monitor interests sharing an eligible job, two citations and one run allowance per task. Enumerate bounded histories containing admit, attach, freeze, effect, cancel, withdraw, finish, scope change, crash/reopen and stale callback. Enforce current ownership/reachability constraints; any two-task same-job model belongs to a separately labeled future contract. Check invariants after every transition; compare reference traces to storage fixtures using exact identities and ordered receipts rather than incidental timestamps. Publish explored bounds, state/transition counts and any deliberately omitted native/OS states. This is bounded checking, not a proof of all workloads. A later TLA+ model can formalize fairness and refine the catalog-to-supervisor handoff without replacing implementation tests.

Inspected existing [run fixtures](../crates/sigy-service/src/storage/tasks/run/tests.rs) cover receipt rollback, replay after drift, cancellation, immutable reopen, bounded pending pages and independent artifact collisions. [Execution fixtures](../crates/sigy-service/src/storage/recognition/tests/translation/task_execution.rs) cover resumed/uninterrupted membership, retained history and publication cancellation. [Processing fixtures](../crates/sigy-service/src/storage/tasks/processing/tests/admission.rs) cover atomic charges/interests and shared monitor work; their cancellation deliberately keeps admitted jobs alive. Extend these, preserving their historical assertions.

| Bounded scenario | New acceptance observation |
| --- | --- |
| Two collected recordings plus newer unrelated same-source recording | Exact manifest contains only owned identities and their actual job revisions. |
| Original corrected and translation superseded after freeze | Replay and advancing effects preserve frozen lineage; current staleness remains explicit. |
| Pending recognition/translation at freeze | Terminal partial result names pending stage; later completion does not expand that run. |
| Scan row/byte/deadline/SQL-work limit with zero matches so far | Explicit truncation, bounded memory/work and no exhaustive no-match statement. |
| Retention changes before and after one finding commit | Fresh invalid citation skips; already committed finding and manifest survive. |
| Same request with changed origin/target/spec | Conflict; no second lifetime run, job, effect, interest or charge. |
| Task-only, task/direct and task/monitor sharing; two independent task jobs | Only last effective-owner withdrawal from queued/running work requests physical cancellation; current task ownership constraints remain enforced. |
| Both attach/withdraw and finish/withdraw orders | Each outcome matches committed ordering, with no intermediate partial admission. |
| Legacy cancelled grant and migrated uncertain interests | Exact old replay unchanged; no retrospective withdrawal or inferred missing authority. |
| Failure at each manifest/effect/withdrawal write and commit boundary | Reopen yields old or fully committed state with exact immutable receipt membership. |
| Crash after cancellation commit before termination; stale result later | Cancellation remains recoverable, result rejected, leases/capacity retained until proven completion. |
| Restore exact catalog and retained media | Identity, manifest, charge and historical withdrawal audit survive; unavailable external process state is reported separately. |

Choose numeric scan/serialization/transaction limits from existing envelopes and measure the resulting workload before integration. Schema/IPC migrations need immutable triggers, hostile-row reopen audits and populated v44 compatibility fixtures. Historical interests still exist when effective interests reach zero; audits must distinguish those sets rather than rejecting a legitimately cancelled terminal job. This research establishes no migration version, native platform qualification, NAS guarantee or completed S-02/S-03 gate.
