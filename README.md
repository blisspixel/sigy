# Sigy

Sigy is practical signals intelligence for everyday use. One local-first application discovers a signal, listens to it or inspects it, captures what matters, decodes and translates it, and explains what it means. The original observation stays attached to every interpretation, including mixed languages, uncertainty, and later corrections. Speech and music are expected to be mostly non-English. English is the primary translation target.

That library is the home for internet radio and podcasts, a listening desk with a list, a globe, and a dial, rolling recordings, live translation, and topic monitoring. Music identification, wider text feeds, receive-only radio hardware, packets and telemetry, Morse, historical ciphers, and supplied-key cryptography follow on the same evidence model. A scanner, a spectrum view, or a cipher workbench is another instrument on that library. Curiosity belongs here: unfamiliar music, a practice signal, and a historical cipher are part of the product.

The first complete release is the listening desk: world radio, organized recordings, live translation, and bounded topic monitoring. The command line can do that work alone. The list explorer is another view of the same library, and the globe and map join it. A background service continues admitted work after the client exits. Local processing is the default, and paid processing stays off until a finite budget is set. This checkout is an early build of that desk. The [roadmap](ROADMAP.md) holds the rest of the product.

## List explorer

![Empty Sigy list explorer on Windows](docs/images/tui.png)

This is one frame of `sigy tui` on Windows, drawn by the Termina backend at 120 columns by 30 rows against an empty local catalog. Reduced motion and monochrome are on, and the frame count stays at zero. Without those flags, the same layout uses the terminal's own colors: cyan for the title and selection, green for a live connection and succeeded health, yellow for a favorite or work in progress, and red for a failure. The words stay. The globe and map are unavailable. Selecting a row does not start audio, capture, refresh, or a directory click. Quitting the explorer does not stop the service.

## Command line

![sigy --help on Windows](docs/images/cli.png)

The picture is the output of `sigy --help` on Windows. The [usage guide](docs/usage.md) is the command reference for this checkout: library, service, radio, podcasts, recording, playback, and agents. Planning documents describe later behavior. They are not a second set of commands.

## Install

Copy one command. It reads the installer from GitHub and builds the latest `main` commit. The repository is private, so GitHub CLI must already be logged in to an account that can read [blisspixel/sigy](https://github.com/blisspixel/sigy). This is not a release binary, an operating-system service, or `cargo verify`.

macOS and Linux:

```text
gh api -H "Accept: application/vnd.github.raw" https://api.github.com/repos/blisspixel/sigy/contents/scripts/install.sh | sh
```

Windows PowerShell:

```text
iex (gh api -H "Accept: application/vnd.github.raw" https://api.github.com/repos/blisspixel/sigy/contents/scripts/install.ps1)
```

The scripts are [install.sh](https://github.com/blisspixel/sigy/blob/main/scripts/install.sh) and [install.ps1](https://github.com/blisspixel/sigy/blob/main/scripts/install.ps1). If `cargo` is missing, the script installs Rust 1.98.1 for the current user from the official rustup installer, then builds `sigy` and puts that binary on the Cargo path. It does not install an operating-system service, download FFmpeg, or contact a station. From a checkout you already have, `./scripts/install.sh` or `scripts\install.ps1` installs that checkout instead of fetching `main`.

```text
sigy update --check
sigy update
```

`sigy update --check` prints the recorded commit and the latest `main` commit. It exits with an error when no commit is recorded or a newer commit is available. `sigy update` installs that commit. When GitHub CLI is logged in, Git uses that login. Neither command selects a library or contacts a station. On Windows the install finishes after `sigy update` exits, because Windows cannot replace the running executable. Stop a running service before that replacement. Recording and playback need a trusted FFmpeg that you install separately and pass to `dvr configure`.

```text
sigy --data-dir PATH_TO_LIBRARY library init
sigy --data-dir PATH_TO_LIBRARY tui
```

`PATH_TO_LIBRARY` is a private directory outside the checkout. Initialization creates a catalog with paid processing disabled.

## What this checkout does

A radio recording of at least 32 MiB seals a segment at 32 MiB or after 5000 ms of receive time and keeps the job running. That 5000 ms bound is the candidate uncommitted window, not a measured durability result. The whole byte budget stays one reservation. A disconnect, recovery, codec change, refused renewal, capture pause, or backward clock leaves a gap with that cause. Seeking inside the gap fails, and no silence file fills it. `listen file` plays one sealed segment while the capture continues and does not stitch those files into one timeline. `listen pause` does not stop that capture. `listen seek` stays inside a published segment. Return to live parks at the newest published end, and the open tail is visible but not readable. `schedule create` stores the next civil occurrence of one source. A missed window stays missed, and a late start leaves a prefix gap. `sigy doctor` checks the catalog, decoder, quota, and cache age without using the network. A stale station cache stays searchable, favorites stay, and the report names the refresh command to run. `radio policy set` saves one bounded directory page. The service refreshes that page when its interval has elapsed. Opening a client does not, and a failed refresh leaves the last cache. The tested path on Windows includes a radio directory cache, favorites, and an explicit click; finite recording and retention; retained-file playback; playlist resolution; one direct listen; one finite HLS recording; and podcast subscribe, RSS refresh, one enclosure download, one publisher transcript or chapter fetch, and playback of that retained file. `sigy mcp` exposes those commands to an agent for the library given at startup.

Windows x86_64 is the host those checks ran on. Linux and macOS are not a support matrix. Follow the [roadmap](ROADMAP.md#build-order) and the [progress record](docs/development/progress.md). Start from [intent](INTENT.md) when the question is what Sigy is for.

## Lawful use

Sigy is for listening, recording, and analysis of sources you are authorized to receive. The [usage guide](docs/usage.md#lawful-use) states that boundary. It is not legal advice.

## License

Copyright 2026 Nick Seal.

Sigy is licensed under the [Apache License, Version 2.0](LICENSE). Third-party components remain under their own licenses and notices.
