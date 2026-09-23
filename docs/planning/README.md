# Sigy research and design

Last updated: 2026-09-22

Status: research-backed product contract. Implementation has begun with a [Rust foundation decision](../decisions/0001-rust-foundation.md); [active work](../development/progress.md) records verified behavior and remaining qualification.

## Read in this order

| Document | Purpose |
| --- | --- |
| [Product and experience](01-product-and-experience.md) | Product boundaries, user journeys, CLI contract, TUI behavior, and first-release scope |
| [Architecture and data](02-architecture-and-data.md) | Process boundaries, capture and analysis flows, persistence, recovery, and extension contracts |
| [Source and model research](03-source-and-model-research.md) | Primary-source evidence, implications, candidate integrations, and uncertainties |
| [Assurance and validation](04-assurance-and-validation.md) | Requirements traceability, failure analysis, quality targets, and verification evidence |
| [Delivery and decisions](05-delivery-and-decisions.md) | Research work packages, decision gates, release criteria, and open questions |
| [Language and stack trade study](06-language-and-stack-trade-study.md) | Rust and Go comparison, integration choices, and evidence needed before selection |
| [Providers and cost policy](07-providers-and-cost-policy.md) | Local, LAN, and hosted processing; spending authorization, reservations, and reconciliation |
| [Signal extensions and workbench](08-signal-extensions-and-workbench.md) | Typed non-audio signals, multilingual blocks, adapter contracts, Morse, historical ciphers, and modern cryptography |
| [Security, privacy, and release design](09-security-privacy-and-release.md) | Network/input boundaries, data destinations, keys, packaging, updates, and maintenance |
| [Analysis, knowledge, and multi-stream processing](10-analysis-and-knowledge.md) | Local queues, classifier contracts, persistent topic context, and reproducible weekly statistics |
| [Complete CLI, terminal explorer, and radio DVR](11-radio-explorer-and-dvr.md) | Full CLI operation, geographic TUI, station freshness, visualizers, rolling playback, and recording schedules |
| [Repository organization and engineering](12-repository-and-engineering.md) | Proposed package ownership, minimal dependencies, verification, persistent instructions, and later Scorecard assessment |

## Confirmed requirements

These come directly from the product discussion. Changes require revisiting the product decision, rather than silently changing scope.

| ID | Requirement |
| --- | --- |
| C-01 | CLI and TUI are the initial interfaces, with a high-quality experience on Linux, macOS, and Windows. |
| C-02 | The first complete release includes radio exploration, recordings, live translation, and topic monitoring. |
| C-03 | A persistent background service keeps recording and monitoring when the CLI or TUI closes. Clients reconnect to running work. |
| C-04 | Processing defaults to local models. The user may configure remote providers. Ollama is an explicit integration target. |
| C-05 | Multiple simultaneous recording streams and organized analysis of public sources are central capabilities. |
| C-06 | Meshtastic/LoRa and SDR hardware, including HackRF Pro, are planned for later physical testing. |
| C-07 | Topic-oriented requests should drive discovery, collection, and insight generation. Regional music discovery is a desired use case. |
| C-08 | Future native or web clients should remain possible. Their implementation is undecided. |
| C-09 | Deep research and full planning precede language and stack selection. Python is excluded. |
| C-10 | Reliability, correctness, maintainability, and careful engineering take priority over quick implementation. |
| C-11 | Monitors may discover and adjust stations automatically within user-defined source, time, and resource limits. |
| C-12 | Music identification and sampled airplay rankings are planned now and implemented after the first release. |
| C-13 | Both desktops and small always-on machines require tested capability profiles. |
| C-14 | User-configured network providers, including OpenRouter, are supported design targets alongside local processing. |
| C-15 | Paid functionality requires excellent enforced cost controls and transparent accounting, with no surprise spend. |
| C-16 | The completed documentation baseline produced intent, README, roadmap, detailed designs, and dated topic research before implementation began; subsequent work updates these alongside measured evidence. |
| C-17 | Most expected listening is non-English. Identify language or languages within content blocks, preserve originals, and translate primarily into English. |
| C-18 | Music research and identification must represent predominantly non-English material and regional catalog coverage. |
| C-19 | Radio/source and processing integrations must be extensible, including sources whose observations are not audio. |
| C-20 | Morse code support is a planned capability. Its release placement remains open. |
| C-21 | An extensible offline historical cipher laboratory includes Enigma, engaging TUI traces, self-generated encrypted messages, supplied-settings decryption, bounded classical unknown-key challenges, and reveal/compare/replay. Hidden answers remain outside solver inputs; originals, settings, cribs, search coverage, and uncertainty are preserved. |
| C-22 | Modern authenticated encryption and post-quantum operations using supplied keys remain separate from classical cryptanalysis. Planned offline lessons use maintained ML-KEM key-establishment plus AEAD profiles and ML-DSA/SLH-DSA signatures, with independent vectors and no quantum-breaking claim. |
| C-23 | Exploration, learning, and fun are explicit product goals; guided synthetic exercises make mechanisms and limits inspectable even without an operational purpose. Cipher and post-quantum lessons remain future roadmap stages 11 and 12. Setup asks for a local, editable country/state selection with US-default guidance and no GPS/IP inference; guidance and educational notices grant no interception, decryption, or transmission authority. |
| C-24 | Sigy is practical signals intelligence for everyday people: broad everyday workflows must be fun and approachable to non-specialists while retaining advanced control. The informal "90%" ambition describes desired breadth, not verified coverage. |
| C-25 | The project is licensed under Apache License 2.0. Preserve required third-party legal notices and review dependency/distribution compatibility. |
| C-26 | Sigy is intended for lawful use. The README includes clear legal-responsibility, warranty, and liability notices without implying that a disclaimer authorizes otherwise unlawful activity. |
| C-27 | Multi-stream local language detection and speech processing with no metered inference fees are priorities. Support live and batch queues, independently qualify capture/analysis capacity, and distinguish broad language ambition from tested support. |
| C-28 | Research optional decision models such as Jev through OpenRouter and open-source local classifiers for transcript/music analysis. Consider their value after transcription or identification, within explicit budgets; this does not select a provider or make paid classification required. |
| C-29 | The CLI must work fully on its own; the TUI is an optional interface over the same operations. |
| C-30 | Station discovery needs usable search and filters in the TUI and CLI, with refreshed channel/catalog information and visible freshness. |
| C-31 | Provide DVR-style recording and retention. Default media retention is 14 days or capacity pressure within 50 GB; keep/archive protection prevents automatic deletion and protected bytes still count toward quota. Detailed buffer/schedule behavior remains to qualify. |
| C-32 | Geographic exploration belongs in the TUI, including a rotatable globe, day/night world view, and visualizations of sources and processing activity. Rendering details and capability tiers require evaluation. |
| C-33 | Prioritize internet radio and podcasts before physical radio integration. Support feed and episode identity, subscriptions and supplied resources through shared evidence/processing controls; syndicated text analysis follows. |
| C-34 | Keep dependencies minimal and intentional while preserving mature implementations for complex or security-sensitive capabilities. Repository organization must support extension without duplicated infrastructure. |
| C-35 | Plan periodic open-source engineering assessments as the project matures, aiming for strong applicable results. OpenSSF Scorecard is the current interpretation of the requested score; no assessment or numeric target is established. |
| C-36 | Understanding signals in context is central. Preserve an inspectable path from original observations through decoding, transcription, translation and interpretation, including uncertainty and alternatives. Shared contracts extend across languages, mathematical/symbolic material and non-audio sources. |
| C-37 | Users can trace findings to retained evidence, correct scoped observations or interpretations, inspect affected results and revision history, and understand monitor decisions and coverage. Exploration should teach how results were obtained and what remains unresolved. |
| C-38 | Recording metadata and sidecars use a consistent versioned envelope with extensible typed profiles for internet radio, podcasts, SDR/IQ, LoRa, Meshtastic and other signals. Preserve frequency/time context where known without assuming every observation is audio. |
| C-39 | Later receive-only RF exploration includes supported bands such as CB, tuning/scanning and signal-finding tools. Device capabilities, measured coverage and regional rules govern available profiles. |
| C-40 | Broadcast analysis detects language shifts, candidate advertisements and new-song boundaries within one station. Preserve mixed/overlapping content and uncertainty; song boundaries do not require successful title identification. |
| C-41 | Support personal-machine and server operation, with optional user-controlled proxy/VPN routing to improve access to sources across networks. Keep routing a small capability supporting a free and open internet; exact profiles, remote control and release placement require qualification. |

## Reading the status labels

- **Confirmed:** explicitly established by the user.
- **Proposed:** an engineering recommendation for review, not an approved decision.
- **Researched:** supported by the linked primary source as reviewed on the date above.
- **Unvalidated:** requires an experiment, benchmark, hardware session, or user decision.

All detailed architecture, command names, UI layouts, numerical performance targets, and release sequencing below are proposed unless identified as confirmed. No technology mentioned in the research is selected merely because it appears here.

## What this package establishes

The documents connect the requested experience to system responsibilities, identify material failure modes, and define evidence needed before choosing implementation technologies. They preserve the full first-release scope while allowing internal milestones to verify individual layers.

They do not establish measured performance, completed hardware compatibility, model accuracy, or an implementation-ready baseline. Those require the decision and validation gates in [Delivery and decisions](05-delivery-and-decisions.md). The engineering goal is a durable, exceptionally reliable product, with explicit invariants and reproducible failure and recovery tests.

## Outstanding product decisions

Outstanding questions cover concrete capacity targets within the confirmed machine profiles, caption versus spoken translation, service behavior across logout and reboot, detailed buffer/derivative retention, distribution, qualified language coverage, and workbench/cryptography sequencing. Recording defaults are confirmed at 14 days and 50 GB with Keep/Archive protection. Remaining decisions are centralized in the [decision register](05-delivery-and-decisions.md#3-decision-register).
