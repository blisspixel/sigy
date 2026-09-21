# Roadmap

Last updated: 2026-09-20

This roadmap is organized by evidence and exit criteria. It does not assign speculative completion dates. Implementation has begun on the Rust foundation; the complete release remains ahead. [Current evidence and active work](docs/development/progress.md) distinguish completed slices from these planned stages.

The product journey is to explore signals and discover their meaning in context. Each milestone must make the path from observation to interpretation inspectable, preserve uncertainty, and support learning and correction. These are planned acceptance requirements, not claims about the current implementation.

| Stage | Scope | Exit criteria |
| --- | --- | --- |
| 0. Intent and research | Intent, multilingual journeys, aggregators, agentic analysis, local/hosted classifiers, typed signals, workbench/cryptography, architecture alternatives, cost policy and open decisions | The complete product is described, consequential unknowns are visible, and the research supports a reviewable design |
| 1. Design baseline | Resolve first-release behavior, representative everyday tasks, guided/advanced interactions, capacity profiles, language quality targets, service modes, retention, and distribution | Reviewed requirements, interface contracts, failure behavior, usability/verification plan, and technology-evaluation criteria |
| 2. Technology evaluation | After the documentation phase: bounded experiments comparing viable language, terminal, media, storage, and inference choices | Measured results on representative machines; a written stack decision with tradeoffs and rejected alternatives |
| 3. Durable foundation | Service lifecycle, job journal, typed artifacts and interpretation lineage, resource admission, media segmentation, recovery, local API, budget ledger | Crash, restart, disk pressure, concurrent reservation, backup, and restore tests pass; originals, context, revisions and gaps remain distinguishable |
| 4. Radio experience | Refreshable station catalogs, complete CLI and optional TUI, search/filters, globe and day/night map, audio/activity visualizers, DVR buffers, saved intervals, recording schedules and library, source context and guided inspection | CLI/TUI parity, geographic/terminal and audio tests, catalog freshness/identity recovery, DVR expiry/scheduling tests and reliable multi-source capture; users can explain displayed activity and inspect retained source intervals |
| 5. Language processing | Per-block language spans with mixed/unknown states, local live/batch ASR and fair bounded queues, aligned originals and English translation, scoped corrections, provider configuration, local/LAN/remote routing and paid limits | Majority non-English quality/latency results; visible uncertainty and correction history; sustainable local profiles with a zero paid budget; no unapproved remote requests; billing fault tests pass |
| 6. Topic monitoring | Bounded source discovery with decision history, scheduling, coverage, evaluated optional classification, deterministic statistics, evidence-linked briefings, contradictions and revision-aware topic history | End-to-end topic and missed-event evaluation; duplicate-report and coverage review; finding-to-original navigation; revision/evidence preservation, bounded correction propagation, restart recovery, policy enforcement and cost reconciliation |
| 7. First complete release | All confirmed first-release capabilities, installers, diagnostics, documentation, migration and recovery | Platform matrix, long-running tests, model quality and cost-control assurance, plus newcomer/experienced-user reviews of the understanding and correction journeys below, pass on declared profiles |
| 8. Music intelligence | Predominantly non-English metadata/catalog/fingerprint evaluation, identification quality, optional category classification, weekly sampled-airplay analysis | Regional coverage and false-match results, deduplicated plays, unknown-airtime denominators, stable-panel trends, reproducible rankings, independent paid-service budgets |
| 9. Podcasts and feeds | RSS/Atom subscriptions, finite podcast episodes, bounded refresh/backfill, supplied-transcript evaluation, local batch processing and cross-source topic insights | Entry/media revision and duplicate handling, multilingual evidence quality, XML/download security, fair incremental processing, storage and paid-cost limits |
| 10. Hardware integration | Meshtastic first transport pilots; SDR receive, RF spectrum/waterfall, demodulation, IQ metadata, device scheduling | Actual device acceptance tests on declared OS/device combinations; disconnect and contention recovery |
| 11. Signal workbench | Proposed post-release placement: Morse decode/practice, packet/timing inspection, Enigma and historical transforms with guided experiments | Independent decoder/cipher fixtures, replayable input/intermediate/output traces, explanation of uncertainty, keyboard and reduced-motion review; physical radios are optional for file/demo workflows |
| 12. Modern cryptography | Authenticated encryption and post-quantum operations with supplied keys and extensible profiles | Conformance/interoperability, key lifecycle, nonce and failure tests, dependency review; archive encryption separately decided |
| 13. Additional clients | Evaluate native and/or web interfaces, remote-host operation, optional collaboration | Product demand established and access, media transport, and compatibility designs reviewed |

Stages 3 through 6 are internal engineering milestones. An explorer-only build is not the first complete release. Music, podcast/feed, hardware, Morse, and cryptography planning happens during stage 0. Music, podcast/feed ingestion, and physical radio integration follow the first release. Post-release placement for stages 11 and 12 is a proposal pending scope review. The order of stages 8 through 12 is flexible; file-based workbench features need not wait for hardware delivery.

The globe, day/night map and basic audio/activity visualizers belong to the terminal radio experience. They are not deferred until native/web clients. Programme-guide recording, additional feed directories/namespaces, live-podcast extensions, and distributed collection are separate refinements rather than implied first-release requirements. [Explorer and DVR design](docs/planning/11-radio-explorer-and-dvr.md), [podcast/feed research](research/19-podcasts-and-feeds.md).

## Understanding and correction journeys

These requirements refine the first complete release's radio, recording, translation and monitoring workflows. They use the shared [interpretation contract](docs/design/signal-interpretation.md) and [topic context design](docs/planning/10-analysis-and-knowledge.md#5-durable-topic-context). The same contracts extend to mathematical, symbolic and other non-audio material as its adapters are qualified; this does not move later hardware or workbench milestones into the first release.

- **Trace an answer.** From a finding, reach the supporting translation, original-language transcript, and exact retained recording interval with surrounding context. Show source, time, observed language, location basis, processing method and gaps. Keep measurement, decoding, transcription, translation and interpretation distinct. Label expired evidence and unsupported interpretations explicitly.
- **Correct and revisit.** Correct a name, language span, transcript or interpretation through either client. Preserve the original and prior revisions, expose affected findings, and create revised results through bounded reprocessing. A correction grants no additional source, network, spending or retention authority. If reprocessing cannot run, affected results remain visibly stale or unresolved.
- **Understand coverage and disagreement.** A topic briefing identifies supporting and conflicting reports, repeated or syndicated material, unresolved relationships, and collection/processing gaps. Repetition alone does not establish independent corroboration. Source, speaker and subject locations remain distinct; a map reflects its stated geographic basis.
- **Inspect autonomous decisions.** Show which sources a monitor selected or changed, the recorded basis, applicable policy, execution outcome and resource/cost effects. Explain actual recorded actions without inventing a retrospective rationale. Retained decision history must remain inspectable after reconnecting.
- **Learn through exploration.** A newcomer can use contextual help and replay to explain a displayed signal, translation or finding and identify its limits. Original scripts, regional language distinctions and notation remain available beside translated explanations. Later workbench experiments use the same inspection pattern without requiring specialist pipeline assembly.

Verify these as complete CLI and TUI journeys using multilingual passages, ambiguous input, conflicting and repeated reports, corrections, expired evidence, interrupted processing and exhausted allowances. Inspect usability as well as data integrity. Acceptance is tracked in C-36/C-37 and R-10, R-11, R-36, R-45, R-52 and R-53 in the [requirements](docs/planning/README.md) and [assurance register](docs/planning/04-assurance-and-validation.md).

## Near-future watchlist

Revisit multilingual speech models, regional music coverage, local acceleration backends, TUI library compatibility, provider billing controls, hardware APIs, and cryptographic standards/errata at each relevant selection gate. Prefer replaceable interfaces and measured upgrades over dependencies on announced features or expected model improvements.

Keep installed model identity and processing provenance stable for existing results. New models can improve future runs or create new revisions without rewriting historical evidence.

## Release discipline

Do not reduce the reliability or cost-control requirements to meet an arbitrary date. Do not claim a platform, language pair, stream count, or hardware device is supported before its acceptance evidence exists. Release notes distinguish verified support from experimental adapters.

Plan dependency inventory, shared verification, and hardened CI with the first implementation, then verifiable artifacts for distribution. Review applicable OpenSSF Scorecard findings periodically as the repository matures; assessment is separate from public publication and does not certify product correctness. [Repository engineering plan](docs/planning/12-repository-and-engineering.md).

Detailed gates and open decisions live in [Delivery and decisions](docs/planning/05-delivery-and-decisions.md).
