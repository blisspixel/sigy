# Sigy

**Practical signals intelligence for everyday people.**

A planned CLI and TUI platform that makes it easy and enjoyable to discover signals, record them, understand what they contain, follow what changes, and experiment with how they work.

The CLI is a complete interface for operation and automation. The optional TUI adds searchable, refreshable station catalogs, a rotatable terminal globe and day/night world map, source/activity visualizers, and DVR-style pause, rewind, scheduled recording, and replay within retained audio.

**Status: research and design. No implementation language or stack has been selected. Python is excluded.**

Sigy is a working name. Naming research remains open; no replacement has been selected. The [current checkpoint and next steps](docs/planning/05-delivery-and-decisions.md#8-current-checkpoint-2026-09-20) provide a starting point for continuing the planning work.

The first complete release is intended to include world radio exploration, organized recordings, live translation, and autonomous topic monitoring within user-defined limits. A persistent background service will continue work when the terminal interface closes. Model processing will be local by default, with user-configured LAN and remote providers, including Ollama and OpenRouter. Paid processing requires enforced budgets and transparent accounting.

Multilingual use is fundamental: most expected listening and music are non-English. Sigy will identify languages within each content block, preserve originals, and translate primarily into English. Source contracts include audio, IQ samples, packets, telemetry, text, and other non-audio observations.

Local speech processing, reliable language detection, and durable live/batch queues are priorities for monitoring many streams without metered inference fees. Capture and analysis capacities will be qualified separately by machine and language. Optional classifiers can organize transcripts and identified music; reproducible statistics and evidence-linked findings remain separate stages. Music identification and weekly rankings of the monitored station sample are planned after the first release.

Podcasts and RSS/Atom feeds are on the post-release roadmap for automated insights across audio and published text, reusing local processing, evidence history, resource limits, and optional budgeted providers.

Meshtastic, other LoRa integrations, and software defined radios such as HackRF Pro belong to the hardware roadmap. The broader plan includes Morse decoding and practice, a visual historical cipher workbench with Enigma, and modern authenticated and post-quantum cryptography using supplied keys. Exploration and fun are product goals alongside dependable unattended operation. Native and web interfaces remain future possibilities.

Start with [Intent](INTENT.md), the [Roadmap](ROADMAP.md), [Design documents](docs/README.md), and [Topic research](research/README.md). The [planning index](docs/planning/README.md) distinguishes confirmed requirements, proposed designs, research findings, and unresolved decisions. Research is dated September 20, 2026.

This repository currently describes the intended product. Command examples in the planning documents are interface proposals, not available commands.

## Lawful use and responsibility

Sigy is intended for lawful listening, research, learning, and analysis of sources you are authorized to access. You are responsible for complying with the laws and permissions applicable to your location, equipment, and use, including rules governing reception, interception, recording, privacy, decryption, radio transmission, copyright, and redistribution. A signal being receivable or a stream being accessible does not by itself establish permission to record, decrypt, publish, or reuse it.

Do not use Sigy for unauthorized access or interception, unlawful decryption or disclosure, harmful interference, or transmission without any required authorization. Respect source and service terms. Hardware integration is initially scoped to reception; modern decryption workflows use supplied keys and supported protocols.

Transcriptions, translations, identifications, and generated findings can be incorrect. Check important results against the original material. This documentation is not legal advice, and this notice does not make an otherwise unlawful activity permissible. It does not add restrictions to the Apache License.

## License and warranty

Copyright 2026 Nick Seal.

Sigy is licensed under the [Apache License, Version 2.0](LICENSE) (`Apache-2.0`). Third-party components and content remain subject to their own licenses and required notices.

Unless required by applicable law or agreed to in writing, Sigy is provided on an "AS IS" basis, without warranties or conditions of any kind. The warranty disclaimer and limitation of liability are set out in Sections 7 and 8 of the [license](LICENSE).
