# Research synthesis

Baseline: September 20, 2026. Detailed sources and unresolved experiments are under [research](../../research/README.md).

## Principal findings and design implications

| Finding | Implication | Evidence |
| --- | --- | --- |
| Public radio directories expose useful discovery fields but depend on changing mirrors and station metadata | Use an adapter, cache, provenance, health observations, and direct-URL support | [Internet radio](../../research/01-internet-radio.md) |
| Media engines provide decoding and segmentation primitives, not Sigy's complete durability contract | Design a manifest, independent capture lifecycle, time model, and recovery protocol | [Audio and media](../../research/02-audio-and-media.md) |
| Live speech needs incremental recognition behavior as well as translation | Separate ASR and translation capabilities; benchmark partial/final output and language quality | [Speech and translation](../../research/03-speech-and-translation.md) |
| A local model endpoint can route elsewhere; hosted billing includes in-flight uncertainty | Record effective destinations and reserve bounded liability before paid requests | [Providers and costs](../../research/04-model-providers-and-costs.md) |
| Mesh messages and raw IQ differ fundamentally from internet audio | Share evidence/job contracts while preserving typed source capabilities | [Hardware and signals](../../research/05-hardware-and-signals.md) |
| Native-language safety and service simplicity have different benefits and costs | Compare Rust and Go on the actual service/media boundary, without invented benchmark scores | [Language and stack](../../research/07-language-and-stack.md) |
| A transactional catalog does not atomically commit separate media files | Test crash reconciliation, retention, consistent backup, and restore | [Storage and evidence](../../research/08-storage-and-evidence.md) |
| Fingerprinting and sampled airplay require distinct quality and coverage work | Build music after the first release while specifying its data path now | [Monitoring and music](../../research/09-monitoring-and-music.md) |
| A source can change language within a block; speech, lyrics, and metadata need different quality evidence | Preserve language spans and original scripts; qualify a majority non-English corpus by language/task | [Multilingual processing](../../research/10-multilingual-processing.md) |
| Signals can be samples, symbols, packets, or telemetry without any audio | Specify typed adapters, clock mappings, decoder boundaries, and a replayable Morse workspace | [Non-audio and Morse](../../research/11-non-audio-and-morse.md) |
| Historical simulation, authenticated encryption, signatures, and key establishment have different contracts | Provide an engaging historical workbench and separately qualified modern operations | [Cryptography](../../research/12-cryptography.md) |
| Remote input, native decoders, paid providers, and update channels introduce distinct trust boundaries | Validate each boundary and maintain release/restore evidence independent of language choice | [Security and release engineering](../../research/13-security-and-release-engineering.md) |
| Aggregation and retained topic context serve different responsibilities from raw evidence and operational state | Keep versioned findings, user annotations, and current topic views linked to immutable retained inputs | [Aggregation and context](../../research/14-aggregation-and-context.md) |
| Tool calling and structured outputs do not supply durable execution or factual verification | Use bounded model proposals, deterministic admission, and whole-monitor evaluation | [Agentic analysis](../../research/15-agentic-analysis.md) |
| Jev has a documented OpenRouter decision endpoint, text-only inputs, and language/precision limitations | Evaluate it as optional semantic classification alongside local alternatives; retain strict billing admission and deterministic statistics | [Decision models and classifiers](../../research/16-decision-models-and-classifiers.md) |
| Local recognizers have differing coverage and throughput; recording capacity exceeds analysis capacity on some hosts | Qualify language-specific live/batch profiles and bound durable queues without requiring paid fallback | [Local processing and capacity](../../research/17-local-processing-and-capacity.md) |
| Terminal drawing supports geographic primitives, while station coordinates, solar context and retained audio have distinct meanings | Specify the globe/day-night/map/list experience, complete CLI, catalog refresh and DVR lifecycle before selecting a TUI stack | [Terminal explorer and radio DVR](../../research/18-terminal-explorer-and-radio-dvr.md) |
| Feeds contain revisable entries and optional finite media; their content and timestamps differ from live streams | Add post-release RSS/Atom and podcasts through bounded polling/download adapters and versioned evidence | [Podcasts and feeds](../../research/19-podcasts-and-feeds.md) |

## Platforms and terminal behavior

Persistent services have different user-session and logout semantics across operating systems. Separate unattended collection from interactive playback, and test native OS service installation independently. Terminal Unicode, focus, resizing, and accessible plain output need a declared compatibility matrix. [Platform research](../../research/06-platforms-and-tui.md).

## Current evidence boundary

This phase contains primary-source desk research and a small number of read-only connectivity observations. It contains no implementation, model ranking, performance result, live billing experiment, or device qualification.

Research conclusions support a design review. Hardware profiles, language-pair quality, precise media formats, technology versions, distribution, and latency/capacity limits require the planned decision gates. Rolling documentation and hosted-provider semantics must be rechecked before relying on them in a release.
