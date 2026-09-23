# Install and update Sigy

Sigy is a development preview. The source installer has been exercised on Windows x86_64, including an isolated checkout install. Native macOS and Linux installs have not been qualified. There is no release binary or crates.io application package yet. Read the [current evidence](development/progress.md) before relying on a capability.

## Prerequisites

- Git and a working Rust build environment. The installer uses the repository's pinned Rust 1.98.1 toolchain. If Cargo is absent, it installs Rust for the current user through the official rustup installer. Windows native dependencies may require the [Microsoft C++ build tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
- PowerShell on Windows, or a POSIX shell and `curl` on macOS or Linux.
- Enough free space for Rust dependencies and a source build. Recordings need additional space in a private library outside the checkout.
- A trusted local FFmpeg executable for recording and playback. Sigy does not download FFmpeg. The current decoded-format evidence is one Windows run with FFmpeg 9.0.1, not a platform support matrix.

Review the [PowerShell installer](../scripts/install.ps1) or [shell installer](../scripts/install.sh) before running a remote script. When run remotely, they obtain public `main` from [the repository](https://github.com/blisspixel/sigy). They build `crates/sigy` with `cargo install --locked --force` and record a clean checkout's commit under `~/.sigy/installed-commit`. They install a user-level command, not an operating-system service. A source build can take several minutes. The first shell may need to be reopened for Cargo's binary directory to appear on `PATH`.

## Install from GitHub

Windows PowerShell:

```powershell
& { $ErrorActionPreference = 'Stop'; iex (Invoke-RestMethod -Uri https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.ps1) }
```

macOS or Linux:

```sh
sh -c 'f=$(mktemp) || exit; curl --proto "=https" --tlsv1.2 -fsS https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.sh -o "$f" && sh "$f"; s=$?; rm -f "$f"; exit "$s"'
```

After installation, open a new shell and run `sigy --version`. On Windows, type `sigy`; PowerShell resolves the installed `sigy.exe` through `PATH`. If the command is not found, add `$HOME/.cargo/bin` to `PATH`. On Windows the equivalent directory is `$env:USERPROFILE\.cargo\bin`. A failed fetch or build should be resolved before first use; a printed installer message alone is not a version check.

## Install from a checkout

The checked-in scripts install that checkout without fetching a different source tree. Run the matching command from the repository root:

```powershell
./scripts/install.ps1
```

```sh
sh ./scripts/install.sh
```

The build embeds the checkout's commit only when its working tree is clean. Installing a checkout with local edits clears the installed commit marker, so the binary does not claim the source commit. Set `SIGY_SRC` only if you intentionally want the scripts to fetch and update a separate checkout. Managed source trees must have the repository's configured origin and no local changes; the installer refuses them otherwise, without discarding edits.

## First use

Pick a private library path outside the repository. On Windows:

```powershell
$library = Join-Path $env:USERPROFILE '.sigy\library'
sigy --data-dir $library library init
sigy --data-dir $library doctor
sigy --data-dir $library service start
sigy --data-dir $library service status
sigy --data-dir $library radio refresh first-page --limit 100
sigy --data-dir $library radio refresh-status first-page
```

Repeat `refresh-status` until it reports completion, then open the explorer:

```powershell
sigy --data-dir $library tui
```

On macOS or Linux, set `library="$HOME/.sigy/library"` and use `sigy --data-dir "$library"` with the same subcommands. The initial library has paid processing disabled. `doctor` checks local state without contacting a station or changing the catalog. The refresh fetches one bounded directory page; it does not contact station streams. Use a fresh request ID for another refresh. Quitting the explorer leaves the service running. Stop it explicitly with `sigy --data-dir PATH service stop`.

Before recording, configure the absolute path of a trusted FFmpeg executable. For example, replace the placeholder below with its actual installed path:

```text
sigy --data-dir PATH dvr configure --decoder ABSOLUTE_PATH_TO_FFMPEG --quota-gb 50 --retention-days 14
```

This command sets the default 50 GB and 14-day managed-media limits explicitly. Sigy checks the decoder when recording. `--destination null` is the tested playback path; system audio output depends on the installed FFmpeg build. Follow the [command reference](usage.md) for source registration, bounded recording, retained-file playback, podcast feeds, and the current command limits.

## Updating

Allow active recordings to finish, then stop the service and confirm it has exited before replacing the binary. Stopping the service interrupts active captures. Keep a separate backup of any library you care about; `record archive` protects managed retention but does not make a backup. Then run:

```text
sigy update --check
sigy update
```

`--check` fetches and checks out the latest public `main` commit in the managed source tree, then compares it with the installed commit without installing. It refuses a managed tree with local changes or a different configured origin. It exits with an error status when no installed commit is recorded or a newer commit is available, so read its report even if a shell shows a nonzero status. `sigy update` builds that commit. On Windows it starts a helper that installs after the current `sigy` process exits; wait for that helper to finish before restarting the service or checking the new version. An update does not migrate an old running service. Restart the service with the new binary after installation. The [usage guide](usage.md#update) describes the command's exact behavior.

## Current limits and help

The source installer and updater follow `main`, which can change before a tagged release. The Windows build and local media fixtures do not qualify macOS, Linux, every audio device, station, format, or long-running capture. There is no operating-system startup service. For a quick local preflight use `sigy --data-dir PATH doctor`; for command syntax use `sigy --help`. Report suspected vulnerabilities through the [security policy](../SECURITY.md). Use only material and signals you are authorized to access; see [lawful use](usage.md#lawful-use).
