# Assurance and validation

Status: proposed engineering acceptance plan. No tests or benchmarks described here have been performed. The goal is exceptional product reliability and maintainability, not an aerospace certification program.

## 1. Definition of quality

Sigy should preserve what it successfully collects, explain what it misses, remain controllable under load, respect user policy, and recover without duplicating effects. Its interfaces should make those properties understandable. Model outputs should be useful and traceable, with measured limitations.

Verification checks whether the implementation satisfies a specified contract. Product validation checks whether the experience actually meets the user's needs. Both are required; an extensive unit-test count alone is not release evidence.

Approachability is a release concern. Use representative tasks and participants new to the relevant tools to measure completion, time to first useful result, assistance needed, recovery from mistakes, and understanding of source/processing/spending state. Set acceptance targets before usability evaluation. Broad capability claims need a defined task catalog and denominator.

## 2. Requirements and verification matrix

All detailed requirements below are proposed refinements of confirmed product commitments. Each needs a corresponding test/evidence record before a release claim.

| ID | Source | Requirement | Verification |
| --- | --- | --- | --- |
| R-01 | C-01 | Core CLI/TUI journeys operate natively on all declared supported platforms | Native installation and interaction matrix |
| R-02 | C-03 | Closing or crashing a terminal client leaves accepted captures and monitors running | Terminate clients during multi-source runs; reconnect and compare state |
| R-03 | C-03, C-10 | Accepted durable work survives a service restart according to its recovery policy | Crash at job-acceptance and transition boundaries |
| R-04 | C-05, C-10 | Recorded gaps, truncations, and failures cannot appear as complete capture | Known-timeline fixtures and fault injection |
| R-05 | C-10 | A finalized segment's identity, checksum, and timing remain verifiable | Integrity checks before/after restart, retention, export, and restore |
| R-06 | C-05, C-13 | Concurrent capture remains inside the declared resource profile | Sustained load with decoder, model, and UI contention |
| R-07 | C-02 | Live original text and translated text expose provisional/final state and delay | Timestamped speech fixture and caption revision inspection |
| R-08 | C-02, C-10 | Batch reprocessing preserves input and output revision provenance | Repeat with a different model/configuration and inspect history |
| R-09 | C-04, C-14 | Effective processing destination matches configured policy | Network-observation tests for local, LAN, hosted, and fallback paths |
| R-10 | C-07, C-11, C-37 | Autonomous source changes remain inside source, time and resource policy and expose their recorded basis, policy version, outcome and resource/cost effects | Generated-plan fuzzing, disallowed-operation fixtures, decision-history review after reconnect and no fabricated rationale for missing history |
| R-11 | C-07, C-37 | Findings reference existing evidence and expose conflicting reports, duplicate-source relationships, unresolved independence and collection/processing coverage | Citation/range validation, repeated/syndicated and contradictory reports, missing-coverage fixtures and labeled briefing review |
| R-12 | C-10 | Source content cannot grant permissions or become executable control input | Adversarial metadata/transcript and plan-boundary tests |
| R-13 | C-15 | Every paid request is admitted against all applicable budget reservations | Concurrent last-balance races and transaction-interruption tests |
| R-14 | C-15 | Uncertain submitted charges survive restart and cannot free budget prematurely | Timeout, missing-final-usage, restart, and period-rollover scenarios |
| R-15 | C-15 | No unapproved paid route, retry, tool call, or fallback can execute | Routing/capability fixtures and audited request records |
| R-16 | C-15 | Spending is explainable by request, monitor, provider, and period | Reconciliation with deterministic and later bounded live billing fixtures |
| R-17 | C-10 | Slow consumers and failed workers cannot cause unbounded queue growth | Deliberately blocked subscribers and stalled workers |
| R-18 | C-10 | Storage exhaustion preserves committed data and stops work according to policy | Full-disk and reserved-space tests |
| R-19 | C-10 | Backup and restore preserve a consistent catalog/media relationship | Restore onto a clean host and compare evidence manifests |
| R-20 | C-01 | Machine output remains versioned, parseable, and free of terminal controls | Redirected stdout/stderr and schema contract tests |
| R-21 | C-01 | Focus, resizing, Unicode, and keyboard navigation preserve user work | Terminal interaction fixtures and user review |
| R-22 | C-06 | Later hardware support includes actual device and disconnect acceptance | Declared device/firmware/OS matrix |
| R-23 | C-06 | Conflicting hardware consumers cannot silently retune an active acquisition | Scheduler/device ownership tests |
| R-24 | C-12 | Later music rankings preserve detection method and sample coverage | Labeled music and duplicate-feed corpus |
| R-25 | C-08 | Client-facing contracts remain independent of UI-specific logic | A second test client exercises the same operations and events |
| R-26 | C-10 | Migration/update failures preserve a documented recovery path | Interrupted upgrade and tested restore/rollback scenarios |
| R-27 | C-10 | Secrets are absent from ordinary logs, exports, and process error displays | Redaction fixtures and diagnostic-bundle review |
| R-28 | C-09, C-16 | Stack selection follows requirements and documented evaluation | Review the decision record and evidence links before implementation |
| R-29 | C-17 | Language decisions apply to observed blocks/spans, preserve mixed/unknown states, and default translation to English | Incorrect station-language hints, multilingual interviews, code switches, short/no-speech intervals, and scoped overrides |
| R-30 | C-17 | Qualified language/task profiles meet separate quality and latency criteria with originals retained | Majority non-English corpus, per-language/condition results, reviewer translation checks, and script/RTL terminal fixtures |
| R-31 | C-18 | Music identification measures regional/non-English coverage and unknown airtime | Majority non-English examples, catalog gaps, alternate scripts, multilingual songs, false-match and denominator review |
| R-32 | C-19 | New typed non-audio sources and transforms share jobs, evidence, and retention without fabricated audio/text | IQ, packet, telemetry, symbol and opaque-data fixtures; reject incompatible types and preserve clocks/gaps |
| R-33 | C-20 | Morse decoding reports uncertainty and preserves replayable input alignment | Independent standard/real-signal fixtures across timing, interference, speed, gaps and prosigns |
| R-34 | C-21 | Enigma output and visible stepping trace match the specified variant | Independent known-answer vectors, double stepping, ring/plugboard cases, deterministic reset/replay |
| R-35 | C-22 | Modern crypto uses qualified profiles and protects operational keys | Conformance/interoperability, tamper and wrong-key tests, nonce concurrency/restart tests, no secret leakage |
| R-36 | C-23, C-37 | Exploration/workbench journeys are engaging and let users inspect inputs and intermediate results, explain displayed activity and identify uncertainty alongside active collection | Newcomer explanation/replay tasks and enjoyment review, keyboard/reduced-motion layouts, synthetic-origin labels, ambiguity and resource contention |
| R-37 | C-10, C-11 | Discovered URLs and nested resources cannot bypass configured host/network boundaries | Redirect, DNS/address, private-endpoint, protocol and decoder-subresource fixtures |
| R-38 | C-01, C-10 | Release/update and local-control paths protect installation and library access | Unauthorized clients, artifact substitution, interruption and clean-host restore tests |
| R-39 | C-24 | Supported everyday workflows are discoverable and usable without specialist prerequisite knowledge | Newcomer task sessions, preset/error-recovery review, comprehension of results and limits, advanced-control discoverability |
| R-40 | C-25, C-26 | Releases include the unmodified Apache 2.0 license, applicable third-party notices, and accurate lawful-use/warranty documentation | Artifact/license comparison, dependency/model/data inventory and distribution review, README consistency |
| R-41 | C-27 | Qualified local profiles provide language detection and live/batch speech processing with paid processing disabled | Zero-budget end-to-end runs; network/destination checks; per-language results, separate capture/analysis limits and explicit unsupported states |
| R-42 | C-05, C-13, C-27 | Sustained multi-stream processing has bounded durable backlog, source fairness, and finite retention dependencies | Arrival-rate overload, starvation, model-load churn, thermal contention, restart, queue-age and catch-up evidence |
| R-43 | C-15, C-17, C-28 | Any adopted classifier has measured end-to-end utility, per-language acceptance/abstention, and bounded paid attempts | Held-out original/translated comparisons, rare-topic misses, calibration/drift tests, hidden-retry and price-bound fixtures |
| R-44 | C-07, C-12, C-18 | Music counts and weekly comparisons are reproducible from deduplicated observations with coverage denominators | Crossfade/partial-play/version fixtures, unknown airtime, stable-panel comparison, boundary-time and taxonomy revisions |
| R-45 | C-07, C-10 | Evolving topic context retains evidence lineage, revision history, user annotations, and policy separation | ASR/identity corrections, contradictions, expired/deleted evidence, model changes, export and restore review |
| R-46 | C-29 | Every functional TUI operation is available through a complete noninteractive CLI over the same policy-enforced application operations | Action parity matrix; CLI-only setup, station, DVR, processing, monitor, cost and recovery journeys; redirected-input/output checks |
| R-47 | C-30 | Catalog refresh preserves user identity links and history while exposing partial/stale results | Mirror/pagination failures, renamed/missing stations, URL revisions, preserved favorites, stale search responses and bounded refresh traffic |
| R-48 | C-31 | DVR playheads, retained intervals, saved recordings and schedule occurrences remain distinct and recoverable | Expiry/save races, two clients, gaps, disk pressure, restart, daylight-saving and missed/duplicate occurrence fixtures |
| R-49 | C-32 | Globe/map/list views provide consistent keyboard discovery and bounded rendering on declared terminal profiles | Rotation, zoom/clusters, query agreement, small/monochrome/SSH modes, resize, reduced motion and capture-under-render-load tests |
| R-50 | C-32, C-19 | Geographic and signal displays expose actual location/measurement basis and distinguish solar context from reception | Unknown/coarse coordinates, fixed-date sun cases, stale data, audio-versus-RF labels and source-type fixtures |
| R-51 | C-33 | Later podcasts/feeds support bounded, idempotent, multilingual incremental analysis with reproducible evidence | Feed/episode revision and duplicate fixtures, XML/network boundaries, partial downloads, supplied-transcript alignment, queue fairness and cost limits |
| R-52 | C-19, C-36, C-37 | Both clients expose an aligned path from findings through distinct interpretations and representations to retained originals and context, with unknown or expired evidence explicit | Multilingual finding-to-recording journeys, missing/expired ranges, alternative interpretations, location provenance and later typed non-audio replay fixtures |
| R-53 | C-10, C-15, C-37 | Scoped corrections preserve originals and history, identify stale dependents, and admit any reprocessing through existing resource, destination, retention and cost policy | CLI/TUI correction journeys, dependent and unaffected result checks, concurrent edits, interrupted rebuild, exhausted allowances, unavailable input and old-report comparison |
| R-54 | C-03, C-31 | Recording reservations are atomic, replay cannot dispatch twice, and age/quota cleanup protects kept/archive and active media | Concurrent reservations, 14-day expiry, pressure ordering, process kill with partial bytes, protected-full quota and interrupted deletion fixtures |
| R-55 | C-19, C-38 | Versioned recording exports preserve source/clock/payload lineage without inventing RF or language observations | Sidecar roundtrip, byte/hash evidence, missing/deleted media, typed-profile compatibility and future SigMF interoperability |
| R-56 | C-06, C-39 | RF detections distinguish measured activity, candidate identities and decoded content within actual device capabilities | Device/region profile validation, scan gaps, ambiguous station matches, numeric units, disconnect and tuner-conflict tests |

## 3. Capacity profiles

Two machine classes are confirmed. Exact supported hardware and workload limits remain to be measured.

| Profile | Intended use | Evaluation bands, not promised support |
| --- | --- | --- |
| Small always-on host | Reliable capture and selective local or network processing | Test 1, 2, 4, and 8 radio captures; test zero, one, then multiple live speech pipelines until sustainable limits are known |
| Desktop | Interactive exploration and more concurrent local analysis | Test 1, 4, 8, and 16 captures; scale ASR/translation independently with CPU and available acceleration |
| Hardware receive | Future SDR operation | Test supported sample formats/rates, demodulators, drop counters, and capture duration within disk limits |

For every result, record CPU, RAM, OS, architecture, storage, network conditions, accelerator/driver, model hashes, quantization, runtime versions, stream formats, language mix, and thermal/power conditions. A LAN processing profile reports both host and server characteristics.

Capture capacity and live-analysis capacity are separate numbers. A small host may capture several streams while processing one locally, queueing others, or using an explicitly configured network runtime.

Qualify sustained batch throughput, queue-age bounds, power/thermal behavior, and storage growth alongside simultaneous stream counts. [Local-processing research](../../research/17-local-processing-and-capacity.md) defines capacity arithmetic and the full mixed-workload experiment. A $0 API budget means no metered model-service fees, not zero hardware or operating costs.

## 4. Proposed measurable targets

These numbers are initial design targets for discussion and benchmarking. They are not established capabilities or launch commitments.

| Metric | Candidate target | Measurement scope |
| --- | --- | --- |
| UI response | p95 under 100 ms for local navigation/input | Declared terminal and library-size fixture, under supported capture load |
| Service control | p95 under 250 ms for local state/control acknowledgment | Excludes source connection and model loading; durable submission measured separately |
| Provisional captions | p95 under 5 seconds after the relevant audio reaches Sigy | Qualified live profile; source delivery delay reported separately |
| Final translation | p95 under 15 seconds after utterance end reaches Sigy | Defined language pair/model profile and speech-boundary rule |
| Restart readiness | Recover local control and classify interrupted work within 10 seconds for the reference fixture | Model warm-up, source reconnect, and very large libraries measured separately |
| Uncommitted capture window | At most 5 seconds of received media under the selected durable profile | Verify segment/flush strategy; host power-loss behavior has explicit filesystem assumptions |
| Integrity | Zero falsely complete or silently corrupt records in the release fault suite | Every relevant storage/commit transition exercised |
| Cost admission | Zero submissions beyond authorized reserved liability under validated billing contracts | Includes concurrency, retries, unknown charges, and period rollover |
| Soak behavior | 72-hour development runs and a 7-day release-candidate run on each supported profile class | No unexplained data loss, runaway memory, orphan workers, or unauthorized requests |

Define histograms and denominators before collecting performance numbers. Report cold starts, warm operation, steady load, overload, and recovery separately. Percentile targets cannot hide a permanent tail of stuck jobs.

Model quality thresholds must be set by language/task after assembling representative data. Inventing one universal error threshold now would hide meaningful differences between clean news, noisy call-ins, music, and low-resource languages.

A majority of speech evaluation duration and music examples must be non-English, with minimum coverage per declared language and condition. Report language identification, ASR, translation, and end-to-end topic retrieval independently. A strong English result cannot offset failure in another declared language. The [multilingual evaluation plan](../../research/10-multilingual-processing.md) defines the proposed survey and scorecard.

Requirements R-31 and R-33 through R-35 qualify their respective future releases; they do not silently expand the first-release milestone. R-32 establishes first-release architecture contracts and later device/decoder acceptance separately. The radio explorer provides first-release evidence for R-36; workbench evidence follows its chosen milestone.

R-44 qualifies the post-release music milestone. R-43 applies to whichever classifiers are adopted; it does not require Jev or a hosted classifier in the first release. R-45 initially covers monitor overview/history and evidence; advanced graph exploration remains a later decision.

R-46 through R-50 refine the first-release radio and terminal experience, with RF-specific visualizations qualified alongside hardware. R-51 qualifies the later podcast/feed milestone; common typed-artifact and job contracts are designed now.

R-52 and R-53 qualify the first-release radio evidence and correction journeys. Their shared contracts cover later representations without requiring every decoder or hardware adapter in that release. Corrections must remain useful when reprocessing is paused: show the accepted correction, affected results and reason for waiting without presenting stale output as current.

## 5. Failure and recovery analysis

| Failure | Required response | Evidence to collect |
| --- | --- | --- |
| Directory unavailable | Use labeled cached data and direct sources; bounded retries | Search state and retry history |
| Stream stalls or changes codec | Mark interruption/configuration change; preserve prior segments | Source timeline and media validation |
| Capture worker crashes | Isolate other captures, classify incomplete segment, restart per policy | Worker exit, owned-process cleanup, gap interval |
| Inference hangs or exhausts memory | Bound/cancel or replace worker; retain capture and queued work | Queue age, process memory, preserved segments |
| Service dies after file finalization | Reconcile orphan media on restart | Catalog/object manifest comparison |
| Service dies after paid submission | Retain reserved liability and reconcile | Durable attempt record and provider ID if obtained |
| Provider changes price or ignores required limits | Refuse new incompatible paid work | Price/capability snapshot and rejection reason |
| Budget races under concurrency | Admit only what the serialized reservations permit | Atomic ledger history |
| Disk is full, read-only, or missing | Stop affected writes cleanly; keep committed state inspectable | Preserved files, status transitions, diagnostics |
| Host sleeps, reboots, or changes time | Record missed time and apply recovery/schedule policy | Clock events, schedule occurrences, gaps |
| Client is slow or disconnects | Drop/compact notification delivery with resync; retain jobs | Cursor gap and recovered snapshot |
| Device disappears or is in use | Mark unavailable/conflict, release or retain lease per policy | Device identity and ownership history |
| Source content contains instructions | Preserve as data; reject policy-changing proposals | Rejected action and intact monitor bounds |
| Transcript is revised | Retain old result, mark/recompute dependents | Revision graph and briefing status |
| Update/migration is interrupted | Recover documented compatible state or restore | Versioned backup and recovery result |

## 6. Test design

### Deterministic core

Use reproducible clocks, scheduler inputs, resource limits, source events, and provider responses. Test job state transitions, ownership, references, revisions, accounting, retention, and schema compatibility independently from public networks.

Property-oriented checks include nonnegative available balances under admission rules, bounded queues, no unauthorized state transition, no loss of pinned references during retention, idempotent recovery, and no overwrite from a stale worker generation.

### Parser and protocol boundaries

Fuzz directory responses, playlists, URLs, stream metadata, provider chunks, configuration, import manifests, and later hardware frames. Bound input sizes, nesting, decompression, and parser work. Feed terminal control characters through station names and transcript text to verify literal rendering.

### Integration fixtures

Use local deterministic streams with known audio/sample timing, disconnections, delayed segments, redirects, malformed metadata, and codec changes. Provider fixtures model partial SSE, inconsistent usage, timeouts, throttling, unsupported parameters, and billing uncertainty.

### System and manual validation

Test actual installers, service managers, audio devices, terminals, sleep/resume, and supported filesystems. Include exploratory keyboard-only sessions and library recovery by someone following the documentation. Later hardware tests use physical devices, not only mocks.

### Model evaluation

Version corpora, expected passages, review rubrics, and model configurations. Hold back a test subset from prompt/profile tuning. Evaluate source-language fidelity, translation meaning, evidence retrieval, citation support, false positives, and unknown cases. Use qualified language review for launch language pairs.

Public live stations provide a limited smoke test after deterministic verification. They are too variable to be the sole regression suite.

## 7. Observability and maintenance

Expose source health, capture health, processing backlog, queue depth, last successful segment, dropped samples, storage reserve, worker restarts, provider latency, and cost state. Correlate events with source, job, monitor, attempt, and request IDs.

Diagnostic bundles are local and inspectable by default, with secrets and unnecessary source payloads redacted. Metrics have a bounded retention policy. Logging must not exhaust the same disk reserve needed to preserve captures.

Maintain versioned contracts, dependency inventories, supported-profile results, reproducible build instructions where feasible, migration notes, and operational runbooks. Native/unsafe boundaries receive explicit ownership and callback-lifecycle review regardless of language.

## 8. Release evidence

A release candidate needs a requirements-to-evidence map, platform results, media-integrity results, soak results, declared model quality/latency profiles, cost-control fault results, restore demonstration, package verification, and known limitations.

Release blockers include silent capture loss, falsely complete media, budget-bound violations, unauthorized destinations, invalid evidence references, and inability to recover a supported library. Cosmetic polish and documentation are also required, but they cannot compensate for a failed integrity or policy contract.
