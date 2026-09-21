# Sigy

**Practical signals intelligence for everyday people.**

A CLI and TUI platform in development for discovering signals, recording them, understanding what they contain, following changes, and experimenting with how they work.

The planned complete CLI supports operation and automation. The optional TUI will add searchable, refreshable station catalogs, a rotatable terminal globe and day/night world map, source/activity visualizers, and DVR-style pause, rewind, scheduled recording, and replay within retained audio.

**Status: early Rust implementation. Radio Browser refresh and cached search, finite service-owned audio recording, decoder validation, metadata export and rolling storage policy work through the CLI. Local Windows tests and a live directory check support this increment. Podcast subscriptions, integrated playback, TUI and model processing remain to build.**

Sigy is a working name. Naming research remains open; no replacement has been selected. See [implementation progress](docs/development/progress.md), the [foundation decision](docs/decisions/0001-rust-foundation.md), and the [planning checkpoint](docs/planning/05-delivery-and-decisions.md#8-current-checkpoint-2026-09-20).

The first complete release is intended to include world radio exploration, organized recordings, live translation, and autonomous topic monitoring within user-defined limits. A persistent background service will continue work when the terminal interface closes. Model processing will be local by default, with user-configured LAN and remote providers, including Ollama and OpenRouter. Paid processing requires enforced budgets and transparent accounting.

Multilingual use is fundamental: most expected listening and music are non-English. Sigy will identify languages within each content block, preserve originals, and translate primarily into English. Source contracts include audio, IQ samples, packets, telemetry, text, and other non-audio observations.

Local speech processing, reliable language detection, and durable live/batch queues are priorities for monitoring many streams without metered inference fees. Capture and analysis capacities will be qualified separately by machine and language. Optional classifiers can organize transcripts and identified music; reproducible statistics and evidence-linked findings remain separate stages. Music identification and weekly rankings of the monitored station sample are planned after the first release.

Internet radio and podcasts are the first source priorities, before physical radios. Podcast feeds, supplied transcripts, chapters and episode metadata will reuse the same recording, evidence and processing controls. RSS/Atom text analysis extends that path later.

Meshtastic, other LoRa integrations, and software defined radios such as HackRF Pro belong to the hardware roadmap. The broader plan includes Morse decoding and practice, a visual historical cipher workbench with Enigma, and modern authenticated and post-quantum cryptography using supplied keys. Exploration and fun are product goals alongside dependable unattended operation. Native and web interfaces remain future possibilities.

Start with [Intent](INTENT.md), the [Roadmap](ROADMAP.md), [Design documents](docs/README.md), and [Topic research](research/README.md). The [planning index](docs/planning/README.md) distinguishes confirmed requirements, proposed designs, research findings, and unresolved decisions. Research notes carry their review dates; the latest acquisition review is September 21, 2026.

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
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json source add demo:v1 --name "Radio example" --url https://radio.example/audio
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json source list
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY --json source show demo:v1
```

Replace `PATH_TO_LIBRARY` with a dedicated private directory outside the checkout. Initialization creates a SQLite catalog with paid processing disabled. The service holds the exclusive library lock; maintenance commands reconnect to its operations while it is running. `service run` keeps the controller in the foreground; `service start` detaches it from the client. This does not install an OS startup service or qualify logout/reboot behavior. Stop the controller before replacing its binary with a different protocol version.

The example source is a placeholder. Registration stores configuration without contacting the station. Reusing a revision key cannot change its URL, name or network permission. Public-internet access is the default; `--pin-address` explicitly binds a revision to one supported IP, including a private or loopback address. Lists are paginated with `--limit` and `--after`. Displayed origins omit paths and queries; full URLs remain in the private catalog in plaintext. Do not put access credentials in source URLs. See [source authority and transport](docs/decisions/0004-source-authority-and-http.md).

The capture journal records intent, revisions and worker generations and marks abandoned active attempts interrupted on startup. Recording is opt-in through the running service. Provider/model dispatch remains unavailable. Amounts in JSON are exact decimal USD strings. Windows x86_64 is locally tested; Linux/macOS native validation and release packaging remain pending.

## Find radio stations

Start the service using the commands above, then request a directory page:

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio refresh french-news-001 --language french --tag news --limit 100
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio refresh-status french-news-001
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio search --language french --tag news
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio show STATION_UUID
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio add STATION_UUID --revision selected:v1
```

Wait for refresh status to become completed, then replace `STATION_UUID` with an ID from search. Search supports `--name`, `--country`, `--language`, `--tag`, `--healthy` and pagination. It works offline against the partial local cache. Refresh uses the network only when requested; choose a new refresh ID to fetch again. Mirror discovery, timeouts, response sizes and cache growth are bounded. Refresh and registration do not contact station streams, play audio or run analysis.

Observation age and directory health are shown separately from actual stream compatibility. Directory languages are hints, not detected speech. A station can change languages, programmes, ads and songs; the [broadcast analysis contract](docs/design/broadcast-analysis.md) plans separate revisable timelines for those changes. No such detectors are implemented yet.

`radio add` preserves a metadata snapshot and registers the chosen public stream as an immutable source. Use `selected:v1` with `record start --source` below. Some catalog entries use redirects, playlists or HLS that the initial recorder cannot handle. This is a bounded discovery increment, not a complete radio player. See [directory behavior and limits](docs/decisions/0006-radio-discovery.md).

## Record and manage audio

Configure a trusted installed FFmpeg executable once. The following are actual commands; replace both paths and the placeholder source URL above. Source registration must precede recording.

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY dvr configure --decoder ABSOLUTE_PATH_TO_FFMPEG --quota-gb 50 --retention-days 14
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY service start
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY record start morning-001 --source demo:v1 --seconds 60 --max-mib 64
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY record show morning-001
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY record path morning-001
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY record metadata morning-001
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY record keep morning-001
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY dvr status
```

In PowerShell, `(Get-Command ffmpeg -CommandType Application).Source` locates an installed decoder. No executable is downloaded automatically. `record start` returns while the service continues working. Poll `record show` until completed or failed. `record path` prints a verified retained file's path for a media player; audible playback inside Sigy is not implemented yet. `record metadata` emits a versioned JSON sidecar snapshot. Reusing the same recording ID and parameters only reconciles the prior request; choose a new ID for another recording.

Default retention is 14 days with a 50 GB total managed-media quota. Temporary media expires or is evicted oldest-first under quota pressure. `record keep ID` and `record archive ID` protect it from automatic removal, while its bytes still count toward quota. Archive does not create a backup. New recordings fail clearly if protected media fills the allowance. `record temporary ID` restores rolling retention. `record processed ID --receipt RECEIPT_ID` acknowledges external processing and makes temporary media eligible for early cleanup; this does not run analysis. `dvr prune` reclaims eligible media, and `record delete ID` explicitly deletes inactive media even if protected, retaining catalog history.

This initial recording profile accepts direct audio responses, with up to two active attempts, 15 minutes and 256 MiB per attempt. Redirects, playlists/HLS, interleaved ICY metadata, continuous segmented DVR and podcast downloads are not yet qualified. Failed/partial bytes retain their reservation and are not offered as playable media. The decoder is supervised, but aggregate native-memory sandboxing and physical power-loss durability remain open. See [recording and retention](docs/decisions/0005-recording-and-retention.md) and the extensible [recording metadata design](docs/design/recording-metadata.md).

Run `./scripts/verify.ps1` in PowerShell for native-source verification, formatting, tests, warnings-denied Clippy, build, and dependency auditing. It requires cargo-audit and fails if a check is unavailable. Run `./scripts/verify-media.ps1` separately with an installed FFmpeg for the real recording/decoder and process-kill tests. Builds use two jobs; tests run with bounded concurrency. Command examples in planning documents remain proposals unless implemented and documented here.

## Lawful use and responsibility

Sigy is intended for lawful listening, research, learning, and analysis of sources you are authorized to access. You are responsible for complying with the laws and permissions applicable to your location, equipment, and use, including rules governing reception, interception, recording, privacy, decryption, radio transmission, copyright, and redistribution. A signal being receivable or a stream being accessible does not by itself establish permission to record, decrypt, publish, or reuse it.

Do not use Sigy for unauthorized access or interception, unlawful decryption or disclosure, harmful interference, or transmission without any required authorization. Respect source and service terms. Hardware integration is initially scoped to reception; modern decryption workflows use supplied keys and supported protocols.

Transcriptions, translations, identifications, and generated findings can be incorrect. Check important results against the original material. This documentation is not legal advice, and this notice does not make an otherwise unlawful activity permissible. It does not add restrictions to the Apache License.

## License and warranty

Copyright 2026 Nick Seal.

Sigy is licensed under the [Apache License, Version 2.0](LICENSE) (`Apache-2.0`). Third-party components and content remain subject to their own licenses and required notices.

Unless required by applicable law or agreed to in writing, Sigy is provided on an "AS IS" basis, without warranties or conditions of any kind. The warranty disclaimer and limitation of liability are set out in Sections 7 and 8 of the [license](LICENSE).
