# Research index

Research baseline: **September 20, 2026**. Latest topic update: **September 22, 2026**. Individual notes retain their review dates.

This folder records primary-source findings, measured experiments, design implications, competing approaches, and unanswered questions. It supports the [intent](../INTENT.md) and [design documents](../docs/README.md). Each note distinguishes completed measurements from proposed work and technology selection.

| Topic | Research note |
| --- | --- |
| World radio discovery and metadata | [01. Internet radio](01-internet-radio.md) |
| Capture, playback, timing, and formats | [02. Audio and media](02-audio-and-media.md) |
| Speech recognition and translation | [03. Speech and translation](03-speech-and-translation.md) |
| Local, LAN, hosted inference, and billing | [04. Model providers and costs](04-model-providers-and-costs.md) |
| Meshtastic, LoRa, SDR, and IQ | [05. Hardware and signals](05-hardware-and-signals.md) |
| Services, terminal behavior, and distribution | [06. Platforms and TUI](06-platforms-and-tui.md) |
| Rust, Go, native boundaries, and maintenance | [07. Language and stack](07-language-and-stack.md) |
| Catalog, evidence, search, and recovery | [08. Storage and evidence](08-storage-and-evidence.md) |
| Autonomous discovery, topics, and music | [09. Monitoring and music](09-monitoring-and-music.md) |
| Language spans, non-English coverage, scripts, and evaluation | [10. Multilingual processing](10-multilingual-processing.md) |
| Typed signal families, decoder integrations, and Morse | [11. Non-audio and Morse](11-non-audio-and-morse.md) |
| Historical workbench, authenticated encryption, and post-quantum operations | [12. Cryptography](12-cryptography.md) |
| Input boundaries, privacy, updates, and maintenance | [13. Security and release engineering](13-security-and-release-engineering.md) |
| Media/signal aggregators and evolving topic context | [14. Aggregation and context](14-aggregation-and-context.md) |
| Bounded agents, evidence, interoperability, and evaluation | [15. Agentic analysis](15-agentic-analysis.md) |
| Jev, OpenRouter System One, and local classification alternatives | [16. Decision models and classifiers](16-decision-models-and-classifiers.md) |
| Local speech candidates, language routing, queues, and multi-stream capacity | [17. Local processing and capacity](17-local-processing-and-capacity.md) |
| Rotatable globe, day/night map, live station catalogs, visualizers, and DVR | [18. Terminal explorer and radio DVR](18-terminal-explorer-and-radio-dvr.md) |
| Finite media, syndicated text, feed updates, and mixed-source insights | [19. Podcasts and feeds](19-podcasts-and-feeds.md) |
| Workspace ownership, persistent instructions, dependency discipline, and Scorecard | [20. Repository engineering](20-repository-engineering.md) |
| Understandable naming, product associations, and preliminary collision screening | [21. Product naming](21-naming.md) |
| Current terminal libraries, interaction, accessibility and performance | [22. Modern terminal UX](22-modern-terminal-ux.md) |
| Interface localization and substantive language coverage | [23. Localization and language coverage](23-localization-and-language-coverage.md) |
| Language, mathematical and unfamiliar signal interpretation | [24. Universal interpretation](24-universal-interpretation.md) |
| Source authority, HTTP/TLS dependencies, destination policy and finite transfer | [25. HTTP acquisition](25-http-acquisition.md) |
| Recording validation, rolling retention, catalog/feed evidence and RF metadata | [26. Recording, discovery and RF](26-recording-discovery-and-rf.md) |
| Radio Browser mirrors, bounded cache refresh, offline search and provenance | [27. Radio directory integration](27-radio-directory.md) |
| Explicit redirect grants, checked hops and recording provenance | [28. HTTP redirects](28-http-redirects.md) |
| User-controlled proxies, DNS trust, external VPNs and route evidence | [29. Network routing](29-network-routing.md) |
| Native ASR/translation candidates, iGPU comparison, automated quality evidence, and bounded remote validation | [30. Retained-recording language evaluation](30-language-pipeline-evaluation.md) |

## Evidence discipline

- Prefer official specifications, API documentation, and upstream project documentation.
- Distinguish a documented capability from demonstrated Sigy compatibility.
- Label architecture recommendations as inferences and measurements as measurements.
- Do not convert upstream benchmark claims into product performance promises.
- Record material limitations and inaccessible evidence rather than filling gaps with assumptions.
- Recheck versions, provider semantics, pricing, and support matrices before selection and release.
- Treat near-future improvements as things to monitor, not prerequisites for the design to work.

## Research performed in this phase

The initial documentation-only phase reviewed official material including terminal drawing, map data, solar calculations, directory refresh, RSS/Atom, and podcast delivery. A public Radio Browser server-list endpoint was retrieved successfully. That checkpoint included no code or runtime evaluation. Subsequent [foundation experiments](experiments/foundation/README.md) and [implementation evidence](../docs/development/progress.md) include local decoder-backed recording, retention and process-kill tests. Live transcription, public-stream interoperability, paid requests and hardware tests remain unqualified.

Several documentation URLs required an upstream repository or canonical redirected page as a fallback. Links in the notes point to the evidence actually used where available. Rolling documentation can change after this date; the technology-selection gate must retain exact versions or immutable snapshots for consequential decisions.

Additional research reviewed current speech/decision model cards, aggregator and receiver projects, agentic tool interfaces, and the published MCP revision. OpenRouter's Jev model/provider metadata and official System One integration documentation were retrieved read-only. No inference endpoint was invoked. Documented capabilities and illustrative capacity/cost arithmetic remain distinct from measured compatibility or performance.

Repository-engineering research covers workspace boundaries, instruction discovery, dependency review, and OpenSSF Scorecard practices. Naming research records candidate collisions and search limitations. Neither research area establishes an assessed security score or a cleared product name; selected technology and its qualification limits live in [architecture decisions](../docs/decisions/0001-rust-foundation.md).

## Near-future review cadence

Review fast-moving provider interfaces and model capabilities at each integration decision and before release. Review prices at runtime according to the cost policy. Revisit terminal, driver, and OS compatibility when supported versions change. Retest model upgrades on the same evaluation corpus before changing defaults.
