# Delivery and decisions

Last updated: 2026-09-20. Status: product decision and evidence register during incremental implementation.

## 1. Current scope

The initial documentation-only phase is complete as a checkpoint. The user has now authorized evaluation and implementation, with a cumulative USD 10 external-spend ceiling and a preference for zero paid use. [Active work](../development/progress.md) records implementation evidence and accounting. Installed system services, releases, and paid experiments retain their specific operational controls.

The present package is a research-backed draft. Detailed choices marked proposed do not become approved merely because the document is long or the research is extensive.

## 2. Decision gates

| Gate | Reviewable result | Completion condition |
| --- | --- | --- |
| G0: Intent | Intent, confirmed requirements, complete first-release scope | Product goals and exclusions are represented accurately |
| G1: Product behavior | User journeys, CLI/TUI contract, monitoring policy, translation interpretation, cost behavior | Important user-facing ambiguities are resolved or explicitly bounded |
| G2: System design | Component contracts, lifecycle/time model, data model, failure analysis, deployment profiles | Responsibilities and invariants are testable without choosing a language |
| G3: Evaluation specification | Candidate technologies, comparable workloads, data/quality metrics, hardware matrix, scoring criteria | Experiments answer actual open decisions; no technology wins by assumption |
| G4: Technology decision | Results from separately authorized post-planning experiments, maintenance review, decision record | Chosen stack meets hard requirements and its tradeoffs are documented |
| G5: Implementation baseline | Selected versions, schemas/protocols, interface specifications, verification plan, deliverable slices | Work can begin against an agreed design with acceptance criteria |
| G6: Release | Complete first-release workflows and assurance evidence | All release-blocking requirements pass on declared supported profiles |

The [foundation decision](../decisions/0001-rust-foundation.md) selects Rust and SQLite for initial implementation using research and bounded local evidence. The full G4 qualification matrix is not complete, and later subsystem/platform gates remain open. Do not equate a foundation choice with product or release qualification.

## 3. Decision register

Confirmed entries are summarized in the [planning index](README.md). These are the remaining design decisions, with proposed defaults only where useful.

| ID | Decision | Proposed direction or alternatives | Evidence needed |
| --- | --- | --- | --- |
| D-01 | Working product name | Sigy remains temporary; prioritize recognizable meaning. Current user candidate Signeta has a near-name Signetta software collision; SignalSift has exact-name analysis-product collisions. See [naming research](../../research/21-naming.md) | Name decision, confusingly similar uses, distribution/domain checks and appropriate trademark review before public release |
| D-02 | Exact capacity profiles | Both small always-on hosts and desktops are confirmed; stream and analysis counts remain unqualified | Representative machines, OS/architecture choices, benchmark corpus |
| D-03 | Translation output | Captions first; spoken translation separately scoped | User preference and delay/audio-mixing requirements |
| D-04 | Qualified language coverage | Non-English majority and translation primarily to English are confirmed; define meaningful launch language/task profiles and experimental coverage | Representative stations, regional priorities, mixed-language corpus and competent review |
| D-05 | Service across logout/reboot | Offer clearly named per-user and unattended modes | Desired default and platform-installation review |
| D-06 | Retention defaults | Finite storage/time policies, pinned evidence, visible disk reserve | Typical collection duration and storage budget |
| D-07 | Paid budget defaults | Paid disabled until finite provider/task policies are configured | Confirmation of periods, strict-mode UX, and provider tests |
| D-08 | Distribution and dependency licensing | Apache License 2.0 confirmed; packaging, channels, and third-party integration terms remain open | Dependency/model/data license inventory, linking/bundling review, required notices and distribution policy |
| D-09 | Supported platform versions | Native Windows/macOS/Linux; exact versions/architectures undecided | Maintenance horizon and CI/hardware availability |
| D-10 | Application language | Rust selected for the foundation; Go comparison preserved in the decision record; Python excluded | [Decision and limitations](../decisions/0001-rust-foundation.md); broader platform and workload qualification continues |
| D-11 | Media backend/boundary | Supervised workers, embedded libraries, or pipeline engine | Timing, fault isolation, packaging, copy and resource measurements |
| D-12 | Catalog/search | SQLite selected for the initial catalog; multilingual search remains open | Initial transactional ledger tests; further job/media recovery, backup, growth, and retrieval evidence |
| D-13 | Local control transport | Protected local IPC or authenticated loopback interface | OS identity, future-client needs, operational complexity |
| D-14 | Model/runtime defaults | Capability-based adapters; Ollama and OpenRouter targets confirmed | Language quality, latency, footprint, price-bound support |
| D-15 | Media archive format | Source-preserving segments where reliable, or documented lossless normalization | Seek, recovery, format coverage, storage and license testing |
| D-16 | Remote Sigy host control | Architecture-compatible, first-release inclusion undecided | Desired SSH/LAN workflow and remote playback requirement |
| D-17 | External briefing delivery | In-app and export proposed initially | Desired destinations, account access, delivery/cost scope |
| D-18 | Hardware pilot | Meshtastic connected-node and HackRF Pro receive tests later | Actual devices, firmware, antenna/setup, OS hosts |
| D-19 | Music provider/catalog | Post-release metadata plus evaluated fingerprint/catalog adapters | Regional coverage, false-match rate, fees and terms |
| D-20 | Terminal geography and visualizer profile | Rotatable globe, flat day/night map, linked lists, and truthful activity views confirmed; evaluate text-cell rendering with accessible fallbacks | Geometry/time fixtures, keyboard journeys, terminal matrix and resource measurements before a UI framework is chosen |
| D-21 | Morse/workbench release placement | Proposed post-release milestone independent of hardware delivery | Product priority, bounded initial cipher/decoder scope, impact on full first release |
| D-22 | Modern crypto profiles and release placement | Authenticated encryption and post-quantum operations with supplied keys confirmed; exact profiles/libraries unresolved | Maintained implementations, standards/errata, interoperability, key lifecycle and review |
| D-23 | Encryption of library or exports | Separate from workbench operations; inclusion undecided | Metadata confidentiality, search/unlock behavior, unattended startup, backup/recovery requirements |
| D-24 | Extension execution and trust | Built-in modules and supervised typed workers are candidates; third-party plugin loading separately scoped | Throughput, cross-platform isolation, device/FFI needs, schema compatibility and distribution |
| D-25 | Historical and Morse profiles | Enigma I and International Morse are proposed starting profiles; other variants/ciphers extend later | Exact reference conventions, independent fixtures, alphabet/prosign behavior and interaction review |
| D-26 | Semantic classifier profiles | Compare rules, local classifiers, local constrained generation, and optional Jev through OpenRouter; no classifier required by name | Per-language end-to-end recall/precision, calibration, native runtime parity, strict billing contract and total cost |
| D-27 | Persistent topic context | First-release overview/history/evidence; richer linked notebooks and external tool access remain optional | Revision/correction journeys, retrieval/support quality, user annotations, export and maintenance burden |
| D-28 | Live/batch scheduling policy | Deadline-aware admission, source fairness, bounded backlog and retention; no automatic paid overflow | Sustainable arrival/service rates, starvation/overload tests, latency-quality tradeoffs on both machine profiles |
| D-29 | Directory freshness and reconciliation | Cached usable generations, bounded refresh, preserved favorites and explicit health/location provenance | Provider limits, changed-list completeness, mirror/pagination faults and source-identity fixtures |
| D-30 | DVR and recording schedules | Finite rolling buffers, independent playheads, saved-interval promotion and station/time recurrence | Buffer ownership/defaults, codec seeking, storage/expiry races, time-zone/DST and recovery review |
| D-31 | Podcast/feed integration profile | Post-release RSS/Atom and finite podcast media using shared local analysis; linked articles and discovery providers separately scoped | Format/namespace support, private-feed policy, polling/backfill limits, media revisions, transcript alignment and mixed-source evaluations |
| D-32 | Repository and dependency organization | One repository, a few packages with explicit ownership, typed integrations; Rust-first profile remains proposed | G4 stack evidence, actual dependency graph, packaging and build isolation; [organization proposal](12-repository-and-engineering.md) |
| D-33 | Engineering assessment policy | Periodic OpenSSF Scorecard review after implementation/hosting, individual findings and honest limitations | Confirm scoring tool, qualified release, cadence, hosting controls and publication preference; no score currently assessed |

Resolve decisions in small related groups. Implement and measure bounded choices without presenting unresolved product behavior as confirmed.

## 4. Research and future experiment register

| Work package | Question | Required output |
| --- | --- | --- |
| E-01: Directory survey | Can representative radio sources be discovered and resolved reliably? | Dated sample, format/metadata coverage, failure taxonomy |
| E-02: Capture integrity | Which backend/format survives faults while preserving timing? | Identical fixture comparison, recovery matrix, archive recommendation |
| E-03: Native services | How do session, logout, reboot, permissions, and updates behave? | Platform-mode contract and installation evidence |
| E-04: TUI experience | Which implementation supports the required terminal behavior? | Comparative keyboard/resize/Unicode/large-data results |
| E-05: Speech and translation | Which profiles meet quality and latency on each host class? | Frozen evaluation corpus, results by language and condition |
| E-06: Catalog and search | Can the catalog recover and retrieve evidence under sustained use? | Fault/backup results, retrieval benchmark, growth estimates |
| E-07: Paid providers | Can every allowed billing dimension and fallback be bounded? | Adapter contract, reservation/reconciliation fixtures, later bounded live results |
| E-08: Monitoring quality | Does autonomous discovery produce relevant, well-supported and explainable coverage? | Labeled topic windows, recorded source-selection decisions/outcomes, duplicate/contradictory reports, coverage gaps and report-quality results |
| E-09: Rust/Go comparison | Which candidate best satisfies the full architecture and maintenance needs? | Comparable workload results and reviewed decision record |
| E-10: Hardware replay/pilot | Are adapter and tuning contracts correct on real devices? | Replay fixtures followed by device/firmware/OS results |
| E-11: Music identification | Which method identifies the intended regional sample reliably? | Catalog coverage, false/unknown match rates, deduplication and cost evidence |
| E-12: Multilingual blocks | Which detection/routing strategy handles the expected non-English majority and code switching? | Confusion/abstention and span results, original/translated retrieval, per-language/condition quality and cost |
| E-13: Typed signal extensions | Can sources beginning at different signal layers share durable contracts correctly? | IQ/event/symbol replay, clock mapping, schema compatibility, throughput and isolation results |
| E-14: Morse and historical workbench | Are decoding and historical traces correct and enjoyable to use? | Independent Morse/Enigma fixtures, uncertainty and stepping evidence, keyboard/replay usability review |
| E-15: Modern cryptography | Which profiles and implementations meet operation and key-lifecycle requirements? | Conformance/interoperability, tamper/nonce/restart tests, key-storage/recovery design and focused review |
| E-16: Security and release boundaries | Can the chosen media, IPC, credential and distribution paths enforce application policy? | Redirect/nested-resource and import fixtures, access-control tests, signed-package/update and restore evidence |
| E-17: Classifier cascade | Do local or hosted decisions improve useful analysis without unacceptable missed events? | Majority non-English original/translated baselines, held-out calibration, false-negative audit, resource/cost comparison, endpoint billing qualification |
| E-18: Persistent topic context | Can users inspect and correct findings across contradictions, retention and model changes? | CLI/TUI finding-to-original and correction journeys, scoped dependency invalidation, bounded interrupted/deferred rebuilds, preserved report history, evidence/support checks and export/restore results |
| E-19: Local processing at scale | Which mixed capture/detection/ASR/translation workloads remain sustainable with no metered inference? | Both host classes, live/batch service curves, quality by language, backlog fairness, retention and thermal/storage evidence |
| E-20: Terminal explorer and parity | Can each viable stack deliver the globe/day-night/list experience without impairing capture or CLI completeness? | Comparable projection, marker, Unicode, keyboard, output-bandwidth, slow-terminal and full CLI workflow evidence |
| E-21: Catalog and DVR lifecycle | Do refresh, shared buffers, independent playback and recording schedules recover correctly? | Identity and stale-data fixtures, seek/gap cases, promotion/expiry races, storage faults, DST and occurrence replay |
| E-22: Podcasts and feeds | Can bounded subscriptions and finite episodes improve cross-source insight without duplicate work? | RSS/Atom/media revision fixtures, language/evidence quality, security boundaries, incremental-cost and mixed-load results |

The user has authorized implementation and bounded experiments. E-07 live paid validation still needs a mechanically enforced allocation inside the cumulative development ceiling. Initial verification uses local deterministic fixtures and no paid calls.

## 5. Planning completeness and honest uncertainty

The design package must cover discovery, listening, recording, multilingual live and batch processing, autonomous monitoring, evidence, storage, costs, service operation, terminal experience, extensible non-audio sources, hardware, music, Morse, historical and modern cryptography, delivery, and maintenance. Exploration and fun need explicit journeys as well as functional tests. Each material design choice needs a reason, alternatives, and a verification path.

Some facts can only be learned through prototypes, real models, and physical devices. Document these as experiments; do not substitute confident prose for measurement. Freeze only the decisions for which sufficient evidence exists. Revise the plan when new evidence changes an assumption.

## 6. Change control and document ownership

Keep one authoritative location for each confirmed requirement and open decision. Update the intent and roadmap when scope changes, then update affected requirements, designs, research, and acceptance cases.

Future architecture decisions record context, alternatives, selection, consequences, evidence, and the conditions that would justify reconsideration. Public authorship and repository attribution follow the repository instructions. Preserve required third-party legal notices.

[AGENTS.md](../../AGENTS.md) holds the canonical shared development instructions. It points to these contracts rather than duplicating the entire design. [Repository engineering](12-repository-and-engineering.md) defines proposed layout and future verification ownership.

## 7. Immediate review order

1. Review intent and full product scope.
2. Review translated-caption behavior, service modes, and storage expectations.
3. Review the provider/cost policy and autonomous monitor boundaries.
4. Review capacity and quality targets, including language qualification.
5. Review architecture alternatives and the Rust/Go evaluation criteria.
6. Finalize the documentation baseline before considering implementation or experimental code.

## 8. Current checkpoint: 2026-09-20

Historical documentation checkpoint, retained for traceability. Subsequent implementation is recorded in [active work](../development/progress.md); the user has advanced the phase and accepted Sigy as the working name.

The intent, roadmap, confirmed requirements, subsystem designs, research notes, assurance requirements, and canonical agent instructions form a reviewable documentation baseline. The baseline is a draft, not approval of every proposed design or completion of G0 through G3. There is no application, build configuration, test suite, CI, release, or measured capability profile.

The first complete release remains worldwide radio exploration, recordings/DVR, live translation, and bounded autonomous topic monitoring through a complete CLI and optional TUI. Persistent service operation, predominantly non-English material, local processing, extensible signal types, and enforced paid-provider limits are confirmed constraints. Later music, feeds, hardware, and workbench capabilities remain planned.

Resume with these bounded decisions:

1. Resolve D-01 naming. Sigy is temporary. Signeta is the latest user suggestion; exact and similar-name findings are recorded in [naming research](../../research/21-naming.md). Suggestions in that note are not selected names.
2. Review D-03 through D-06: translated captions versus spoken output, initial language qualification, service behavior across logout/reboot, and retention defaults. Confirm representative hardware for D-02 and supported platforms for D-09.
3. Review the [architecture](02-architecture-and-data.md), [spending policy](07-providers-and-cost-policy.md), and [assurance requirements](04-assurance-and-validation.md), then close or explicitly bound the remaining G0 through G3 questions.
4. Review the [stack trade study](06-language-and-stack-trade-study.md) and [repository organization](12-repository-and-engineering.md). Rust is the leading candidate to evaluate; Go remains a comparison candidate. No stack or dependency set is selected.
5. Only after the documentation review and explicit phase advancement, run bounded evaluation work from section 4. Record measured results before the G4 stack decision or implementation.

No prototypes, model downloads, paid inference, hardware tests, capacity benchmarks, or Scorecard assessment have been performed. Future work should update these canonical documents rather than starting a competing plan or treating research as shipped behavior.

Checkpoint verification covered all 40 Markdown files and 177 local links, including anchors, register continuity and references, writing rules, and basic credential-pattern checks. No issues were found. The license matches the official Apache 2.0 text. These checks validate documentation integrity, not application behavior or every external source's continuing availability.

The user authorized a private planning repository at [blisspixel/sigy](https://github.com/blisspixel/sigy). The local Git repository uses `main` and an `origin` remote pointing there. This hosting choice preserves Sigy as a working name; D-01 remains open. Local scratch state under `.agents/` is excluded from version control. Check Git status and the upstream commit before claiming a later session is synchronized.
