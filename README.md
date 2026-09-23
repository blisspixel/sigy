# Sigy

Sigy is a local-first place to explore radio and podcasts, keep recordings, and trace what you learn back to the original material. A persistent service owns collection, so closing the command line or terminal explorer does not stop a recording. The longer-term goal is multilingual transcription, translation, topic monitoring, and a workbench for other kinds of signals, all with visible uncertainty and controlled resource use.

**Development preview:** The current build has local Windows x86_64 test evidence, but it is not the first complete release. Radio discovery, listening, recordings, schedules, podcast episodes, and a list explorer work in bounded forms. Speech recognition, translation, topic monitoring, the globe, hardware adapters, and cipher tools are still being built. The [progress record](docs/development/progress.md) separates tested behavior from planned work.

The [source-only development prerelease](https://github.com/blisspixel/sigy/releases/tag/v0.1.0-dev.20260923) marks this checkpoint. It contains no prebuilt application binary or model. Use the source installer below for the current build.

## Why use it

- Keep station discovery, saved sources, podcasts, recordings, and their history in one private library.
- Inspect where a recording came from, when audio is missing, and which results are observations versus interpretations.
- Start locally with paid processing disabled. Future model and provider routes must stay inside explicit limits.

## Preview

![Sigy list explorer with a selected station from one directory page](docs/images/tui.png)

Rendered current explorer with one 16-station Radio Browser cache page. Directory health and languages are provider metadata; no stream is playing or recording. The globe and map are not implemented. Selecting a station does not start playback, capture, refresh, or a directory click.

![Current Sigy command-line help on Windows](docs/images/cli.png)

The command line exposes the same service operations. The [usage guide](docs/usage.md) has current commands and examples; the [roadmap](ROADMAP.md) describes what is still planned.

## Install from source

These scripts fetch the latest public `main` from [blisspixel/sigy](https://github.com/blisspixel/sigy), build with the pinned Rust toolchain, and install `sigy` for the current user. Git is required. If Rust is absent, the script installs Rust 1.98.1 for that user. This is a source install, not a release binary or a crates.io package. Windows is locally tested; macOS and Linux are not yet qualified. Review [install.ps1](scripts/install.ps1) or [install.sh](scripts/install.sh) before running a remote script.

Windows PowerShell:

```powershell
& { $ErrorActionPreference = 'Stop'; iex (Invoke-RestMethod -Uri https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.ps1) }
```

macOS or Linux:

```sh
sh -c 'f=$(mktemp) || exit; curl --proto "=https" --tlsv1.2 -fsS https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.sh -o "$f" && sh "$f"; s=$?; rm -f "$f"; exit "$s"'
```

The installers do not configure an operating-system startup service or install FFmpeg. Recording and playback need a trusted local FFmpeg executable. [Installation and update details](docs/install.md) cover prerequisites, installing from a checkout, and current platform limits.

## First use

Choose a private library directory outside the source checkout. In PowerShell:

```powershell
$library = Join-Path $env:USERPROFILE '.sigy\library'
sigy --data-dir $library library init
sigy --data-dir $library doctor
sigy --data-dir $library service start
sigy --data-dir $library radio refresh first-page --limit 100
sigy --data-dir $library radio refresh-status first-page
```

Repeat `refresh-status` until it reports completion, then open the explorer:

```powershell
sigy --data-dir $library tui
```

On macOS or Linux, set `library="$HOME/.sigy/library"` and use the same `sigy --data-dir "$library"` commands. `radio refresh` explicitly contacts a station directory; opening the explorer does not contact a stream. Use a new request ID for another refresh. [First-use and recording steps](docs/install.md#first-use) explain FFmpeg configuration, service status, and safe updates.

To check for or install a newer `main` commit:

```text
sigy update --check
sigy update
```

`--check` fetches `main` into the managed source checkout and compares commits without installing. On Windows, an update finishes after the running `sigy` process exits. Let recordings finish, then stop the service before replacing its binary; see the [update guide](docs/install.md#updating).

## Next build step

The next build step is to resolve the Spanish decoder's workspace preflight timeout, then run local recognition and reference scoring on the three verified Arabic, Hindi, and Spanish calibration clips. The original audio is verified, but Spanish decoding and model execution have not run. That small end-to-end check comes before a ten-clip run or the frozen 112-clip screen. It will expose timing, resource use, and transcription errors early, without implying language support from successful downloads alone. The [language pipeline plan](docs/development/language-pipeline.md) gives the gates; the [progress record](docs/development/progress.md) shows which have passed.

## Learn more

- [Usage and command reference](docs/usage.md)
- [Current evidence and limitations](docs/development/progress.md)
- [Product intent](INTENT.md) and [build order](ROADMAP.md)
- [Architecture and design documents](docs/README.md)

## Lawful use and license

Use Sigy only with sources and material you are authorized to access. Reception, recording, decryption, transmission, and redistribution can require different permissions. Transcriptions and generated findings can be wrong; check important results against the original material. The [lawful-use guide](docs/usage.md#lawful-use) gives more detail. These notices are general information, not legal advice or permission.

Copyright 2026 Nick Seal. Sigy is licensed under the [Apache License, Version 2.0](LICENSE), including its warranty disclaimer and liability limits. Third-party software and content retain their own licenses and notices. See [contributing](CONTRIBUTING.md) and [security reporting](SECURITY.md).
