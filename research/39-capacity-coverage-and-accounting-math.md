# Capacity, coverage and accounting mathematics

Reviewed: 2026-10-03. Status: primary-source research, current source inspection and proposed measurement contracts. All numerical examples below are hypothetical hand calculations. They are not benchmarks, supported host limits, language-quality evidence or implemented aggregate admission.

Follow the [reliability and scale plan](../docs/development/reliability-and-scale.md), [scaling architecture](../docs/design/scaling-architecture.md), [storage placement research](36-storage-placement-and-capacity.md), and [task-owned processing decision](../docs/decisions/0072-task-owned-processing.md). This record clarifies quantities and proof obligations without selecting a new scheduler or storage backend.

## Three different accounts

| Account | Quantity | Release condition |
| --- | --- | --- |
| Finite authority | Lifetime capture or processing allowance, scoped to immutable intent | Consumption follows its declared grant contract; failure, cancellation and restart do not refill it |
| Execution capacity | CPU rate, native memory, device slot, scratch and output capacity | Reusable after the exact attempt has stopped and associated allocations are released |
| Paid liability | Settled expense plus unresolved worst-case reservations | Only an authorized settlement or proved release changes the reservation |

These accounts cannot be substituted for one another. Cancelling work may eventually free a worker slot without replenishing its lifetime task allowance. A shared canonical job can need one worker allocation and one provider attempt while several authorities each carry their own admitted interest and charge. Removing one interest does not stop work another interest still authorizes.

For finite processing grant `G` and immutable admitted charges `a_i`, all in audio microseconds, require `sum(a_i) <= G`. Historical replay adds zero new charge. For paid limit `B`, settled total `S`, unresolved liability `U` and new worst-case request `x`, require `S + U + x <= B`, subject to frozen/breach rules. An uncertain request remains in `U`; an additional retry needs an additional reservation. Capture grants use their selected planned-duration and maximum-byte contract, rather than substituting successfully retained duration.

Current owners are [task admission](../crates/sigy-service/src/storage/tasks/processing/admission.rs), [processing facts](../crates/sigy-service/src/storage/tasks/processing/facts.rs), [ledger](../crates/sigy-service/src/storage/ledger.rs), and the [core balance transitions](../crates/sigy-core/src/budget.rs). Job, receipt, interest and charge already commit together for task processing. Aggregate host-capacity claims remain proposed.

## Admission is a vector inequality

For each resource dimension `r`, use a declared usable ceiling `C_r`, protected reserve `H_r`, existing attempt claims `q_ir`, and new claim `q_new,r`:

```text
sum_i(q_ir) + q_new,r <= C_r - H_r, for every r
```

CPU rate is measured in processor-equivalents, memory and scratch in bytes, and slots in integer counts. Never add CPU cores to memory bytes to invent a single budget. Demand envelopes include startup, decoding, model execution, output drain and cleanup as applicable. Record whether stages overlap; summing sequential peaks as simultaneous use is conservative but not an observed peak. Unknown required bounds refuse admission.

Hypothetical: a declared 8 GiB execution-memory ceiling protects 2 GiB for other work. Two admitted 2 GiB claims occupy 4 GiB of the remaining 6 GiB. A 3 GiB request would require 7 GiB and is held. It does not become admissible because one process currently reports only 200 MiB. Configured maxima and measured instantaneous use have different roles.

[Kernel cgroup documentation](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html) defines CPU and memory control/accounting semantics. A CPU maximum is a scheduling-rate ceiling, not proof of available throughput. Windows committed-memory and Linux memory-controller observations are different metrics. [Current worker cost](../crates/sigy-service/src/storage/recognition/cost.rs) reports the highest eligible job-object peak and CPU sum; neither number proves a concurrent host envelope. GPU-memory enforcement requires its own qualified mechanism.

## Work, rates and backlog

For one compatible profile, let `A` be completed audio seconds and `T_busy` its measured busy wall seconds. Define observed real-time factor `r = T_busy/A`, in wall seconds per audio second, and busy throughput `mu = A/T_busy`, in audio seconds per wall second. Both require positive denominators and a declared comparable sample. They are undefined for missing or zero-duration observations. Success-only samples omit failed attempts and cannot alone predict total operational capacity.

Queued processing work is `W = sum_p(A_queued,p * r_p)` wall seconds, using compatible per-profile observations. It excludes active-job remainder, future arrivals, resource holds, retry work and stages absent from the report. Different devices, model profiles, targets and contention regimes must not be silently pooled. Startup and per-cue overhead may require a model such as `t = b + r*a + h*n`, where `b` is startup seconds and `h` seconds per cue. Its parameters require measurement; the equation is not a selected implementation.

Hypothetical: 120 queued audio seconds at `r=0.5` contribute 60 wall seconds; another profile has 30 seconds at `r=2`, contributing another 60. The sum is 120 worker-seconds. It is not a promise of completion in two minutes or one minute with two workers: profile placement, dependencies and resource contention can prevent that schedule.

For a constant-rate fluid approximation with arrival rate `lambda` and available processing rate `mu`, both in audio seconds per wall second, positive backlog changes at `lambda-mu`. With initial backlog `A0` and `mu>lambda`, approximate drain time is `A0/(mu-lambda)`. This requires sustained comparable rates, continuing admission and no other bottleneck. If `mu<=lambda`, there is no finite drain estimate under these assumptions. If arrivals stop, use `A0/mu`, provided `mu>0`.

Hypothetical: `A0=600`, `lambda=1`, and `mu=1.5` give `600/0.5=1,200` wall seconds, or 20 minutes. At `mu=1`, backlog does not drain. A historical busy rate excludes idle/resource-held periods and therefore cannot be substituted for available `mu` without evidence.

Current [pace](../crates/sigy-service/src/storage/recognition/pace.rs) floors per-profile milliseconds per audio second. [Queue reporting](../crates/sigy-service/src/storage/recognition/queue.rs) sums queued audio by profile, applies the stored pace and rounds each profile's product upward; a zero rounded pace remains separately labeled. Floor-then-ceiling is not an upper confidence bound. [Arrival reporting](../crates/sigy-service/src/storage/recognition/arrival.rs) compares retained admitted audio over its stored clock span with completed busy work using checked cross-products. It remains a descriptive finite sample, not transient ETA or measured free capacity. Same-time admissions have no positive span; observation-boundary effects, clock changes and profile mix limit interpretation. [Doctor](../crates/sigy-service/src/control/doctor.rs) admits no work.

[Little's original queueing paper](https://fisherp.scripts.mit.edu/wordpress/wp-content/uploads/2015/11/ContentServer.pdf) establishes `L=lambda*W` for compatible long-run averages under its conditions. Choose one population and boundary: queued jobs with queue waiting time, or all jobs with total sojourn time. Do not mix them. Hypothetical stationary averages of 0.2 jobs/second and 15 seconds in-system yield three jobs in-system. Three jobs observed now do not establish a 15-second completion estimate. Startup, draining, censored jobs and unstable overload require explicit treatment.

## Fair claims are not equal service time

[Current fairness](../crates/sigy-service/src/fairness.rs) rotates sources and reserves every fourth claim for the oldest eligible batch job. This is a claim opportunity rule, not 25% CPU time or a wall-time starvation bound. Hypothetical claims lasting 1, 1, 1 and 300 seconds give the fourth claim `300/303` of that cycle's elapsed service time.

[Deficit round robin's original publication](https://dl.acm.org/doi/10.1145/217382.217453) studies accounting for variable-sized work. [Dominant Resource Fairness](https://www.usenix.org/legacy/event/nsdi11/tech/full_papers/Ghodsi.pdf) studies heterogeneous multi-resource allocations. They are alternatives to evaluate, not automatic proofs for indivisible, nonpreemptive model jobs. Near-term admission needs bounded bypass, explicit reasons for ineligible large jobs and preserved older-work opportunities. A wall-time bound additionally needs bounded service, cleanup, resource availability and the number of older eligible jobs. Blocked filesystem operations can invalidate that bound.

Bound backlog by count and representation bytes as well as declared work. Hypothetical 1,000 queued descriptors capped at 4 KiB account for 4,096,000 descriptor bytes, but not database indexes, referenced media, models, output or allocator overhead. Unknown work estimates remain unknown; they do not count as zero. Queue admission refusal must not erase accepted history or automatically authorize paid execution.

## Coverage is an interval union within an identity

Use half-open intervals `[start,end)` in one declared recording/sample coordinate. For valid intervals `I_i` and target window `D`, unique coverage is `length(union_i(I_i intersect D))`. Sort, merge overlap/abutment and use checked lengths. Subtract excluded gaps on the same coordinate before reporting applicable covered time. Do not union unrelated sources or clocks.

Hypothetical: `[0,10)` and `[8,15)` seconds contain 17 summed seconds but 15 unique seconds. In target `[0,20)`, uncovered time is five seconds. Repeating the first citation adds no observed time. A second station with its own ten seconds adds source-seconds to workload/observation totals, not ten extra seconds on the first station's timeline.

Keep denominators explicit: planned duration, recorded audio, recognized coverage and translated cues measure different stages. Complete processing coverage does not prove speech, accurate wording or semantic support. Multiple corrections and translations share evidence ancestry; repeated claims are not independent corroboration.

[Task evidence](../crates/sigy-service/src/storage/tasks/evidence.rs) follows exact owned recording/job/revision identities and distinguishes uncovered from unprocessed time. [Monitor coverage](../crates/sigy-service/src/storage/monitors/coverage.rs) counts the current monitor's bounded observations. Current coverage duration sums rely on their validated transcript-coverage contract. Generic cross-recording union and semantic evidence independence remain proposed; do not relabel existing totals as either.

## Storage, exact arithmetic and hostile cases

Physical capacity counts copies, not citations. Hypothetical retention of a 100 MiB source while creating a 100 MiB archival copy and 20 MiB scratch requires 220 MiB of concurrent file capacity, before metadata and protected reserves. Two read leases do not create two media copies, but they prevent reclamation until both are resolved. An offline store cannot be treated as freed space; unknown deletion or copy completion retains its obligation. Logical deduplication saves physical bytes only after exact content/layout verification and lifecycle authority establish a shared object.

Use integer base units and explicit rounding for grants, liability and admission. [SQLite floating-point guidance](https://sqlite.org/floatingpoint.html) distinguishes approximate numbers from exact answers. Its [aggregate documentation](https://sqlite.org/lang_aggfunc.html) distinguishes integer `sum` and floating `total`; [expression rules](https://sqlite.org/lang_expr.html) permit overflow-driven conversion. Validate operand domains and use checked arithmetic rather than assuming an INTEGER column makes every expression exact. Compute `ceil(n/d)` with positive `d` through checked quotient/remainder, avoiding overflow in `n+d-1`.

Required evidence includes overflow/zero/negative inputs, overlapping and out-of-window intervals, gaps and stale revisions, duplicate receipts/interests, crash after reservation but before launch, replaced attempt generations, failed cleanup, output floods, underestimated scratch, device reset, unavailable media stores and uncertain paid completion. Compare interrupted and uninterrupted final charges, effects and coverage using independent expected values. Include boundary-heavy hand examples rather than only examples generated by the implementation under test.

The first package should define resource/accounting types and denominators, preserve present doctor wording, and add atomic reusable attempt claims with capture headroom. Capacity and fairness changes require complete-pipeline workload measurements, failed-attempt accounting and recovery evidence. No mathematical identity by itself qualifies a device, workload or language.
