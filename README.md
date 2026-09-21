# Sigy

**Practical signals intelligence for everyday people.**

A CLI and TUI platform in development for discovering signals, recording them, understanding what they contain, following changes, and experimenting with how they work.

The planned complete CLI supports operation and automation. The optional TUI will add searchable, refreshable station catalogs, a rotatable terminal globe and day/night world map, source/activity visualizers, and DVR-style pause, rewind, scheduled recording, and replay within retained audio.

**Status: early Rust implementation. The durable catalog, exact budget ledger, background controller, and capture job journal pass local Windows tests. Radio acquisition, media storage, TUI, and model processing remain to build.**

Sigy is a working name. Naming research remains open; no replacement has been selected. See [implementation progress](docs/development/progress.md), the [foundation decision](docs/decisions/0001-rust-foundation.md), and the [planning checkpoint](docs/planning/05-delivery-and-decisions.md#8-current-checkpoint-2026-09-20).

The first complete release is intended to include world radio exploration, organized recordings, live translation, and autonomous topic monitoring within user-defined limits. A persistent background service will continue work when the terminal interface closes. Model processing will be local by default, with user-configured LAN and remote providers, including Ollama and OpenRouter. Paid processing requires enforced budgets and transparent accounting.

Multilingual use is fundamental: most expected listening and music are non-English. Sigy will identify languages within each content block, preserve originals, and translate primarily into English. Source contracts include audio, IQ samples, packets, telemetry, text, and other non-audio observations.

Local speech processing, reliable language detection, and durable live/batch queues are priorities for monitoring many streams without metered inference fees. Capture and analysis capacities will be qualified separately by machine and language. Optional classifiers can organize transcripts and identified music; reproducible statistics and evidence-linked findings remain separate stages. Music identification and weekly rankings of the monitored station sample are planned after the first release.

Podcasts and RSS/Atom feeds are on the post-release roadmap for automated insights across audio and published text, reusing local processing, evidence history, resource limits, and optional budgeted providers.

Meshtastic, other LoRa integrations, and software defined radios such as HackRF Pro belong to the hardware roadmap. The broader plan includes Morse decoding and practice, a visual historical cipher workbench with Enigma, and modern authenticated and post-quantum cryptography using supplied keys. Exploration and fun are product goals alongside dependable unattended operation. Native and web interfaces remain future possibilities.

Start with [Intent](INTENT.md), the [Roadmap](ROADMAP.md), [Design documents](docs/README.md), and [Topic research](research/README.md). The [planning index](docs/planning/README.md) distinguishes confirmed requirements, proposed designs, research findings, and unresolved decisions. Research is dated September 20, 2026.

## Try the current foundation

Rust 1.98.1 is pinned by `rust-toolchain.toml`. From the repository root:

```text
cargo run --locked -p sigy -- --help
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json library init
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json library status
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json budget show
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json service start
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json service status
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json service stop
```

Replace `PATH_TO_LIBRARY` with a dedicated private directory outside the checkout. Initialization creates a SQLite catalog with paid processing disabled. The service holds the exclusive library lock; maintenance commands reconnect to its operations while it is running. `service run` keeps the controller in the foreground; `service start` detaches it from the client. This does not install an OS startup service or qualify logout/reboot behavior. Stop the controller before replacing its binary with a different protocol version.

The capture journal stores finite intent, revisions and worker generations and marks abandoned active attempts interrupted on service startup. Job creation is currently an internal storage API; there is no recording command or capture worker yet. Status explicitly reports capture and provider dispatch as unavailable. Amounts in JSON are exact decimal USD strings. Windows x86_64 is locally tested; Linux/macOS native validation and release packaging remain pending. See the [controller](docs/decisions/0002-local-controller.md) and [capture journal](docs/decisions/0003-capture-journal.md) decisions for current boundaries.

Run `./scripts/verify.ps1` in PowerShell for native-source verification, formatting, tests, warnings-denied Clippy, build, and dependency auditing. It requires cargo-audit and fails if a check is unavailable. Builds use two jobs and the script limits test concurrency to two. Command examples in the planning documents remain proposals unless implemented and documented here.

## Lawful use and responsibility

Sigy is intended for lawful listening, research, learning, and analysis of sources you are authorized to access. You are responsible for complying with the laws and permissions applicable to your location, equipment, and use, including rules governing reception, interception, recording, privacy, decryption, radio transmission, copyright, and redistribution. A signal being receivable or a stream being accessible does not by itself establish permission to record, decrypt, publish, or reuse it.

Do not use Sigy for unauthorized access or interception, unlawful decryption or disclosure, harmful interference, or transmission without any required authorization. Respect source and service terms. Hardware integration is initially scoped to reception; modern decryption workflows use supplied keys and supported protocols.

Transcriptions, translations, identifications, and generated findings can be incorrect. Check important results against the original material. This documentation is not legal advice, and this notice does not make an otherwise unlawful activity permissible. It does not add restrictions to the Apache License.

## License and warranty

Copyright 2026 Nick Seal.

Sigy is licensed under the [Apache License, Version 2.0](LICENSE) (`Apache-2.0`). Third-party components and content remain subject to their own licenses and required notices.

Unless required by applicable law or agreed to in writing, Sigy is provided on an "AS IS" basis, without warranties or conditions of any kind. The warranty disclaimer and limitation of liability are set out in Sections 7 and 8 of the [license](LICENSE).
