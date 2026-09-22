# Contributing

Read [README.md](README.md), [ROADMAP.md](ROADMAP.md), [AGENTS.md](AGENTS.md), and [current progress](docs/development/progress.md) before changing behavior. Keep changes focused and distinguish implemented behavior from future plans.

Use the pinned Rust toolchain and preserve `Cargo.lock`. Install `cargo-audit` 0.22.2, then run `cargo verify` from the repository root. Recording, playback, acquisition, and retention changes also need `cargo verify-media` with a trusted installed FFmpeg. Report the platform and checks actually exercised. Current test evidence is from Windows; other platforms remain unqualified.

Use [issues](https://github.com/blisspixel/sigy/issues) for reproducible bugs, proposed changes, and usage questions. Include the commit, platform, commands, expected result, and actual result. Remove credentials, private URLs, personal paths, and captured media from reports. Follow [SECURITY.md](SECURITY.md) for vulnerabilities.

Preserve third-party copyright and license notices. Explain the resulting behavior and validation in pull requests. Do not add attribution trailers or tool credits; the repository's writing and attribution policy is in AGENTS.md.
