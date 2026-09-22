# Design documents

Last updated: 2026-09-22

Read [Intent](../INTENT.md) first, followed by the [Roadmap](../ROADMAP.md).

Implementation has begun. Read [active work and evidence](development/progress.md), the [usage guide](usage.md), and the [Rust foundation decision](decisions/0001-rust-foundation.md) for current state. The planning chapters retain the broader product contract; they are not a list of shipped features.

Implemented boundaries are recorded in the [local controller](decisions/0002-local-controller.md), [capture journal](decisions/0003-capture-journal.md) and [source authority and HTTP transport](decisions/0004-source-authority-and-http.md) decisions. Focused implementation targets cover the [terminal experience](design/terminal-experience.md), [language coverage and localization](design/languages.md), and [universal signal interpretation](design/signal-interpretation.md).

Finite recording and its current limitations are covered by [recording and retention](decisions/0005-recording-and-retention.md). The [metadata contract](design/recording-metadata.md) separates shared envelopes from podcast, SDR, CB, LoRa and Meshtastic profiles.

[Directory refresh and local search](decisions/0006-radio-discovery.md) records the first discovery adapter. The planned [broadcast analysis contract](design/broadcast-analysis.md) separates language shifts, candidate advertisements and song boundaries from source metadata and track identification.

[Authorized redirects](decisions/0007-authorized-redirects.md) adds explicit source policy and retained HTTP route observations to the existing acquisition path.

[Radio favorites](decisions/0008-radio-favorites.md) keeps saved station choices separate from refreshed directory metadata, with shared service/maintenance operations and filtered offline search.

[Playlist resolution](decisions/0009-playlist-resolution.md) reads one playlist document on the shared acquirer and can register one accepted entry as an immutable audio revision. It does not play that entry or fetch nested documents.

[Directory clicks](decisions/0010-directory-clicks.md) send one explicit counter request and discard the stream URL in the provider response. Refresh, search, and favorites do not send it.

[Direct listen](decisions/0011-direct-listen.md) plays one audio revision through the shared acquirer and a private local pipe. The client decodes that pipe. The listen is not a recording.

[HLS media playlists](decisions/0012-hls-media-playlist.md) record one finite media playlist through the shared acquirer and the existing recording path. A master playlist fails before a variant fetch. Playlist resolution still rejects HLS.

[ICY observations](decisions/0013-icy-observations.md) keep explicit stream titles out of the published audio. The default recording and every listen still reject interleaved metadata.

[Decoded formats](decisions/0014-decoded-formats.md) names only the formats one local run decoded, on the operating system and FFmpeg build that ran them.

[Terminal stack](decisions/0015-terminal-stack.md) selects Ratatui with the Termina backend. [The list explorer](decisions/0016-list-explorer.md) uses that backend. The client still opens no second catalog. The globe and map are not drawn.

[Local podcast subscriptions](decisions/0017-local-podcast-subscriptions.md) store one feed URL, scope, pin, and redirect policy. Subscribe does not resolve DNS or start a capture. Unsubscribe stops future polls and deletes nothing.

[RSS feed refresh](decisions/0018-rss-feed-refresh.md) reads one RSS 2.0 document on the shared acquirer and lists episodes. It does not download enclosures. A failed document leaves the last good snapshot.

[Episode enclosure download](decisions/0019-episode-enclosure.md) fetches one stored enclosure through the existing recording path. It reserves 512 MiB and 30 minutes before connecting. Feed text does not grant that fetch.

[Retained episode playback](decisions/0020-retained-episode-playback.md) plays that file with `listen file`. The episode has no live edge. Subscribe and refresh do not start playback.

[Agent plugin](decisions/0021-agent-plugin.md) exposes those commands over MCP 2026-07-28. The package is Agent Plugins 1.0.0. A tool cannot choose another library or change a budget.

[Publisher text](decisions/0022-publisher-text.md) fetches one transcript or chapter document only when asked. Cue times are not media time, and the text is not an ASR row.

[Measured recording intervals](decisions/0023-recording-intervals.md) project one published file onto decoded duration and byte length. The planned window and an unpublished part are not airtime.

[Doctor and cache freshness](decisions/0024-doctor-and-cache-freshness.md) checks the library without using the network. A stale station cache stays in place until an explicit refresh.

[Segment seals](decisions/0025-segment-seals.md) close a running radio capture at 32 MiB or 5000 ms of receive time. The 5000 ms bound is not a measured durability result.

[Capture gaps](decisions/0026-capture-gaps.md) record a hole with a cause. A seek inside that range fails, and no silence file fills it.

[Segment playback](decisions/0027-segment-playback.md) gives each listener a playhead over sealed segments. Pausing a playhead or leaving `listen play` does not stop the capture.


Planned [network routing](design/network-routing.md) covers optional user-configured proxies, external VPN compatibility and the distinction between personal-machine and server deployment. It preserves explicit destinations, DNS policy and route evidence.

The [planning index](planning/README.md) records confirmed requirements and the status of the design. Detailed documents cover:

1. [Product and experience](planning/01-product-and-experience.md)
2. [Architecture and data](planning/02-architecture-and-data.md)
3. [Research synthesis](planning/03-source-and-model-research.md)
4. [Assurance and validation](planning/04-assurance-and-validation.md)
5. [Delivery and decisions](planning/05-delivery-and-decisions.md)
6. [Language and stack trade study](planning/06-language-and-stack-trade-study.md)
7. [Providers and cost policy](planning/07-providers-and-cost-policy.md)
8. [Signal extensions and workbench](planning/08-signal-extensions-and-workbench.md)
9. [Security, privacy, and release design](planning/09-security-privacy-and-release.md)
10. [Analysis, knowledge, and multi-stream processing](planning/10-analysis-and-knowledge.md)
11. [Complete CLI, terminal explorer, and radio DVR](planning/11-radio-explorer-and-dvr.md)
12. [Repository organization and engineering](planning/12-repository-and-engineering.md)

[Research](../research/README.md) contains the dated evidence, alternatives, and unresolved experiments for individual topics. Specifications describe proposed behavior; research explains the evidence behind it. Neither implies an implemented capability.

[AGENTS.md](../AGENTS.md) is the canonical concise development guidance. [Naming research](../research/21-naming.md) retains historical screening; the current decision is to keep Sigy.
