# Workflow invariants and mathematical contracts

Updated: 2026-10-03. Status: proposed specification for near-term implementation and verification. Equations define quantities and candidate obligations, not measured capacity, formal proof or implemented aggregate admission. Current source/test owners and primary evidence are recorded in [publication/cancellation research](../../research/38-publication-and-cancellation-logic.md), [capacity/coverage research](../../research/39-capacity-coverage-and-accounting-math.md) and [discovery/retrieval research](../../research/40-discovery-retrieval-and-evidence-logic.md). The [near-term implementation briefs](../development/near-term-implementation.md) apply these contracts to concrete packages.

Keep one canonical authority: service transactions admit intent, maintain exact accounts and publish effects. Models, source content, indexes, wiki pages and terminal state cannot authorize effects. Existing decisions remain authoritative for their implemented versions; the formulas below refine new work without silently rewriting historical meaning.

## State, events and proof scope

Model an operation as `S_next = step(S, event)` or an explicit refusal/hold. State includes accepted scope, immutable grants, canonical jobs and generations, owner interests, effects/receipts, capacity claims and media leases. Events include admission, attach, cancel, completion, policy change, correction, retention, restart and clock movement. UI state and physical worker state are separate from catalog state.

| Property | Question | Evidence |
| --- | --- | --- |
| Safety | Can any admitted transition violate identity, authority, accounts or retained history? | Predicates after every transition, transaction fault injection and independent event histories |
| Liveness | Can eligible finite work advance under declared conditions? | Explicit fairness/resource/service assumptions, rotating passes and bounded no-progress outcomes |
| Refinement | Does the implementation realize the specified transition and observable result? | Compare domain/state-machine outcomes with storage, actor and CLI/IPC fixtures |
| Operational qualification | Does the declared real workload remain useful and recoverable? | Actual platform/storage/native/quality and sustained mixed-workload evidence |

Do not infer liveness from safety or performance from model checking. An unavailable model, exhausted grant, absent store or blocked read may legitimately hold progress. State those conditions. A supported method must report a useful hold, partial or unsupported result instead of looping indefinitely. Finite exploration checks only explored modeled cases; OS, database and storage assumptions still need their own evidence.

## Identity, immutable inputs and digest meaning

An interpretation's identity includes its exact input revision, transform/profile hash and parameters, translation target where applicable, and derivation version. A task's publication additionally binds accepted scope, collection/processing receipts, exact owned occurrence/recording identities, cutoff, coverage, citations and truncation. Location, display title, locale and relevance rank do not substitute for those identities.

```text
manifest_digest = H(domain_version || encode_typed(manifest))
effect_key      = H(effect_domain || grant_digest || effect_kind || ordinal)
```

The separators here denote unambiguous typed encoding, not naive string concatenation. Freeze byte encoding, field/array ordering, integer units, optional/missing distinction and validation rules before publication. Preserve original scripts; do not normalize every evidence string simply to make its digest easier. Identity canonicalization applies only to fields whose contract explicitly selects it.

Existing hash recipes and stored payloads keep their historical meaning. A new manifest gets a new domain/schema version. Inspect current typed serialization before selecting an encoding; generic JSON canonicalization is an alternative to evaluate, not an automatic migration. Avoid floating-point loss in integers or money. A checksum proves identity of the encoded bytes, not correctness of the claim, source independence or safe execution of the content.

## Atomic effects and replay

Let `intent(k)` be a stored admitted effect intent with stable key `k`, `owned_effect(k)` the successfully published artifact from that intent, and `success_receipt(k)` its committed outcome. For successful catalog publication:

```text
owned_effect(k) exists iff success_receipt(k) exists
successful_owned_effect_count(k) <= 1
successful_owned_effect(k) implies valid_intent(k)
```

A preexisting unrelated artifact using the same visible identifier is a conflict, not successful task ownership. Refused/skipped effects have their own receipts without a success artifact. Finding and receipt commit together; one briefing contains exactly that run's committed successful finding identities. No broader monitor membership enters by convenience.

For request key `r` with accepted normalized payload `p`:

```text
replay(r, p) = the stored result, with no new effects or charge
replay(r, p_other) = conflict, when p_other != p
```

Historical replay remains available after policy drift. Fresh admission and each fresh effect recheck current authority. A retry after an unknown client response first inspects/replays the stored identity; it does not invent a new request key to bypass uncertainty. Exactly-once catalog materialization does not mean a network or native process ran exactly once.

Validity in the successful historical-effect predicate means valid at its publication commit. Later revocation does not invalidate the fact that a past effect was authorized then. Freeze-request replay checks stored normalized caller intent before scanning live evidence; later completion or correction cannot create a false replay conflict. Snapshot cutoff means exact stored membership/revisions or a committed observation ordinal, never timestamp alone.

SQLite transaction boundaries establish which competing catalog event commits first. Snapshot assembly must finish under one consistent bounded snapshot, or use explicitly frozen immutable membership. Do not mutate a scanned table on the same connection while depending on an unfinished query's order. No network, worker execution or filesystem copy belongs inside the publication transaction. Physical I/O and catalog entries need the separate [storage protocol](storage-and-memory.md#media-identity-and-cross-store-publication).

## Job interest and cancellation logic

For canonical job `j`, let `I_j` be immutable admitted interests and `X_j` explicit withdrawal receipts. Define active interests by exact identity:

```text
active(j) = { i in I_j : no valid withdrawal in X_j names i }
new_job_stop_allowed_by_task(j) iff active(j) is empty
                                   and no conservative legacy guard remains
                                   and j.state in {queued, running}
```

Count authority, not clients or processes. Current direct interests do not distinguish independently cancellable callers. Monitor pause retains its documented admitted-work authority. A task can withdraw its own interests only; it cannot withdraw a direct or another monitor/task interest. Existing administrative analysis cancellation retains its separate meaning until an explicit decision changes it.

A valid withdrawal can retain history for a succeeded job without requesting physical stop. Compatible successful-result reuse is separate from reviving cancelling/cancelled/failed work. Current task-owned occurrences are globally unique, so two different tasks cannot share the same recording/job through current collection APIs. Use reachable task-plus-direct/monitor sharing for implementation fixtures and separate two-task independent jobs; abstract future sharing models must state their additional ownership assumptions.

Append cancellation intent, exact task withdrawals, the remaining-authority decision and any job cancellation transition in one immediate transaction. Signal the supervisor only after commit. Recovery reconciles committed cancelling jobs to their current generation; a process signal is not the durable intent. Never refund a finite owner allowance because its interest was withdrawn.

| Competing events | Required outcome |
| --- | --- |
| Another owner attaches before final task withdrawal commits | Preserve the canonical job under the remaining interest |
| Final withdrawal commits before attach | Job becomes cancelling/cancelled; later attachment holds or refuses rather than resurrecting it |
| Valid completion commits before cancellation | Preserve completed artifact; withdrawal changes future owner authority/history only |
| Cancellation commits before a completion is accepted | The completion cannot publish success under a cancelling or replaced attempt |
| Cancellation commits, then service dies before signalling | Reopen reconciles cancellation intent; retain leases/capacity while completion remains uncertain |

Generation and state both matter. Current cancellation can mark a running job `cancelling` without replacing its generation; publication validates the expected state as well as generation. A replacement attempt invalidates old-generation callbacks. Never claim generation checking alone solves every completion race.

Legacy cancellation promises admitted jobs continue. Migration must preserve that promise and exact replay. New interest withdrawal needs versioned explicit intent and receipts; upgrading a binary cannot retroactively stop legacy-cancelled work. Retained history alone is not current execution authority, and missing historical direct provenance is not proof that direct authority never existed.

## Finite authority, paid liability and reusable capacity

Keep integer base units and checked arithmetic. Let `G_o` be an owner's finite processing allowance in audio microseconds and `a_ok` its immutable admitted charges:

```text
sum_k(a_ok) <= G_o
```

Each owner pays its own declared allowance charge once even when another owner shares the canonical job. Sharing does not duplicate canonical admission. One logical attempt can decode and execute many windows/cues or contain separately admitted subrequests; every physical stage, retry, overlapping allocation and provider request retains its own bounds and exact liability. At-most-once catalog effects cannot establish one physical/provider execution after an unknown outcome. A task charge cannot be used as a monitor charge or interpreted as two worker launches.

For a paid limit `B`, settled amount `S`, unresolved reserved liability `U` and a new worst-case request `x`, all in the same exact monetary unit:

```text
S + U + x <= B
```

This admits new work only when existing breach/frozen policy also permits it. Uncertain completion retains liability. Failure, clock rollover, retry, policy revision and restart cannot refill lifetime allowance. Report authority consumed, unsettled liability and capacity held separately.

For resource dimension `r`, capacity ceiling `C_r`, protected capture/control headroom `H_r`, existing attempt claims `q_ir` and a new claim `q_new,r`:

```text
sum_i(q_ir) + q_new,r <= C_r - H_r, for each r
```

Dimensions include memory/scratch bytes, CPU-rate ceilings and integer worker/device slots; they are not added into one score. Unknown required bounds hold admission. A lower instantaneous memory reading cannot replace an admitted worst-case claim. A slot or read lease becomes reusable only after the exact owner is proven finished and allocations are reconciled. Deadline expiry or lost PID/connection alone does not prove that condition.

Require compatible units, nonnegative claims and `0 <= H_r <= C_r` before checked subtraction. Headroom above the ceiling is invalid configuration, not zero available capacity. Saturating or wrapping arithmetic cannot hide that error. Validate exact withdrawal authority/version and immutable interest/receipt identity before treating any interest as inactive; a row merely naming another owner's interest is insufficient.

Use checked addition/multiplication and explicit overflow refusal. For nonnegative `n` and positive `d`, compute integer ceiling as quotient plus a nonzero-remainder increment; avoid overflowing `n+d-1`. Budget identity does not depend on approximate SQL numeric conversion. Rates and exploratory plots may use floating-point with labeled precision; ledger and admission comparisons do not.

## Coverage, stage denominators and absence

For intervals `I_k` on one recording/channel/clock epoch and comparable target interval `D`, define covered duration:

```text
covered(D) = length(union_k(I_k intersect D))
```

Use half-open intervals, sort and merge overlaps/abutment, and check all endpoints and lengths. Two intervals `[0,10)` and `[8,15)` cover 15 seconds, not 17. Two stations contribute separate source-seconds. Multiple translations or findings attached to one passage add no new observed time.

Subtract gaps only where they share a declared coordinate mapping. Planned UTC duration, received time, compressed bytes, decoded audio time, recognized coverage and translated cue counts are different quantities. If mapping is missing or uncertain, retain separate measures instead of manufacturing an exact partition. Preserve current evidence counters and their historical meanings; generalized unions are a new versioned measurement.

| State | What can be said |
| --- | --- |
| Not observed within declared collection coverage | No observation is available here; this says nothing about the world outside the sample |
| Observed but unprocessed/undecoded | Retained input exists, but interpretation is unavailable |
| Processed with unqualified meaning | A result exists; its semantic fidelity remains unmeasured |
| No literal match in a complete declared scan | The exact selected terms did not match under that comparator |
| Truncated or partial scan with no match | No match was found in the inspected part; remainder stays unknown |
| Historical evidence with expired media | Revision/citation history remains, while original playback is unavailable |

Do not collapse these states into one boolean success or confidence number. Workflow completion, coverage, retention, language quality and semantic support are independent report fields. Operationally complete processing does not prove event absence, identity or truth.

## Workload, fairness and retrieval metrics

For compatible measured profile `p`, observed real-time factor is `r_p = busy_wall_seconds/audio_seconds`. Queued work estimate is `sum_p(queued_audio_seconds_p * r_p)` worker-seconds. It requires positive denominators and measured comparable profiles. It excludes future arrivals, active remainder, holds, failed attempts and unrepresented stages. It is not elapsed completion time or available host capacity.

Under explicitly constant comparable rates, initial positive audio backlog `A0` in audio seconds, arrival rate `lambda` and available processing rate `mu` in audio seconds per wall second, approximate drain time is `A0/(mu-lambda)` wall seconds only when `mu>lambda`. Otherwise this approximation has no finite drain time. Busy throughput and available throughput differ. Little's law relates compatible long-run population averages under its conditions; it cannot turn an instantaneous queue count into a completion promise. Current integer pace rounding is descriptive, not an upper confidence bound.

Every fourth claim going to older batch work is a claim-opportunity rule, not 25% CPU time. Wall-time progress additionally needs bounded execution/cleanup, available resources and a bounded eligible population. Preserve source rotation and old-work opportunity while evaluating heterogeneous costs; do not replace it with unmeasured cost predictions.

For independently labeled retrieval units, `precision = relevant_retrieved/retrieved` and `recall = relevant_retrieved/relevant_available`, with undefined zero denominators stated explicitly. Freeze query scope, reference units, revisions and cutoff; report truncation and missing evaluation items. A relevant passage score does not establish a supported claim. Measure interpretation, entity identity, temporal reasoning and abstention separately.

Here `relevant_available` means independently labeled relevant units in the frozen eligible search corpus, not playable media or worldwide observations. Complete stored-text retrieval excludes untranscribed/unsupported inputs. Report that coverage and reference completeness; a result without a judgment cannot automatically count as irrelevant.

Country reference membership, cached station count, directory location, observed language and actual local playability remain distinct. String comparison, alias resolution, ordering and geography each have versioned rules. The [discovery research](../../research/40-discovery-retrieval-and-evidence-logic.md) supplies Unicode, ambiguity and geometric obligations; a directory point cannot locate a speaker or guarantee reception.

## Independent reference and transition checks

Begin with small exhaustive event histories, boundary examples and deterministic fixtures. A proposed Rust reference model should be structurally simpler than the implementation: sets for interests, exact counters for charge and union/reference calculations for intervals. It does not import production transition helpers or generate expected values by calling the code under test.

For S-02/S-03, explore two tasks with their distinct owned jobs, direct/monitor sharing, two source entries, a small citation set, two generations and bounded event depth. Check every reachable modeled transition and retain the shortest counterexample. State symmetry reductions, impossible-event constraints and omitted OS/I/O behavior. Two-task same-job sharing is future-contract model coverage, not a reachable current catalog fixture. Separate safety exploration from finite liveness checks with explicit enabled-event/fairness assumptions. TLA+ is a research alternative for difficult interleavings; it is not selected or required for ordinary increments.

Translate each discovered counterexample into a storage/actor/CLI regression fixture where applicable. Exercise rollback before/after catalog effects, reopen and backup/restore, stale callback, retention/correction, injected payload and exhausted limits. Compare exact committed identities, charges and receipts with an independent expected trace. Live outages may produce different observed audio; recorded gaps explain that difference instead of fabricated replay equivalence.

Acceptance requires normal and hostile paths, changed-boundary review, appropriate local runtime gates, rendered inspection for interface changes and measured resource behavior before increasing workload. Formal notation and a passing fixture are supporting evidence, not substitutes for an actual useful workflow.
