# Intent

Last updated: 2026-10-03

Sigy's purpose is practical signals intelligence for everyday people: a broad, approachable, enjoyable toolkit for discovering signals and turning observations into understanding.

The central journey is to explore the world's signals and discover what they mean. Understanding includes the original observation, its context, the steps used to interpret it, and what remains unresolved. This applies across languages and representations, including mathematical and symbolic material, through the shared [interpretation contract](docs/design/signal-interpretation.md).

It should be an exceptionally well-engineered application that feels natural in a terminal and remains dependable during long unattended runs. A newcomer should be able to achieve useful results while learning; experienced users should be able to inspect and control the details.

The intended character is a professional signals instrument accessible to everyday people. Its usefulness comes from actual collection, measured observations, reproducible processing, inspectable evidence and dependable operation. Professional quality is an engineering and workflow goal to demonstrate through evidence. Present the product as a listening, analysis and operations desk; personal journal framing is not desired. Learning and enjoyment come from discovering real material, understanding mechanisms and accomplishing useful tasks.

This is a passion project with the breadth of a serious signals workstation and the freedom to make exploration unusually rewarding. The core must cover discovery and acquisition, retained observations, decoding and language work, time/place context, search and analysis, monitoring, cited results and dependable recovery within declared supported profiles. The wider workbench adds expressive receivers, world listening, unfamiliar representations, historical ciphers, experiments and creative explanation. Modern interaction, visual ambition and play belong in the delivered experience alongside correctness, evidence and resource control.

Aim for the instrument a deeply experienced practitioner has always wanted and looks forward to opening after work. Preserve context across exploration, keep expert depth close at hand, and make navigation and reversible investigation fluid enough to support absorbed, self-directed work. A newcomer gets a clear way in; a specialist gets a rich instrument to grow into. Compact access preserves usability, while expansive terminals support composed multi-pane views and expressive real activity.

A user should be able to state a task and have a bounded agent workflow use the toolkit to complete it well. Reusable skills, the existing plugin and MCP surface, and agent-to-agent interoperability through A2A belong to this direction. Humans get an immersive, calm instrument they can enjoy; agent harnesses get discoverable capabilities, structured results, exact evidence and durable task state. Both use the same service, permission, budget, revision and recovery contracts. CLI/plugin/MCP composition is part of product quality, with useful partial results and explicit unsupported capabilities. A qualified local-model path with zero metered inference fees is required; OpenRouter and other explicitly configured providers are optional. The service owns durable progress, policy and effects across client exit and restart. [Durable task workflows](docs/design/task-workflows.md) records the current gap and proposed contract.

Support a free and open internet through user choice: open discovery, portable data and optional user-controlled network routes. The same service should run on a personal machine or a server. Proxy configuration and compatibility with externally managed VPNs should remain small, optional capabilities; remote service access is a separate security boundary.

Storage and analysis should grow from a Pi-class always-on host or gaming machine to explicitly configured 2 TB or 20 TB bulk storage and reproducible larger deployments. Qualify capture, catalog/search, inference and storage independently. Keep local transactional authority and bounded spool/scratch separate from future NAS media; more available space cannot silently enlarge authority or consume the whole device. The [storage and memory contract](docs/design/storage-and-memory.md) records the proposed path and current co-location limit.

Build durable knowledge from exact evidence: searchable originals, attributed claims, evolving topic relationships, time-aware history and portable linked Markdown views. Research Open Knowledge Format interchange and graph/wiki memory as complementary representations. Task context, user preferences, authorized procedures and generated interpretations have distinct scope and retention. A readable wiki or fluent summary cannot become another source of truth or edit service policy. Multiple physical memory systems are an option to earn through measurements, not a requirement to install competing databases.

It should make it enjoyable to explore world radio, straightforward to record several sources, and possible to understand broadcasts through live transcription, translation, and topic monitoring. A user should be able to ask to follow a subject, set boundaries, and return to useful findings with inspectable source evidence.

Curiosity and play are part of the product. Discovering unfamiliar music, inspecting a signal, practicing Morse, or stepping through an Enigma machine should be satisfying in its own right. A historical cipher feature does not need an operational justification to belong here.

Receiver personalities such as a CB channel scanner, AM/FM dial and shortwave or spectrum desk should make unfamiliar signals approachable. They share source, capture and interpretation contracts rather than becoming separate applications. Temporary capture for local analysis is a normal workflow; keeping a permanent recording is optional.

Broadcasts change within a session. Detect language shifts, candidate advertisements and new-song boundaries on their own timelines, preserving overlaps and uncertainty. A song can start before its identity is known. Directory metadata never substitutes for listening to the content. See the [broadcast analysis contract](docs/design/broadcast-analysis.md).

## Breadth and approachability

The ambition is to cover most everyday signal workflows in one coherent application: discover, listen or inspect, capture, decode, translate, search, compare, monitor, and explain findings. The informal "90%" aspiration expresses that breadth. Coverage will be judged against a documented set of representative tasks, source types, and supported profiles before any numerical claim is made.

Everyday tasks should begin with a clear action or question, such as "understand this station," "follow this topic," or "show me how this code works." Useful presets, plain-language status, contextual explanations, and reversible exploration help users progress. Detailed signal controls and reproducible automation remain available as users need them.

Ease of use is part of engineering quality. Complexity belongs in well-designed internals and optional detail views; users should not need to assemble a decoder pipeline or understand model infrastructure to complete a supported common task.

Status reports and optional exploration achievements are possibilities raised on 2026-09-30, not selected features. Reports could summarize actual source activity, collection and processing coverage, findings, gaps, failures and resource use. Achievements could encourage completing useful tasks across supported capabilities, with an inspectable basis. Their design must preserve the distinction between completing a task, learning a mechanism and qualifying an analysis result. Detailed proposals belong in [Product and experience](docs/planning/01-product-and-experience.md#professional-utility-and-exploration).

The TUI must be visually polished, modern and enjoyable from its first usable increment. Expressive receiver views, purposeful motion and satisfying exploration belong alongside reliability and clear information. The [terminal experience contract](docs/design/terminal-experience.md) defines the visual direction, interaction standards and rendered-review requirements.

Users must be able to follow findings back to retained originals, inspect alternative interpretations, and correct an observation or interpretation without erasing its history. Corrections expose affected results and can trigger bounded reprocessing under the existing policy. Exploration should help users learn what a signal contains, how an answer was obtained, and where the evidence stops.

## Product commitments

- CLI and TUI first, on Linux, macOS, and Windows.
- The CLI is complete without opening the optional TUI. Both expose the same source, recording, processing, monitoring, and administrative operations.
- The TUI includes searchable and refreshable station catalogs, a rotatable globe/world map with day/night context, and truthful source/activity visualizers. Type or select countries/territories and a declared worldwide major-city reference independently of cached station availability; combine filters and preserve unfamiliar scripts.
- DVR-style radio workflows include bounded pause/rewind, return to live, saved intervals, scheduled recordings, and replay of retained material.
- Broad practical signal workflows must be approachable to non-specialists, with guided starting points and progressively available advanced controls.
- A persistent service owns recording and monitoring independently of the interface.
- World radio exploration, recordings, live translation, and topic monitoring belong in the first complete release.
- Most expected listening is non-English. Identify language or languages within each content block, including mixed and uncertain results, and preserve originals. English is the default translation and initial evaluation target; humans and agents can choose other qualified targets. Translation direction, source variety and target variety require separate evidence.
- Monitors may discover and adjust sources within the user's source, time, and resource limits.
- Local processing is the default. Local runtimes, user-managed network services, and explicitly configured remote providers are supported design targets. Ollama and OpenRouter are named integrations.
- Paid processing requires excellent enforced cost controls. No silently enabled paid fallback, automatic budget increase, or surprise recurring spend.
- Both desktops and small always-on machines need tested capability profiles.
- Useful multi-stream local language detection and speech processing must remain available with a paid API budget of zero, with separate live and batch queues and honestly qualified language coverage.
- Music identification and sampled airplay rankings are designed now and built after the first release.
- Internet radio and podcasts are prioritized before hardware. Podcast subscriptions, publisher transcripts and chapters share the same evidence, multilingual processing and storage controls; RSS/Atom text analysis extends that path.
- Music research and qualification must represent predominantly non-English listening, regional catalogs, and original scripts.
- Research optional hosted and local classifiers for organizing transcripts and identified music. Model judgments remain distinct from deterministic statistics and source evidence; no classifier is selected by this research request.
- Meshtastic, other LoRa protocols, and SDR hardware such as HackRF Pro are future source integrations, tested with actual devices when available.
- Source and transform contracts must extend to new inputs and non-audio observations such as IQ, packets, telemetry, and timed symbols.
- Morse support and an extensible historical cipher workbench, including a visual Enigma experience, are planned product capabilities.
- Modern authenticated encryption and post-quantum cryptography using supplied keys are also planned, with separate operational key handling from historical demonstrations.
- Future native and web clients remain possible through stable application interfaces.
- Apache License 2.0 is the project license. Preserve required third-party licenses and notices.
- The product is intended for lawful use with appropriate source/device permissions. The README must state user responsibilities and the applicable warranty/liability terms without claiming that a disclaimer establishes legal authorization.

## Engineering standard

Optimize for long-term correctness, performance, maintainability, portability, and operational clarity. Additional implementation effort is justified when it produces a demonstrably better product.

Keep dependencies minimal and intentional across the complete distribution, including native engines and model assets. Prefer maintained implementations for security-sensitive and specialized functionality. Periodic OpenSSF Scorecard reviews should strengthen applicable supply-chain practices as the project matures; a score does not establish product correctness.

Reliability means preserving captured data, reporting gaps, recovering predictably, keeping resource use bounded, and letting a user understand what the system did. Analysis quality means separating original observations from interpretations and making findings checkable.

Privacy and local security are product requirements. Product diagnostics stay private, local, minimal and bounded; there is no automatic analytics, crash upload or diagnostic phone-home. Explicit source acquisition and configured provider processing retain their separate destination and spending policies. Necessary task and evidence state is protected application data, not duplicated into ordinary logs. Secrets and unnecessary payloads are excluded from diagnostics, and any support export is inspectable before explicit sharing.

Aim for software that remains understandable, recoverable and useful over a long service life: defensive contracts, resumable tasks, corruption detection, verified restores, controlled migrations, portable artifacts, intentional dependencies and maintained recovery instructions. Demonstrate these properties through fault injection, replay, long-running and real-platform evidence. Institutional labels, a passing test suite or an imagined service-life number cannot establish that reliability.

Language choice contributes to this standard but cannot establish it alone. Rust is selected for the foundation after initial Rust/Go evaluation; Python is excluded. SQLite is the initial catalog. Model, media, and terminal choices retain their own evidence gates. See the [foundation decision](docs/decisions/0001-rust-foundation.md).

## Current phase

The initial documentation and research baseline preceded implementation. The user advanced the project to building on September 20, 2026. Continue researching consequential choices, preserving the product contracts, and verifying each implementation increment before expanding it.

Current deliverables include source, tests, [documentation](docs/README.md), [topic research](research/README.md), and the [roadmap](ROADMAP.md). [Implementation progress](docs/development/progress.md) records evidence, open work, and the active development spending ceiling. Research conclusions do not establish measured language, hardware, or platform support.

## What success looks like

A user installs Sigy, finds a station, listens, starts several recordings, enables translation, and creates a bounded topic monitor. They can close the terminal, reconnect later, inspect failures and spending, and follow a report back to the exact retained material that supports it.

The same product remains manageable on a small always-on host, benefits from a capable desktop, and can accept hardware sources later without rebuilding its storage, job lifecycle, or evidence model. The 2026-10-02 clarification requires architecture that can grow to large deployments through measured resource admission, stable evidence identities and explicit ownership. It establishes no current capacity claim or requirement for a distributed installation.

The terminal explorer should use the available screen deliberately and feel like an expressive, modern instrument with a retro-futuristic character inspired by late-1980s computing, WarGames and hands-on radio desks. Its sense of power comes from useful worldwide discovery, simultaneous source work, precise DVR control and insights traceable to originals. Country and major-city selection remains available even when the station cache is empty; coverage limits and useful fallbacks stay visible. Dependable radio lists, bounded validation/recovery, linked search/filter/map views and exceptional keyboard interaction belong to the intended CLI/TUI experience. Later native applications remain possible; the current product does not require a browser. [Terminal experience](docs/design/terminal-experience.md) defines the visual direction and DVR work without weakening source or resource authority.

Unfamiliar, constructed or hypothetical languages and symbol systems remain eligible for investigation. Preserve observations, candidate representations, competing interpretations and supplied context without inventing origin or meaning. Physical instrumentation, analytical research and artistic attention inform a coherent instrument whose usefulness and beauty come from actual evidence. The [reliability and scale plan](docs/development/reliability-and-scale.md) makes the next increments reviewable.

A user can also open a replayable experiment, inspect Morse timing or an Enigma rotor path, and understand the result without a model account or paid request. Exact release placement for the workbench and cryptographic capabilities remains a planning decision.
