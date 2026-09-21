# Roadmap

Last updated: 2026-09-20

This roadmap is organized by evidence and exit criteria. It does not assign speculative completion dates. The current phase is documentation only.

| Stage | Scope | Exit criteria |
| --- | --- | --- |
| 0. Intent and research | Intent, multilingual journeys, aggregators, agentic analysis, local/hosted classifiers, typed signals, workbench/cryptography, architecture alternatives, cost policy and open decisions | The complete product is described, consequential unknowns are visible, and the research supports a reviewable design |
| 1. Design baseline | Resolve first-release behavior, representative everyday tasks, guided/advanced interactions, capacity profiles, language quality targets, service modes, retention, and distribution | Reviewed requirements, interface contracts, failure behavior, usability/verification plan, and technology-evaluation criteria |
| 2. Technology evaluation | After the documentation phase: bounded experiments comparing viable language, terminal, media, storage, and inference choices | Measured results on representative machines; a written stack decision with tradeoffs and rejected alternatives |
| 3. Durable foundation | Service lifecycle, job journal, typed artifact/extension contracts, resource admission, media segmentation, recovery, local API, budget ledger | Crash, restart, disk pressure, concurrent reservation, backup, and restore tests pass |
| 4. Radio experience | Refreshable station catalogs, complete CLI and optional TUI, search/filters, globe and day/night map, audio/activity visualizers, DVR buffers, saved intervals, recording schedules and library | CLI/TUI parity, geographic/terminal and audio tests, catalog freshness/identity recovery, DVR expiry/scheduling tests and reliable multi-source capture |
| 5. Language processing | Per-block language spans, mixed/unknown states, local live/batch ASR and fair bounded queues, English translation, provider configuration, local/LAN/remote routing, paid limits | Majority non-English quality/latency results; sustainable local profiles with a zero paid budget; no unapproved remote requests; billing fault tests pass |
| 6. Topic monitoring | Bounded source discovery, scheduling, coverage, evaluated optional classification, deterministic statistics, evidence-linked briefings and topic history | End-to-end topic and missed-event evaluation, revision/evidence preservation, restart recovery, policy enforcement, cost reconciliation |
| 7. First complete release | All confirmed first-release capabilities, installers, diagnostics, documentation, migration and recovery | Platform matrix, long-running tests, model quality review, cost-control assurance, and newcomer/experienced-user journey reviews pass |
| 8. Music intelligence | Predominantly non-English metadata/catalog/fingerprint evaluation, identification quality, optional category classification, weekly sampled-airplay analysis | Regional coverage and false-match results, deduplicated plays, unknown-airtime denominators, stable-panel trends, reproducible rankings, independent paid-service budgets |
| 9. Podcasts and feeds | RSS/Atom subscriptions, finite podcast episodes, bounded refresh/backfill, supplied-transcript evaluation, local batch processing and cross-source topic insights | Entry/media revision and duplicate handling, multilingual evidence quality, XML/download security, fair incremental processing, storage and paid-cost limits |
| 10. Hardware integration | Meshtastic first transport pilots; SDR receive, RF spectrum/waterfall, demodulation, IQ metadata, device scheduling | Actual device acceptance tests on declared OS/device combinations; disconnect and contention recovery |
| 11. Signal workbench | Proposed post-release placement: Morse decode/practice, packet/timing inspection, Enigma and historical transforms | Independent decoder/cipher fixtures, replayable traces, keyboard and reduced-motion review; physical radios are optional for file/demo workflows |
| 12. Modern cryptography | Authenticated encryption and post-quantum operations with supplied keys and extensible profiles | Conformance/interoperability, key lifecycle, nonce and failure tests, dependency review; archive encryption separately decided |
| 13. Additional clients | Evaluate native and/or web interfaces, remote-host operation, optional collaboration | Product demand established and access, media transport, and compatibility designs reviewed |

Stages 3 through 6 are internal engineering milestones. An explorer-only build is not the first complete release. Music, podcast/feed, hardware, Morse, and cryptography planning happens during stage 0. Music, podcast/feed ingestion, and physical radio integration follow the first release. Post-release placement for stages 11 and 12 is a proposal pending scope review. The order of stages 8 through 12 is flexible; file-based workbench features need not wait for hardware delivery.

The globe, day/night map and basic audio/activity visualizers belong to the terminal radio experience. They are not deferred until native/web clients. Programme-guide recording, additional feed directories/namespaces, live-podcast extensions, and distributed collection are separate refinements rather than implied first-release requirements. [Explorer and DVR design](docs/planning/11-radio-explorer-and-dvr.md), [podcast/feed research](research/19-podcasts-and-feeds.md).

## Near-future watchlist

Revisit multilingual speech models, regional music coverage, local acceleration backends, TUI library compatibility, provider billing controls, hardware APIs, and cryptographic standards/errata at each relevant selection gate. Prefer replaceable interfaces and measured upgrades over dependencies on announced features or expected model improvements.

Keep installed model identity and processing provenance stable for existing results. New models can improve future runs or create new revisions without rewriting historical evidence.

## Release discipline

Do not reduce the reliability or cost-control requirements to meet an arbitrary date. Do not claim a platform, language pair, stream count, or hardware device is supported before its acceptance evidence exists. Release notes distinguish verified support from experimental adapters.

Plan dependency inventory, shared verification, and hardened CI with the first implementation, then verifiable artifacts for distribution. Review applicable OpenSSF Scorecard findings periodically as the repository matures; assessment is separate from public publication and does not certify product correctness. [Repository engineering plan](docs/planning/12-repository-and-engineering.md).

Detailed gates and open decisions live in [Delivery and decisions](docs/planning/05-delivery-and-decisions.md).
