# Contributing

Read [README.md](README.md), [ROADMAP.md](ROADMAP.md), [AGENTS.md](AGENTS.md), and [current progress](docs/development/progress.md) before changing behavior. Keep changes focused and distinguish implemented behavior from future plans.

Use the pinned Rust toolchain and preserve `Cargo.lock`. Install `cargo-audit` 0.22.2, then run `cargo verify` from the repository root. Recording, playback, acquisition, and retention changes also need `cargo verify-media` with a trusted installed FFmpeg. Report the platform and checks actually exercised. Current test evidence is from Windows; other platforms remain unqualified.

Run the additional local `cargo verify-coverage` gate with `cargo-llvm-cov` 0.9.0, the pinned toolchain's `llvm-tools` component, and FFmpeg. It requires at least 80% line coverage in every workspace crate, includes ordinary and native fixtures, test sources and build scripts, and permits no source exclusions. Add tests of real contracts and failures when coverage falls short. See [verification](docs/usage.md#verification) for prerequisites and receipt locations.

For an isolated Rust research workspace, validate an existing LLVM JSON receipt with `cargo run --locked -p sigy-xtask -- verify-coverage-report MANIFEST_PATH LLVM_JSON_REPORT`. This read-only command derives workspace membership from locked Cargo metadata, checks owned report paths and line counts, and applies the same exact 80% threshold separately to each crate. It does not rerun tests or inference. Collect receipts with all targets and build scripts included and no source exclusions; a report alone cannot prove that the collection included every executable source or that the source files are unchanged since collection. Retain the collection command and source identity alongside the receipt.

Use [issues](https://github.com/blisspixel/sigy/issues) for reproducible bugs, proposed changes, and usage questions. Include the commit, platform, commands, expected result, and actual result. Remove credentials, private URLs, personal paths, and captured media from reports. Follow [SECURITY.md](SECURITY.md) for vulnerabilities.

Preserve third-party copyright and license notices. Explain the resulting behavior and validation in pull requests. Do not add attribution trailers or tool credits; the repository's writing and attribution policy is in AGENTS.md.
