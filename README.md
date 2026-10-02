# Sigy

**Practical signals intelligence for everyday people.** Explore internet radio and podcasts, record what matters, and trace findings back to their sources. Sigy brings station discovery, a private recording library, local speech recognition, English translation and bounded topic monitoring into one terminal toolkit. A background service keeps working when you close the terminal.

**Windows development preview.** Recognition and translation are experimental. See [current capabilities and limits](docs/development/progress.md) and the [roadmap](ROADMAP.md#whats-next).

![Sigy's radio explorer with an offline world map and selected station](docs/images/tui.png)

The current interface rendered with one real cached station page. Map positions come from directory metadata; selecting a station does not start playback or recording. [Explore the interface](docs/usage.md#list-explorer).

## Install

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.ps1 -ErrorAction Stop | iex
```

macOS or Linux:

```sh
sh -c 's=$(curl --proto =https --tlsv1.2 -fsS https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.sh) && sh -c "$s"'
```

These commands build `main` from source; [installation details](docs/install.md) cover Git, the pinned Rust toolchain, FFmpeg and platform limits.

## What you can do

- **Explore world radio.** Search stations by name, country, language or tag, save favorites, browse a globe or map, and listen to radio or podcast episodes.
- **Keep a useful record.** Capture streams in the background, schedule recordings across time zones, and retain selected intervals. Recording timelines distinguish audio, gaps and expired material.
- **Read across languages.** Run local recognition and translation on retained audio, keep original scripts beside English text, and correct a transcript without erasing its history.
- **Follow a topic with evidence.** Set source and resource bounds for monitoring. Explicit finite tasks can collect up to two recordings and publish cited findings and a briefing. Selected operations are also available through the [agent plugin](docs/decisions/0021-agent-plugin.md).

## Get started

Create your local library, start the background service and load a first station page:

```text
sigy init --radio
sigy tui
```

Sigy uses `~/.sigy/library` by default. Use `sigy init` for setup without a directory fetch, or `sigy init --no-start` to prepare storage without starting a service. See [first use](docs/install.md#first-use) for custom libraries and recording setup, and the [usage guide](docs/usage.md) for full commands.

Your library and local processing stay on your machine by default. Paid model dispatch and general task orchestration are not implemented. The [progress record](docs/development/progress.md#tracked-intermittent-test-issues) keeps current limitations visible, including a Windows client shutdown failure that has a structural repair but no proven cause.

## Learn more

- [Installation and updates](docs/install.md) and [command reference](docs/usage.md)
- [Current progress and verification evidence](docs/development/progress.md)
- [Product intent](INTENT.md) and [what comes next](ROADMAP.md#whats-next)
- [Architecture, design and research](docs/README.md)

## Use responsibly

Laws about receiving, recording, decoding, storing and sharing signals and broadcasts vary by country and region, and they can differ for internet streams, radio frequencies, encrypted traffic and personal data. You are solely responsible for knowing and following every law, regulation, license condition and terms of service that applies to you, your equipment, your location and your use. Only access sources you are permitted to access, keep hardware receive-only unless you hold the required authorization, and respect privacy and copyright. That a signal can be received does not mean you may record, decode, publish or reuse it. The [usage guide](docs/usage.md#lawful-use) has more detail.

Transcriptions, translations and findings are machine output and can be wrong. Check anything important against the original.

Sigy is provided "as is", without warranty of any kind. The authors and contributors are not liable for any claim, damage or other liability arising from its use or misuse, as set out in the license. Nothing in this project is legal advice.

## License

Copyright 2026 Nick Seal. Sigy is licensed under the [Apache License, Version 2.0](LICENSE), including its warranty disclaimer and limitation of liability. Third-party software and content retain their own licenses and notices. See [contributing](CONTRIBUTING.md) and [security reporting](SECURITY.md).
