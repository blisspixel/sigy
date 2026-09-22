# Changelog

## Unreleased

- Added a Rust workspace with a dependency-free domain core, transactional service storage, and an initial maintenance CLI.
- Added exact USD accounting, global and named lifetime budgets, atomic reservations, submission uncertainty, idempotent reconciliation, and billing-breach freezes. Provider dispatch remains unavailable.
- Added exclusive library ownership, schema identity checks, SQLite migrations, and append-only ledger events.
- Added a bounded local controller with foreground/detached operation, client reconnect, explicit shutdown, protected local IPC, and restart reconciliation. Native Windows behavior is tested; OS startup installation remains pending.
- Added capture intent, atomic admission, revision/generation checks, append-only job history, interrupted-attempt recovery, integrity audits and bounded reads.
- Added schema v3 immutable HTTP source revisions, exact replay/conflict handling, explicit network grants and paginated CLI registration/list/inspection through the shared controller.
- Added finite HTTP acquisition with checked destinations, disabled implicit proxies/retries, explicit bounded redirect policy, strict response handling, byte/time limits and shared admission. Successful recordings retain origin/peer/status provenance in metadata envelope v2.
- Added service-owned finite recordings with decoder validation, original-byte hashes, 14-day/50 GB rolling retention, Keep/Archive protection, processing acknowledgments and staged deletion.
- Added bounded Radio Browser refresh, cached multilingual search and immutable source registration with directory provenance. Refresh does not contact station streams.
- Added persistent radio favorites and filtered offline search through shared service/maintenance operations. Catalog and IPC are v7; saved choices survive refresh and restart.
- Added truncated-catalog rejection, conservative build/test concurrency, bounded CLI test processes, and regression coverage for detached output-handle inheritance.
- Added local tests for accounting, concurrency, restart recovery, injected storage failures, library locking, and real CLI operations.
- Pinned Rust 1.98.1 and verified SQLite 3.53.4 source through a documented temporary native dependency override.
- Extended the roadmap with optional user-controlled proxy routing and external VPN compatibility for personal machines and servers. Routing profiles, remote access, podcasts, integrated playback, TUI, model processing and monitoring remain unimplemented.
- Recorded the build order from retained-file playback through the first complete release. That order is the implementation sequence.
- Added foreground playback of one retained recording through the configured FFmpeg decoder. The client owns the playhead. Playback does not contact the source, reserve quota, or stop capture. Station listening remains unimplemented.
- Added bounded playlist resolution on the shared acquirer. One request reads a playlist document, rejects HLS, and does not open entry URLs. Accepting an index registers a new audio revision without another fetch. Catalog and IPC are v8.
- Verification is `cargo verify` and `cargo verify-media`, implemented by the Rust `sigy-xtask` package. PowerShell is no longer required.
- Added an explicit Radio Browser click. One command sends `GET /json/url/{stationuuid}` and discards the returned stream URL. Search, refresh, and favorites do not send it. Replay does not send it again. Catalog and IPC are v9.
- Added a finite listen of one direct audio revision. The service fetches through the shared acquirer and writes a private local pipe. The client decodes that pipe and does not receive a source URL. The listen is not a recording, does not reserve quota, and is not resumed after restart. Replay does not reconnect. A playlist or ICY response fails before decode. Catalog and IPC are v10.
- Corrected the development instructions so they match retained-file playback, the roadmap build order, and the current verification commands.
- Pushing `main` runs `cargo verify` on one standard GitHub-hosted Windows runner. `cargo verify-media` stays local. There is no product release.
- Added explicit ICY observations on `record start --icy`. Metadata blocks are removed before the audio file is hashed. Titles do not rename a source or grant a URL. The default, every listen, and HLS recording still reject interleaved metadata. Export schema is version 3 only when observations exist. Catalog schema is v11 and local IPC is v12.
- Added finite HLS media-playlist recording. `record hls` fetches one ended media playlist on the shared acquirer, at most 32 segments, and publishes the concatenated audio through the existing recording path. A master playlist fails before a variant fetch. Live playlists, encryption, maps, byte ranges, and discontinuity are rejected. Segments do not become source revisions, and the decoder does not receive an `.m3u8` address. Catalog schema remains v10. Local IPC is v11.
