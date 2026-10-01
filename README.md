# Sigy

**Practical signal intelligence for everyone.**

The world is broadcasting all the time: internet radio and podcasts in every language, and on the airwaves shortwave, FM, CB, LoRa mesh networks and countless signals most people never hear. Sigy is a local-first toolkit for exploring those signals and working out what they mean. Find a station on the other side of the world, record it, read what was said in its original language, get an English translation, and follow a topic across many sources. Every answer links back to the exact moment it came from.

It starts with internet radio and podcasts, which need nothing but a computer. The same design extends to receive-only software-defined radio (such as a HackRF), LoRa and Meshtastic devices, and other signals: every source, whether a stream, a recording, IQ samples or packets, flows through the same capture, evidence and analysis pipeline. Your own machine, your own library, results you can check.

> **Development preview.** Radio and podcast discovery, listening, recording, schedules, a terminal explorer, and early local speech recognition and translation work today on Windows. Early topic monitoring now transcribes and translates what it records, within caps you set. See [what works today](#what-works-today) and the [progress record](docs/development/progress.md).

## What Sigy is for

- **Explore world radio.** Search and browse thousands of stations by name, country, language, and tag. Listen, save favorites, and see where a station is and when it last answered.
- **Keep what matters.** Record live streams and podcast episodes into a private library. Pause and rewind a running capture, schedule recordings in any time zone, and hold the parts you want to keep.
- **Understand any language.** Most of the world's audio is not in English. Sigy is designed to transcribe speech in its original script, identify the language (including mixed and uncertain speech), and translate into English, with the original always beside the translation.
- **Follow a topic.** Ask Sigy to watch a subject across sources, within limits you set. Findings cite the exact recording, transcript, and translation behind them, show where reports agree or conflict, and say when evidence is missing.
- **Give the toolkit a task.** Planned durable workflows turn a bounded goal into collection, analysis and a cited result using qualified local models. Optional configured providers and agent integrations use the same service permissions and limits. Interrupted work retains its progress; the general task harness is not implemented yet.
- **Tune the airwaves.** Planned hardware support turns a software-defined radio or a LoRa device into another source: an AM/FM dial, shortwave and CB scanning, spectrum and waterfall views, and Meshtastic packet inspection, all receive-only and recorded with the same evidence as a stream.
- **Play and learn.** Morse practice, timing and packet puzzles, and a visual Enigma machine. Curiosity is reason enough.

## Principles

- **Local first.** Everything runs on your machine by default. Nothing is sent to a paid service unless you set an explicit spending limit, and a zero limit always refuses.
- **Show your work.** Original recordings, transcripts, translations, and interpretations are kept separate. You can trace any finding to its source and see gaps, expired audio, and uncertainty.
- **Honest about limits.** Sigy says "unknown", "unsupported", or "not measured" rather than guessing. Language support is claimed only for languages that have passed measured tests.
- **Always on, never in the way.** A background service owns recordings and monitoring. Closing the terminal does not stop a recording.
- **Private and recoverable.** Private bounded local diagnostics, no automatic analytics or diagnostic uploads, and tested recovery are product requirements. Necessary task and evidence state is kept separately from ordinary logs. These goals are still being implemented and qualified.
- **Correct, don't erase.** Fix a name, a language, or a transcript without losing history. Results that depend on the correction are marked for review.

## What works today

| Area | Status on Windows x86_64 |
| --- | --- |
| Station directory | Refresh pages from Radio Browser, search offline, favorites, scheduled refresh |
| Listening | Direct streams, playlists, finite HLS, retained recordings with seek |
| Recording | Segmented captures with gaps, finite and live HLS stations, pause/rewind playheads, holds, 14-day/50 GB retention |
| Schedules | Once, daily, or weekly recordings in any IANA time zone, including daylight-saving edges |
| Podcasts | Subscribe, refresh RSS, download and play episodes, fetch publisher transcripts and chapters |
| Terminal explorer | Search, favorites, health and playback; read-only monitor coverage, passages and named findings; recording timelines that distinguish available audio, released intervals, gaps and unpublished time |
| Agents | An MCP server and packaged skill expose selected operations inside one library. General task orchestration, local planning transport, live paid transport and A2A remain planned |
| Tasks | Early CLI slice: immutable monitor-bound goals and frozen evidence checkpoints. Explicit execution materializes bounded task-owned literal findings and an exact-membership briefing, with durable receipts and cancellation. General planning and task-owned collection remain planned |
| Speech recognition | Early: transcribe a retained recording in its original script with your own local whisper.cpp model, on the CPU, fully offline. Measured on 32 reference clips across eight languages and tried on eight live stations. No language is qualified |
| Translation | Early: translate recognized text into English with your own local llama.cpp model, cue by cue beside the original. A 32-clip calibration found critical meaning errors. A subsequent 126-control local judge screen failed its criteria in every tested language. No model or language is qualified |
| Corrections | One cue's original script can be replaced by appending a revision. The previous revision stays readable. Times are copied and wording stays uncertain. Names, language spans, and automatic reprocessing are not included |
| Globe and day/night map | Early: an orthographic globe or flat map with offline coastlines, geometric night at an explicit time, and the current filtered station page. Crowded terminal cells show a count and preserve the selected station |
| Topic monitoring | Early: a monitor keeps your terms in any script, the stations to follow and daily and total processing caps. Opt-in owned schedules capture within separate daily seconds, lifetime seconds and lifetime byte ceilings. New recordings are transcribed and translated automatically within processing caps. Coverage and literal matches cite the exact cue and revision. A stored finding cites original and English text plus a retained interval, or states expired or missing audio; the terminal can inspect that citation and navigate to recording metadata. A briefing freezes coverage, counts a repeated report once, and leaves a different report unresolved. Its export is a redacted snapshot. Classification stays off; agents cannot publish findings or briefings |
| Visualizers | Recording metadata timelines work. Live waveform and frequency views remain planned |
| Software-defined radio, LoRa and Meshtastic (receive-only) | Planned; source contracts for IQ samples and packets are designed, and devices will be tested with real hardware |
| Morse, historical ciphers | Planned after the first complete release |

Tested formats: WAV, MP3, AAC, FLAC, and Ogg Vorbis, decoded by a local FFmpeg 9.0.1. An earlier Linux container run passed the ordinary test suite; local recognition and translation there still need delegated process containment. The current increment has been checked on Windows. macOS is untested. Neither Linux nor macOS is qualified yet.

## Preview

![Sigy list explorer with a selected station from one directory page](docs/images/tui.png)

The list explorer showing one 16-station directory page. Health and language fields come from the directory; nothing is playing or recording. Selecting a station never starts audio or contacts the station.

![Sigy command-line help on Windows](docs/images/cli.png)

The command line and the explorer use the same service operations. The [usage guide](docs/usage.md) lists every command.

## Install from source

The installer fetches the latest `main` from [blisspixel/sigy](https://github.com/blisspixel/sigy), builds it with the pinned Rust toolchain, and installs `sigy` for the current user. Git is required. If Rust is missing, the script installs Rust 1.98.1 for that user. Review [install.ps1](scripts/install.ps1) or [install.sh](scripts/install.sh) before running a remote script.

Windows PowerShell:

```powershell
& { $ErrorActionPreference = 'Stop'; iex (Invoke-RestMethod -Uri https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.ps1) }
```

macOS or Linux:

```sh
sh -c 'f=$(mktemp) || exit; curl --proto "=https" --tlsv1.2 -fsS https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.sh -o "$f" && sh "$f"; s=$?; rm -f "$f"; exit "$s"'
```

Recording and playback need a local FFmpeg executable, which the installer does not provide. [Installation details](docs/install.md) cover prerequisites, installing from a checkout, and platform limits.

## First steps

Choose a private library directory outside the source checkout. In PowerShell:

```powershell
$library = Join-Path $env:USERPROFILE '.sigy\library'
sigy --data-dir $library library init
sigy --data-dir $library doctor
sigy --data-dir $library service start
sigy --data-dir $library radio refresh first-page --limit 100
sigy --data-dir $library radio refresh-status first-page
sigy --data-dir $library tui
```

On macOS or Linux, set `library="$HOME/.sigy/library"` and use the same commands. `radio refresh` is the step that contacts the station directory; opening the explorer does not. [First use](docs/install.md#first-use) explains FFmpeg setup and recording.

Update with `sigy update --check` and `sigy update`. Let recordings finish and stop the service before replacing its binary; see [updating](docs/install.md#updating).

## Now building

Sigy can transcribe retained recordings in bounded local windows, preserve original scripts and media time, and translate cue by cue into English. Explicit monitor-owned schedules feed processing through separate capture and processing caps. [Durable task scope and checkpoints](docs/decisions/0066-durable-task-workflows.md) preserve a finite observation request and its cited progress. [Finite evidence execution](docs/decisions/0067-task-evidence-execution.md) materializes a selected checkpoint into bounded task-owned findings and a briefing through the existing service tick. [Atomic monitor processing](docs/decisions/0068-atomic-monitor-processing.md) commits each canonical job with its exact monitor charge before scheduling a worker. This closes an admission boundary needed by the next slice: task-owned collection and processing with explicit delegation and recovery. Qualified local planning, passage retrieval, cited audio playback and verifiable reports extend that workflow.

Language quality, measured host capacity, private diagnostics, recovery and native containment remain active gates. The [roadmap](ROADMAP.md#whats-next) explains the dependencies, the [task contract](docs/design/task-workflows.md) defines the proposed workflow, and the [language pipeline plan](docs/development/language-pipeline.md) defines how quality is measured. The design links to [agent interoperability research](research/15-agentic-analysis.md) and [privacy and recovery research](research/32-private-diagnostics-and-recovery.md); research and planned features are not support claims.

## Learn more

- [Usage and command reference](docs/usage.md)
- [Progress and evidence](docs/development/progress.md)
- [Product intent](INTENT.md) and [roadmap](ROADMAP.md)
- [Architecture and design](docs/README.md)

## Use responsibly

Laws about receiving, recording, decoding, storing and sharing signals and broadcasts vary by country and region, and they can differ for internet streams, radio frequencies, encrypted traffic and personal data. You are solely responsible for knowing and following every law, regulation, license condition and terms of service that applies to you, your equipment, your location and your use. Only access sources you are permitted to access, keep hardware receive-only unless you hold the required authorization, and respect privacy and copyright. That a signal can be received does not mean you may record, decode, publish or reuse it. The [usage guide](docs/usage.md#lawful-use) has more detail.

Transcriptions, translations and findings are machine output and can be wrong. Check anything important against the original.

Sigy is provided "as is", without warranty of any kind. The authors and contributors are not liable for any claim, damage or other liability arising from its use or misuse, as set out in the license. Nothing in this project is legal advice.

## License

Copyright 2026 Nick Seal. Sigy is licensed under the [Apache License, Version 2.0](LICENSE), including its warranty disclaimer and limitation of liability. Third-party software and content retain their own licenses and notices. See [contributing](CONTRIBUTING.md) and [security reporting](SECURITY.md).
