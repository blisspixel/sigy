# Sigy

**Signal intelligence for everyone.**

The world is broadcasting all the time: thousands of radio stations, podcasts in every language, and signals most people never hear. Sigy is a local-first toolkit for exploring those signals and working out what they mean. Find a station on the other side of the world, record it, read what was said in its original language, get an English translation, and follow a topic across many sources. Every answer links back to the exact moment of audio it came from.

Signals intelligence has usually meant government analysts and expensive equipment. Sigy brings the useful, lawful part of it to anyone with a laptop and some curiosity: open sources, your own machine, your own library, and results you can check.

> **Development preview.** Radio and podcast discovery, listening, recording, schedules, a terminal explorer, and early local speech recognition and translation work today on Windows. Topic monitoring is being built now. See [what works today](#what-works-today) and the [progress record](docs/development/progress.md).

## What Sigy is for

- **Explore world radio.** Search and browse thousands of stations by name, country, language, and tag. Listen, save favorites, and see where a station is and when it last answered.
- **Keep what matters.** Record live streams and podcast episodes into a private library. Pause and rewind a running capture, schedule recordings in any time zone, and hold the parts you want to keep.
- **Understand any language.** Most of the world's audio is not in English. Sigy is designed to transcribe speech in its original script, identify the language (including mixed and uncertain speech), and translate into English, with the original always beside the translation.
- **Follow a topic.** Ask Sigy to watch a subject across sources, within limits you set. Findings cite the exact recording, transcript, and translation behind them, show where reports agree or conflict, and say when evidence is missing.
- **Play and learn.** Later stages add shortwave, CB, and spectrum views for receive-only radios, Morse practice, and a visual Enigma machine. Curiosity is reason enough.

## Principles

- **Local first.** Everything runs on your machine by default. Nothing is sent to a paid service unless you set an explicit spending limit, and a zero limit always refuses.
- **Show your work.** Original recordings, transcripts, translations, and interpretations are kept separate. You can trace any finding to its source and see gaps, expired audio, and uncertainty.
- **Honest about limits.** Sigy says "unknown", "unsupported", or "not measured" rather than guessing. Language support is claimed only for languages that have passed measured tests.
- **Always on, never in the way.** A background service owns recordings and monitoring. Closing the terminal does not stop a recording.
- **Correct, don't erase.** Fix a name, a language, or a transcript without losing history. Results that depend on the correction are marked for review.

## What works today

| Area | Status on Windows x86_64 |
| --- | --- |
| Station directory | Refresh pages from Radio Browser, search offline, favorites, scheduled refresh |
| Listening | Direct streams, playlists, finite HLS, retained recordings with seek |
| Recording | Segmented captures with gaps, pause/rewind playheads, holds, 14-day/50 GB retention |
| Schedules | Once, daily, or weekly recordings in any IANA time zone, including daylight-saving edges |
| Podcasts | Subscribe, refresh RSS, download and play episodes, fetch publisher transcripts and chapters |
| Terminal explorer | List explorer with search, favorites, health, recordings, and playback |
| Agents | An MCP server so an assistant can use the same operations inside your library |
| Speech recognition | Early: transcribe a retained recording in its original script with your own local whisper.cpp model, on the CPU, fully offline. Measured on 32 reference clips across eight languages and tried on five live stations |
| Translation | Early: translate recognized text into English with your own local llama.cpp model, cue by cue beside the original. Not yet quality-checked |
| Topic monitoring, globe, visualizers | Planned for the first complete release |
| Receive-only hardware, Morse, historical ciphers | Planned after the first complete release |

Tested formats: WAV, MP3, AAC, FLAC, and Ogg Vorbis, decoded by a local FFmpeg 9.0.1. macOS and Linux build from source but are not yet qualified.

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

Sigy can now transcribe a retained recording on your own machine: a hash-pinned multilingual recognizer runs under strict process limits, and the text keeps its original script and exact media time. Early English translation runs the same way, cue by cue beside the original. The next steps are reference-scored translation checks, longer recordings, live HLS stations, and then topic monitoring. The [roadmap](ROADMAP.md#whats-next) explains the order and the reasons, and the [language pipeline plan](docs/development/language-pipeline.md) defines how quality is measured.

## Learn more

- [Usage and command reference](docs/usage.md)
- [Progress and evidence](docs/development/progress.md)
- [Product intent](INTENT.md) and [roadmap](ROADMAP.md)
- [Architecture and design](docs/README.md)

## Lawful use and license

Use Sigy only with sources and material you are authorized to access. Reception, recording, decryption, transmission, and redistribution can require different permissions where you live. Transcriptions, translations, and findings can be wrong; check important results against the original material. The [lawful-use guide](docs/usage.md#lawful-use) has more detail. These notices are general information, not legal advice or permission.

Copyright 2026 Nick Seal. Sigy is licensed under the [Apache License, Version 2.0](LICENSE), including its warranty disclaimer and liability limits. Third-party software and content retain their own licenses and notices. See [contributing](CONTRIBUTING.md) and [security reporting](SECURITY.md).
