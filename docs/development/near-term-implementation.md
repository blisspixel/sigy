# Near-term implementation briefs

Updated: 2026-10-03. Status: acceptance briefs for implemented bounded slices and remaining work. This document narrows existing [engineering packages](reliability-and-scale.md); it creates no new authority or qualification. The starting catalog was v44 and local IPC v45; the integrated implementation is v47/v48. [Active work](progress.md) holds exact verification receipts. The [workflow invariants](../design/workflow-invariants.md) define state, identity, accounting and measurement rules. Dated [publication](../../research/38-publication-and-cancellation-logic.md), [capacity](../../research/39-capacity-coverage-and-accounting-math.md) and [discovery](../../research/40-discovery-retrieval-and-evidence-logic.md) research records source inspection and primary evidence.

## What the next useful result is

The workstation increment implements S-02A/B/C, S-03A/B and the reference/interaction parts of EX-01A/B. [Exact evidence/publication](../decisions/0078-exact-task-evidence-publication.md), [withdrawal](../decisions/0079-task-interest-withdrawal.md) and [country discovery](../decisions/0080-offline-country-reference.md) record selected behavior. Schema v47 and local IPC v48 are the integrated interfaces. The 100 ms cooperative work guard, four million VM-operation cap, 10 ms lock wait, 4 MiB examined text and 64 KiB snapshot are bounded failure envelopes, with no machine-independent latency claim. The following briefs retain acceptance requirements and later work; a working implementation does not by itself close their qualification gates.

A finite task collects from its accepted sources, processes only its owned recordings, and publishes an exact, inspectable result or useful partial outcome. A user can stop that task's remaining processing without stopping work another owner still needs. In parallel, a country picker makes world listening easy even with an empty cache. Each step preserves professional controls and an enjoyable, generous terminal experience.

The implemented runtime order was bounded exact reconciliation, exact snapshot/admission/publication, then owner-aware withdrawal, with independent country-reference work. Next refine station name ordering and full-screen composition, exact retained passage playback, native qualification and aggregate admission. Target-aware migration and a bounded local planner remain separate workstreams. Read-only storage inventory and archive query characterization are small operator/retrieval increments; they do not justify a new backend. Language/native experiments keep their separate qualification gates.

| Brief | First user outcome | Dependency |
| --- | --- | --- |
| S-02A: Bounded exact reconciliation | Inspect a task's own current evidence without unbounded cue allocation | Current collection/processing receipts; preserves read-only behavior |
| S-02B: Frozen exact observations | Store and inspect a finite exact-membership evidence snapshot | S-02A and versioned manifest/target identity |
| S-02C: Publication from exact observations | Publish only that snapshot's cited findings and briefing coverage | S-02B; shares existing lifetime run allowance and executor |
| S-03A: Owner withdrawal | Stop future task admissions and explicitly withdraw its job interests | Durable interests and cancellation compatibility design |
| S-03B: Supervisor reconciliation | Cancel an unshared job safely and preserve shared work through restart | S-03A; existing native supervisor and read leases |
| EX-01A: Country reference/resolver | Type/select a declared worldwide country/territory independent of stations | Licensed pinned reference and shared bounded resolver |
| EX-01B: Country interaction | Apply the selected identity through CLI/TUI with truthful cache state | EX-01A; existing filters and station-ID paging |
| ST-01: Placement inventory | Inspect current local library placement and known accounting | Existing Library owner/doctor, no new store configuration |
| AR-01: Query-work baseline | Characterize and bound cooperative canonical archive query work | Existing literal search, selective access and scoped cancellation |

Rows split existing workstreams for review; they do not add nine new roadmap stages or require all rows in one patch. One primary runtime integration runs at a time on this host. Review and integrate independently ready source changes with the appropriate gates.

## S-02A: bounded exact reconciliation

**Outcome:** a direct analyst or task inspection can follow occurrence, recording, receipt, canonical job and published revision identities to literal evidence. No unrelated newer same-source recording or correction replaces that lineage. Inspection writes nothing and admits no work.

Current owners: [evidence types](../../crates/sigy-service/src/task/evidence.rs), [reconciliation](../../crates/sigy-service/src/storage/tasks/evidence.rs), [control](../../crates/sigy-service/src/control/task.rs) and [processing lineage fixtures](../../crates/sigy-service/src/storage/tasks/processing/tests/lineage.rs). The starting `Matching::scan` collected selected cue rows before applying the 64-citation cap. The active increment streams bounded rows before freezing and publication.

Stream rows into bounded matching; count examined headers/cues and text bytes, not only matches. Bind deterministic scan order to entry ordinal and exact revision/cue ordinal. Reuse the frozen terms and current literal comparator. Stop safely on a declared bound and identify the omitted remainder. Query cancellation must be scoped to the connection/operation and cleared on every path; it cannot establish a hard blocked-I/O stop guarantee. No source, worker, hash verification or retention mutation runs inside this read.

Check stored text type/byte length or use a bounded SQL projection before materializing a cue as a Rust `String`. Enforce a cumulative examined-text budget while streaming. Snapshot encoding needs a capped writer or a demonstrably prebounded structure; allocating an unbounded JSON buffer and checking its length afterward is insufficient. Reserve mandatory identity, coverage and partial-reason overhead before adding optional citations.

Proposed initial envelope to measure before selection: two collected entries, at most four committed processing-stage receipts, 64 citations, 4,096 examined cue rows, 64 KiB serialized snapshot and a 100 ms cooperative catalog-work budget. Existing per-value byte bounds still apply. These are candidate safety limits, not a latency promise or a new supported profile. Use a bounded lookahead or remainder flag to distinguish exactly full output from known omitted matches; exhausting scan work with no match is partial, not `no_literal_match`. Bound cumulative scanned text/allocation independently of serialized output. Measure the current fixture library and a boundary-heavy synthetic library before freezing the limit decision in the implementation record.

Include preparation, joins, ordering, coverage aggregates and lookahead in SQL-work accounting. A `LIMIT` on returned rows can still scan/sort a larger relation. Selective query plans and a finite scoped VM-progress budget are independent obligations. Measure lock waiting separately, define its finite policy and preserve rollback/handler cleanup on interrupts. The 100 ms candidate is cooperative catalog work, not a hard operation deadline or a bound on blocked filesystem calls.

The current database binding enables only `bundled`, without its safe progress-callback feature. Review and qualify that feature, or another bounded mechanism, before claiming query-work enforcement. [Encoding and interruption research](../../research/38-publication-and-cancellation-logic.md#encoding-and-query-interruption) records the dependency and exact-integer constraints; no feature or manifest encoding is selected here.

Decode/rendered-text lengths and task-level sums must use checked base-unit arithmetic. Retain existing planned, recorded, recognized and untranslated measures under their documented meanings. Do not change them to a generic interval union unless a separate versioned mapping establishes comparable clocks. Complete scan, complete capture, complete processing and qualified semantics are distinct fields.

Acceptance covers sparse matches beyond the scan boundary, one oversize cue, exactly 64 versus 65 hits, mixed/unknown scripts, no text, untranslated output, pending/failed stages, newer unrelated recording and correction. Expected references are hand-declared IDs/intervals; tests cannot derive them by calling reconciliation itself. Record examined work, output bytes, allocation bound, stop reason and control/capture impact. An unchanged successful CLI result is insufficient if the query still allocates all rows.

## S-02B: frozen observations and compatibility

**Outcome:** explicitly freeze the current exact evidence and inspect what a later publication will use. Freeze now is the initial observation mode. Processing still pending becomes a frozen partial result; later completion does not silently expand it. Automatic wait-until-settled observation is deferred to a separately bounded controller design.

In one consistent bounded catalog transaction, freeze task scope, collection/processing grant digests, entry/occurrence/recording membership, ordered immutable receipts, input/job/profile/transcript/translation identities, current `en` target, coverage/reasons, citations, scan/byte stops and observation mode. Store exact membership and a versioned digest; a wall-clock timestamp alone is not the cutoff. No network/native/filesystem work belongs inside this transaction.

Use a new typed snapshot origin/version rather than changing historical broad-monitor checkpoints. New and old observation records share an explicit finite per-task budget, initially no larger than the current 128-observation ceiling; a populated legacy task cannot gain another 128 rows by switching origin. Preserve legacy checkpoint ordinal allocation, payloads and expected-ordinal replay. Add an explicit combined lifetime observation count/receipt whose capacity check and append share the snapshot transaction. Repeated identical freeze requests return the original snapshot before any new scan, scope check or capacity check. Changed caller-supplied origin/parameters/expected ordinal or digest under the same identity conflict; changing live evidence does not. A new observation requires a distinct explicit request and remaining observation capacity, not a rewritten snapshot.

The exact payload must fit its byte cap. Expected partial scan exhaustion can be represented explicitly only if the bounded snapshot remains structurally valid. Byte overflow of mandatory identity/coverage fields refuses the write; do not persist an incomplete identity manifest disguised as partial evidence. Abort before commit leaves no snapshot or request receipt.

Preserve every old checkpoint, run, digest recipe, cancellation and briefing meaning. Rehearse populated v44 migration and interrupted migration using the existing backup/restore path. Snapshot origin and new operations require an explicit schema/IPC compatibility decision; do not announce a successor version until its migration exists. English identity is recorded now, while non-English-target execution stays S-05 work.

Acceptance compares frozen versus live evidence after a correction, independent recognition, newer translation, job completion, retention and scope drift. Frozen history remains readable; current staleness/media availability is a separate inspection. Reopen audits validate exact receipt membership, revisions, hashes, ordinal/capacity bounds and same-task binding without incorrectly requiring old media to remain retained.

At 127 combined observations, race a legacy checkpoint and exact snapshot: only one 128th observation can commit. At 128, fresh observation refuses while exact old replay still returns its original history. Retry freeze after live job completion without recomputing membership. Verify old expected checkpoint ordinals remain unchanged and rollback consumes no shared observation slot.

## S-02C: exact publication and the human journey

**Outcome:** publish a finite cited artifact and briefing whose membership and coverage come only from the selected exact snapshot. Reuse the [existing run admission](../../crates/sigy-service/src/storage/tasks/run/admission.rs), [advance](../../crates/sigy-service/src/storage/tasks/run/advance.rs), [briefing path](../../crates/sigy-service/src/storage/briefings/task.rs) and run/recovery fixtures.

Legacy checkpoint execution and exact-snapshot execution share one lifetime publication run per task. Choosing a new origin cannot refill the publication allowance. The typed grant binds origin/version, snapshot digest, requested finding limit and accepted scope. Historical replay returns the original grant and receipts after drift; fresh admission/effects recheck present authority. Keep at most 64 finding attempts and one exact-membership briefing.

Each finding effect and its outcome receipt commit together. Fresh media/cue validation can record a skip without rewriting the snapshot or an earlier finding. Current publisher limitations, including unavailable translation references, remain explicit skips rather than permission to invent text or dispatch another job. Cancellation and revocation stop future effects only. Preserve bounded rotating service passes and the existing expected-refusal versus integrity-fault distinction.

Publication reports two layers: finite run state and frozen evidence outcome. Preserve current `Partial` terminal semantics for partial frozen evidence; reaching the end of the finite plan cannot relabel it `Completed`. A complete exhaustive literal scan can still have zero matches. The report names selected sources/window/terms, exact captured/processed coverage, omissions, current unavailable originals, actual `en` target/profile and quality limits. It never labels literal matching as semantic success.

The human flow is inspect current exact evidence, explicitly freeze, inspect frozen scope/partial consequences, then explicitly admit publication. Freezing grants no effects. Explain that publishing a partial snapshot fixes that run's coverage and uses its one publication allowance. Keep advanced identities inspectable and normal output concise with the next permitted action. Proposed syntax enters the command reference only after the parser implements it.

| Difficult event | Required visible/artifact result |
| --- | --- |
| A newer unrelated recording exists on the same source | It is absent from task membership |
| Translation or original is corrected after freeze | Frozen revision remains; separate inspection marks current staleness |
| A cue becomes unavailable before its finding commits | Durable skipped outcome; partial briefing with exact successful members |
| Audio expires after a finding commits | Historical finding/receipt remain with unavailable replay |
| Service dies before or after effect commit | Neither effect/receipt or both; exact replay never doubles them |
| Scope changes between two effects | Committed effects remain; later effects are revoked |
| Output, scan or finding cap is reached | Named truncation/partial outcome; no exhaustive no-match claim |

The integrated acceptance journey uses two authorized finite sources, one controlled missing stage, one same-source unrelated recording, one late correction and a restart. Expected artifact IDs, charges, coverage and memberships are independently frozen. Run storage/control/CLI/actor fixtures and the appropriate local verification gates; inspect actual CLI output. No general planner or newly arranged human reviewer is needed to demonstrate this literal workflow, and it qualifies neither language quality nor semantic completion.

## S-03A: explicit owner withdrawal

**Outcome:** stop future task processing admissions and withdraw only the task's own interests. Another owner retains canonical work. Current owners are [processing storage](../../crates/sigy-service/src/storage/tasks/processing.rs), [interest storage](../../crates/sigy-service/src/storage/interests.rs), [v44 schema](../../crates/sigy-service/src/storage/044-task-processing.sql), recognition/translation cancellation storage and the [analysis actor](../../crates/sigy-service/src/control/actor/analysis.rs).

Admission interests remain immutable. Append exact withdrawal rows bound to job family/identity, owner, original interest/receipt, explicit cancellation semantics version, request and generation. Distinguish historical interest count from effective active count in reads and reopen audits. Withdraw at most the task's exact four recognition/translation interests for its two collected entries; replay adds no withdrawals or allowance charge.

In one immediate transaction, fence future task admission, record the exact withdrawn set, check remaining interests and conservative legacy authority, and transition an unshared queued/running job to cancelled/cancelling through its existing state machine. Only after commit request supervisor cancellation. A full rollback leaves all prior authority and job states unchanged.

Compatibility is part of the behavior: existing processing cancellation means admitted jobs continue. Migration adds no automatic withdrawal. Its exact request replay returns the old receipt and creates no new stop authority. New withdrawal over an already cancelled legacy grant needs a distinct explicitly accepted versioned request. Fresh cancellation can combine future-admission fencing and withdrawal, with its exact new meaning stored. Do not silently reinterpret the old command payload or hash.

Administrative whole-job analysis cancellation is separately authorized and retains its current meaning. Delegated task withdrawal cannot call it as a shortcut around effective-owner checks. Direct interests are not independently cancellable direct clients, and monitor pause does not withdraw admitted interests. The first slice supports task-owned withdrawal only; broader owner cancellation needs its own contract.

Check both committed orders of attach/withdraw, completion/withdraw and new-attempt/old-callback events. Another owner attaching first preserves work. Cancelling/cancelled/failed work cannot be resurrected by attachment; hold/refuse atomically before a new charge or interest. Task allowances never refund, and sharing one job still consumes each admitted owner's declared allowance once.

Distinguish successful immutable-result reuse from revival. A succeeded job can be reused only where current exact input/profile/target identity, owner authority and media policy permit it, with the ordinary atomic owner receipt/allowance charge and no new physical launch. Cancelling/cancelled/failed work cannot be revived by attaching an interest; recomputation needs separately accepted attempt authority. This distinction preserves existing compatible reuse instead of banning every terminal attachment.

Four task withdrawals do not bound remaining-owner lookup over historical interests. Use selective indexes and bounded cooperative SQL work to establish effective authority. If the lookup is interrupted or cannot establish emptiness, hold/roll back cancellation; unknown is never zero remaining owners. Include many withdrawn historical interests plus one surviving owner late in the access path, using reachable owners and valid provenance.

## S-03B: durable cancellation to proven completion

**Outcome:** cancellation remains inspectable and recoverable if service failure occurs between its catalog commit and OS signalling. The exact job generation and cancelling state fence publication. Reuse the native worker supervisor, lease lifecycle and actual empty-group completion evidence.

Separate cancellation requested, worker stop requested, completion observed and capacity/media protection released. Do not turn a deadline or missing process record into proven completion. A service restart derives pending stop/reconciliation from catalog state, validates the actual containment generation and retains unresolved leases/capacity. OS/network-isolation limitations remain separately reported.

Queued unshared work stops before launch; running unshared work enters cancelling. A valid success committed earlier remains readable. A success callback observed after cancellation cannot override committed state. Old-generation callbacks cannot publish or release another attempt's allocations. Interest withdrawal cannot erase previously published transcript/translation/finding history or restore an owner's finite allowance.

Acceptance includes task-only, direct-plus-task and monitor-plus-task shared jobs, two tasks with independent jobs, queued/running/completed/failed states, all committed race orders, crash before signal, unresolved reader, clock regression, legacy cancellations and populated restore. Two tasks cannot currently own the same occurrence/recording, so same-job two-task sharing belongs only to a separately labeled future-contract reference model; never bypass catalog constraints to claim a reachable fixture. Independently compare exact effective interests, job state, artifacts, charges and lease ownership. Native fixtures establish tested termination observations on the mechanism used; a pure state model checks only its explored finite catalog scenarios.

## EX-01A/EX-01B: useful worldwide country discovery

**Outcome:** type a country name/code or open a worldwide selector, resolve ambiguity and apply the selected stable identity through the existing cache filters. Empty/offline libraries remain explorable. Current [directory contracts](../../crates/sigy-service/src/discovery/mod.rs), [storage](../../crates/sigy-service/src/storage/discovery.rs) and explorer search/state/rendering remain the operations to extend.

Before importing assets, record CLDR version/hash/license, selectable code inclusion and exceptional/deprecated/provider-code policy, supplied locale/alias coverage and fallback chain. A complete declared country/territory list does not promise every localized alias or station. Macroregions and unknown placeholders are separate reference types. Preserve existing raw two-letter code fallback and its comparison semantics; a new name/alias resolver cannot silently rewrite stored filters.

Resolve recognized codes first; names/aliases yield a bounded candidate set with locale/provenance and match reason. Several identities require an explicit choice. Evaluate a versioned NFC/casefold primary alias key while retaining diacritics and script distinctions. Transliteration/accent-insensitive expansion is separately labeled candidate assistance, not silent identity resolution. Never normalize transcripts or change the monitor/archive comparator as part of this slice.

Keep existing 128-byte query bounds and 16-station pages in ID order. Proposed country-candidate pages also cap at 16 with a stable reference-code continuation key and explicit `more`. Bind continuation to normalized query, comparison profile and reference version; clear/refuse it on incompatible changes. Determine uniqueness across the eligible candidate sequence, not one page: a singleton page with `more` cannot authorize automatic identity resolution. Evaluate reference/import and transformed-key byte limits before selecting assets. Counts describe an actually computed cache scope, or remain unknown. Applying a country performs only the existing cache search: no directory refresh, stream contact, click, audio, capture or processing.

CLI and TUI share the resolver. The wide picker has deliberate typography, readable native scripts, country/code and match/coverage state, persistent query/selection and a clear explicit apply action. Compact/linear modes retain ambiguity and scope. Draft changes do not relabel applied station rows, stale replies cannot replace newer scope, and cancel restores the prior useful view. Inspect 20x8, 80x24 and 132x40 production renders plus keyboard/paste/resize/offline cases and actual terminal responsiveness under bounded capture.

Acceptance selects every declared reference entry with zero cached rows, checks independent expected codes/aliases, ambiguous and split historical names, unavailable locale fallback, control characters, combining/RTL/wide text, paste expansion and generation races. Retain source/query context when moving between picker, list and globe. Name ordering, collation and cursor migration is a later EX-01 slice; city/radius search remains EX-03. A country picker need not wait for either a world gazetteer or multilingual inference.

## ST-01 and AR-01: bounded operator/retrieval increments

ST-01 uses [Library ownership](../../crates/sigy-service/src/library.rs), the [library control boundary](../../crates/sigy-service/src/control/dvr.rs) and existing [doctor checks](../../crates/sigy-service/src/control/doctor.rs). Derive the already owned paths and report OS/architecture plus configured/charged/reserved bytes. Label unmeasured physical capacity/locality/durability. No media walk, network probe, external-root configuration or policy change. Fresh absent media directories are normal. Sanitize and bound paths, preserve local/service/default/explicit-library parity and distinguish operator inspection from ordinary private logs.

AR-01 uses the existing [archive scan](../../crates/sigy-service/src/storage/archive.rs). Freeze a query/reference library, record literal comparator and current start-time filter/continuation semantics, inspect selective query plans and characterize rows, VM work, page bytes, memory, deadlines and latency tails under capture. Scope/clear progress callbacks; fail or report truncation consistently without claiming blocked-I/O cancellation. Do not change relevance ordering or comparator to make a benchmark look faster. Lexical projection remains AR-02.

These increments establish facts for later deployment/index choices. A large configured quota, a doctor report or fast synthetic scan cannot qualify a Pi, NAS or sustained library. Second-volume movement still needs exclusive ownership, a finite frozen copy manifest and proven lifecycle; it is not a string replacement of the media directory.

## Verification, independent review and integration

For every row, record the exact acceptance brief, source/test owners, chosen finite limits, schema/IPC/dependency impact, supplied fixture licenses and unchanged historical contracts. Write independent hand examples and bounded state/event histories before expected results are encoded. Do not import production transition helpers into the reference oracle.

Use a small Rust finite model for task/interest/effect/accounting races where it adds meaningful coverage. Explore a declared population/event depth, retain state/transition counts and shortest counterexamples, and separately state fairness assumptions for progress. Translate counterexamples into production-path fixtures. This is planned bounded verification, not a formal proof already obtained or a mandatory new framework.

Run focused storage/actor/control/CLI checks, reopen and populated migration/backup/restore for changed persistence. Run `cargo verify` and `cargo verify-coverage` for runtime increments; media/acquisition/decoder changes also require `cargo verify-media`. Preserve warnings-denied linting, native source hashes, all sources in coverage and every per-crate 80% threshold. UI slices include rendered and actual terminal inspection. Measure resource/control/capture impact before increasing load; scope failures to the affected package and keep unsuccessful evidence.

Review against the [invariant checklist](../design/workflow-invariants.md), repair changed trust-boundary defects and update active work and affected summaries. A local passing slice is not a stage exit. No bulk data download, hosted inference, hardware experiment or distributed deployment is necessary to finish these first workflow/country/operator increments. Their later qualified profiles retain explicit independent evidence gates.
