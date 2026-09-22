# Sigy

**Practical signals intelligence for everyday people.**

A CLI and TUI platform in development for discovering signals, recording them, understanding what they contain, following changes, and experimenting with how they work.

The planned complete CLI supports operation and automation. The optional TUI will add searchable, refreshable station catalogs, a rotatable terminal globe and day/night world map, source/activity visualizers, and DVR-style pause, rewind, scheduled recording, and replay within retained audio.

**Status: early Rust implementation. Radio Browser refresh, cached search and favorites, an explicit directory click, finite service-owned audio recording, retained-file playback, playlist resolution, a finite listen of one direct audio revision, finite HLS media-playlist recording, explicit ICY observations on `record start --icy`, decoder validation, metadata export and rolling storage policy work through the CLI. Local Windows tests and a live directory check support this increment. The [roadmap build order](ROADMAP.md#build-order) is the path to the first complete release. The next operation records which formats this profile has actually decoded. Podcast subscriptions, the TUI, and model processing remain to build.**

The project is keeping the name Sigy. Further naming exploration is deferred. See [implementation progress](docs/development/progress.md), the [foundation decision](docs/decisions/0001-rust-foundation.md), and the [planning checkpoint](docs/planning/05-delivery-and-decisions.md#8-current-checkpoint-2026-09-20).

The first complete release is intended to include world radio exploration, organized recordings, live translation, and autonomous topic monitoring within user-defined limits. A persistent background service will continue work when the terminal interface closes. Model processing will be local by default, with user-configured LAN and remote providers, including Ollama and OpenRouter. Paid processing requires enforced budgets and transparent accounting.

Multilingual use is fundamental: most expected listening and music are non-English. Sigy will identify languages within each content block, preserve originals, and translate primarily into English. Source contracts include audio, IQ samples, packets, telemetry, text, and other non-audio observations.

Local speech processing, reliable language detection, and durable live/batch queues are priorities for monitoring many streams without metered inference fees. Capture and analysis capacities will be qualified separately by machine and language. Optional classifiers can organize transcripts and identified music; reproducible statistics and evidence-linked findings remain separate stages. Music identification and weekly rankings of the monitored station sample are planned after the first release.

Internet radio and podcasts are the first source priorities, before physical radios. Podcast feeds, supplied transcripts, chapters and episode metadata will reuse the same recording, evidence and processing controls. RSS/Atom text analysis extends that path later.

The service is intended for personal machines and servers. Planned [network routing](docs/design/network-routing.md) lets users choose a proxy or use an externally managed VPN to reach sources from another network. This supports user choice and a free and open internet; proxy support and remote server access are not implemented yet.

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

The example source is a placeholder. Registration stores configuration without contacting the station. Reusing a revision key cannot change its URL, name, network permission or redirect policy. Public-internet access is the default; `--pin-address` explicitly binds a revision to one supported IP, including a private or loopback address. Lists are paginated with `--limit` and `--after`. Displayed origins omit paths and queries; full URLs remain in the private catalog in plaintext. Do not put access credentials in source URLs. See [source authority and transport](docs/decisions/0004-source-authority-and-http.md).

The capture journal records intent, revisions and worker generations and marks abandoned active attempts interrupted on startup. Recording is opt-in through the running service. Provider/model dispatch remains unavailable. Amounts in JSON are exact decimal USD strings. Windows x86_64 is locally tested; Linux/macOS native validation and release packaging remain pending.

## Find radio stations

Start the service using the commands above, then request a directory page:

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio refresh french-news-001 --language french --tag news --limit 100
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio refresh-status french-news-001
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio search --language french --tag news
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio show STATION_UUID
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio add STATION_UUID --revision selected:v1 --redirects public
```

Wait for refresh status to become completed, then replace `STATION_UUID` with an ID from search. Search supports `--name`, `--country`, `--language`, `--tag`, `--healthy` and pagination. It works offline against the partial local cache. Refresh uses the network only when requested; choose a new refresh ID to fetch again. Mirror discovery, timeouts, response sizes and cache growth are bounded. Refresh and registration do not contact station streams, play audio or run analysis.

Observation age and directory health are shown separately from actual stream compatibility. Directory languages are hints, not detected speech. A station can change languages, programmes, ads and songs; the [broadcast analysis contract](docs/design/broadcast-analysis.md) plans separate revisable timelines for those changes. No such detectors are implemented yet.

Save a station for later without opening its stream:

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio favorite STATION_UUID
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio search --favorites --language french
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio unfavorite STATION_UUID
```

[Favorites](docs/decisions/0008-radio-favorites.md) work offline and survive catalog updates and service restarts. Removing a favorite preserves the station, registered sources and recordings. Catalog schema remains v10 and local IPC is v11. Stop an older service with its existing binary before updating, then restart it.

Report one directory click only when you mean to. The command does not play the station, and the stream address in the provider response is discarded:

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio click heard-001 --station STATION_UUID
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY radio click-status heard-001
```

Search, show, favorite, and refresh do not send this request. Reusing the click ID does not send it again. See [directory clicks](docs/decisions/0010-directory-clicks.md).

`radio add` preserves a metadata snapshot and registers the chosen public stream as an immutable source. Use `selected:v1` with `record start --source` below. The example explicitly permits at most three redirects to checked public-internet destinations. Omit `--redirects` to deny them, or choose `same-origin` to stay within the original scheme, host and port. HTTPS cannot downgrade to HTTP. Existing revisions retain their policy; choose a new revision key to change it. See [directory behavior](docs/decisions/0006-radio-discovery.md) and [redirect policy](docs/decisions/0007-authorized-redirects.md).

Resolve one playlist document only through the running service. The read uses the parent revision's redirect policy, allows at most 32 entries, and does not open those entries. HLS tags and a directory HLS flag fail the resolve. Accepting an index registers a new audio revision and does not connect:

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY source playlist resolve playlist-001 --revision playlist:v1
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY source playlist status playlist-001
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY source playlist accept playlist-001 --index 0 --revision chosen:v1 --name "Chosen stream"
```

Reusing the request ID does not fetch again. Reusing the same accept does not register again. Displayed playlist entries show origins only. Resolving or accepting a playlist does not play it. See [playlist resolution](docs/decisions/0009-playlist-resolution.md).

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

Pass the installed FFmpeg executable to `dvr configure --decoder`. Sigy does not download it. `record start` returns while the service continues working. Poll `record show` until completed or failed. `record path` prints a verified retained file's path. Add `--icy` only when you want interleaved stream titles kept as observations. The default is off, and those titles are not written into the audio file. See [ICY observations](docs/decisions/0013-icy-observations.md).

`record hls` records one finite media playlist on the same service path. The document must include `#EXT-X-ENDLIST`. At most 32 segments share that recording's time and byte ceiling. The service publishes one local file through the same decoder check, hash, and quota. A master playlist fails before a variant request. A live playlist, encryption, a media map, a byte range, discontinuity, and partial segments are rejected. `source playlist resolve` still rejects HLS and stores no entries. The decoder receives the published file, not an `.m3u8` address. See [HLS media playlists](docs/decisions/0012-hls-media-playlist.md).

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY record hls segment-001 --source media:v1 --seconds 60 --max-mib 64
```

`listen file` plays a retained file in this client through the configured FFmpeg decoder:

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY listen file morning-001 --destination null
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY listen file morning-001 --destination system --seek-us 500000
```

`--destination null` discards samples and is the tested fixture path. `--destination system` uses a local audio output when that FFmpeg build has one, and fails when it does not. `--seek-us` starts inside the published duration. Run the command again to restart at another offset. Leaving it stops playback and leaves service-owned capture running. Playback does not stop a recording, change retention, or reserve quota. Partial files are not playable. `record metadata` emits a versioned JSON sidecar snapshot. Reusing the same recording ID and parameters only reconciles the prior request; choose a new ID for another recording.

Listen to one registered direct audio revision through the running service. The service fetches the bytes. This client decodes a private local pipe and does not receive the source URL:

```text
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY listen source live-001 --revision demo:v1 --destination null
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY listen status live-001
cargo run --locked -p sigy -- --data-dir PATH_TO_LIBRARY listen stop live-001
```

Reusing the listen ID does not open the source again. A playlist response or interleaved ICY metadata fails before decode. The session is not a recording and does not reserve quota. Restart marks a running listen interrupted and does not resume it. Stopping it does not stop a recording. See [direct listen](docs/decisions/0011-direct-listen.md).

Default retention is 14 days with a 50 GB total managed-media quota. Temporary media expires or is evicted oldest-first under quota pressure. `record keep ID` and `record archive ID` protect it from automatic removal, while its bytes still count toward quota. Archive does not create a backup. New recordings fail clearly if protected media fills the allowance. `record temporary ID` restores rolling retention. `record processed ID --receipt RECEIPT_ID` acknowledges external processing and makes temporary media eligible for early cleanup; this does not run analysis. `dvr prune` reclaims eligible media, and `record delete ID` explicitly deletes inactive media even if protected, retaining catalog history.

This initial recording profile accepts direct audio responses, explicitly permitted redirects, one finite HLS media playlist, and explicit ICY metadata on `record start --icy`, with up to two active attempts, 15 minutes and 256 MiB per attempt. Playlist media types still fail on `record start`. A listen, an HLS recording, and `record start` without `--icy` still reject an `icy-metaint` response before writing audio. Continuous segmented DVR and podcast downloads are not yet qualified. Failed/partial bytes retain their reservation and are not offered as playable media. The decoder is supervised, but aggregate native-memory sandboxing and physical power-loss durability remain open. See [recording and retention](docs/decisions/0005-recording-and-retention.md) and the extensible [recording metadata design](docs/design/recording-metadata.md).

From the repository root, run `cargo verify` for native-source verification, formatting, tests, warnings-denied Clippy, build, and dependency auditing. It requires cargo-audit and fails if a check is unavailable. A push to `main` runs that command on GitHub-hosted Windows. Run `cargo verify-media` with FFmpeg installed, or with `SIGY_TEST_FFMPEG` set to its path, for the real recording, decoder, and process-kill tests. That media check stays local. The repository Cargo configuration limits builds to two jobs. Command examples in planning documents remain proposals unless implemented and documented here.

## Lawful use and responsibility

Sigy is intended for lawful listening, research, learning, and analysis of sources you are authorized to access. You are responsible for complying with the laws and permissions applicable to your location, equipment, and use, including rules governing reception, interception, recording, privacy, decryption, radio transmission, copyright, and redistribution. A signal being receivable or a stream being accessible does not by itself establish permission to record, decrypt, publish, or reuse it.

Do not use Sigy for unauthorized access or interception, unlawful decryption or disclosure, harmful interference, or transmission without any required authorization. Respect source and service terms. Hardware integration is initially scoped to reception; modern decryption workflows use supplied keys and supported protocols.

Transcriptions, translations, identifications, and generated findings can be incorrect. Check important results against the original material. This documentation is not legal advice, and this notice does not make an otherwise unlawful activity permissible. It does not add restrictions to the Apache License.

## License and warranty

Copyright 2026 Nick Seal.

Sigy is licensed under the [Apache License, Version 2.0](LICENSE) (`Apache-2.0`). Third-party components and content remain subject to their own licenses and required notices.

Unless required by applicable law or agreed to in writing, Sigy is provided on an "AS IS" basis, without warranties or conditions of any kind. The warranty disclaimer and limitation of liability are set out in Sections 7 and 8 of the [license](LICENSE).
