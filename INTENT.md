# Intent

Last updated: 2026-09-20

Sigy's purpose is practical signals intelligence for everyday people: a broad, approachable, enjoyable toolkit for discovering signals and turning observations into understanding.

It should be an exceptionally well-engineered application that feels natural in a terminal and remains dependable during long unattended runs. A newcomer should be able to achieve useful results while learning; experienced users should be able to inspect and control the details.

It should make it enjoyable to explore world radio, straightforward to record several sources, and possible to understand broadcasts through live transcription, translation, and topic monitoring. A user should be able to ask to follow a subject, set boundaries, and return to useful findings with inspectable source evidence.

Curiosity and play are part of the product. Discovering unfamiliar music, inspecting a signal, practicing Morse, or stepping through an Enigma machine should be satisfying in its own right. A historical cipher feature does not need an operational justification to belong here.

## Breadth and approachability

The ambition is to cover most everyday signal workflows in one coherent application: discover, listen or inspect, capture, decode, translate, search, compare, monitor, and explain findings. The informal "90%" aspiration expresses that breadth. Coverage will be judged against a documented set of representative tasks, source types, and supported profiles before any numerical claim is made.

Everyday tasks should begin with a clear action or question, such as "understand this station," "follow this topic," or "show me how this code works." Useful presets, plain-language status, contextual explanations, and reversible exploration help users progress. Detailed signal controls and reproducible automation remain available as users need them.

Ease of use is part of engineering quality. Complexity belongs in well-designed internals and optional detail views; users should not need to assemble a decoder pipeline or understand model infrastructure to complete a supported common task.

## Product commitments

- CLI and TUI first, on Linux, macOS, and Windows.
- The CLI is complete without opening the optional TUI. Both expose the same source, recording, processing, monitoring, and administrative operations.
- The TUI includes searchable and refreshable station catalogs, a rotatable globe/world map with day/night context, and truthful source/activity visualizers.
- DVR-style radio workflows include bounded pause/rewind, return to live, saved intervals, scheduled recordings, and replay of retained material.
- Broad practical signal workflows must be approachable to non-specialists, with guided starting points and progressively available advanced controls.
- A persistent service owns recording and monitoring independently of the interface.
- World radio exploration, recordings, live translation, and topic monitoring belong in the first complete release.
- Most expected listening is non-English. Identify language or languages within each content block, including mixed and uncertain results, preserve originals, and translate primarily into English.
- Monitors may discover and adjust sources within the user's source, time, and resource limits.
- Local processing is the default. Local runtimes, user-managed network services, and explicitly configured remote providers are supported design targets. Ollama and OpenRouter are named integrations.
- Paid processing requires excellent enforced cost controls. No silently enabled paid fallback, automatic budget increase, or surprise recurring spend.
- Both desktops and small always-on machines need tested capability profiles.
- Useful multi-stream local language detection and speech processing must remain available with a paid API budget of zero, with separate live and batch queues and honestly qualified language coverage.
- Music identification and sampled airplay rankings are designed now and built after the first release.
- Podcasts and RSS/Atom feeds belong on the later roadmap for multilingual automated analysis and insights across source types, within the same resource and spending limits.
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

Language choice contributes to this standard but cannot establish it alone. Rust and Go are research candidates. Python is excluded. No language, framework, database, model runtime, or media backend is selected yet.

## Current phase

Research and plan the entire product before writing implementation code. Establish intent, workflows, architecture, risks, operational rules, and verification criteria first. Use primary-source research dated September 20, 2026, and identify what needs rechecking as the ecosystem changes.

The current deliverables are [documentation](docs/README.md), [topic research](research/README.md), and a [roadmap](ROADMAP.md). Benchmarks and prototypes are future work after the documentation phase is reviewed. No implementation, dependency installation, model download, or paid inference is part of this phase.

## What success looks like

A user installs Sigy, finds a station, listens, starts several recordings, enables translation, and creates a bounded topic monitor. They can close the terminal, reconnect later, inspect failures and spending, and follow a report back to the exact retained material that supports it.

The same product remains manageable on a small always-on host, benefits from a capable desktop, and can accept hardware sources later without rebuilding its storage, job lifecycle, or evidence model.

A user can also open a replayable experiment, inspect Morse timing or an Enigma rotor path, and understand the result without a model account or paid request. Exact release placement for the workbench and cryptographic capabilities remains a planning decision.
