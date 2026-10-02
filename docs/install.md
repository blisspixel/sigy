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
irm https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.ps1 -ErrorAction Stop | iex
```

macOS or Linux:

```sh
sh -c 's=$(curl --proto =https --tlsv1.2 -fsS https://raw.githubusercontent.com/blisspixel/sigy/main/scripts/install.sh) && sh -c "$s"'
```

Each command downloads the complete installer before executing it and stops on a failed download. The shell command also preserves the installer's exit status. Review the linked scripts to inspect what will run.

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

Two commands prepare your library and open the explorer:

```text
sigy init --radio
sigy tui
```

`init` creates the per-user library, starts the background service and prints the next steps. `--radio` explicitly requests one page of up to 100 stations from Radio Browser. It waits up to 15 seconds for the refresh; if it is still running, follow the printed status command before expecting stations in the explorer. A failed or interrupted refresh is reported as an error. Repeating setup preserves the library and reuses this first-use request without fetching twice. To request a fresh page later, use `sigy radio refresh NEW_ID --limit 100`.

The default library is `%USERPROFILE%\.sigy\library` on Windows and `$HOME/.sigy/library` on macOS or Linux. You no longer need to repeat a path for this library. For a different location, pass `--data-dir` to each command:

```text
sigy init --data-dir "PATH_TO_PRIVATE_LIBRARY" --radio
sigy tui --data-dir "PATH_TO_PRIVATE_LIBRARY"
```

Replace the placeholder with your path and keep quotes around paths containing spaces. For setup without a directory fetch, run `sigy init`. For local storage only, run `sigy init --no-start`; it never starts a service and cannot be combined with `--radio`. Starting a service on an existing library can resume its previously authorized schedules, processing and directory-refresh policies. Initialization preserves budgets, profiles, recordings and retention settings. A new library starts with paid processing disabled.

`sigy doctor` is a local preflight. Quitting the explorer leaves the service running; stop it explicitly with `sigy service stop`. These commands use the default library unless you pass `--data-dir`. The original `library init` and individual service commands remain available for automation.

Before recording, configure the absolute path of a trusted FFmpeg executable. Replace the placeholder below with its actual installed path:

```text
sigy dvr configure --decoder ABSOLUTE_PATH_TO_FFMPEG --quota-gb 50 --retention-days 14
```

This sets the default 50 GB and 14-day managed-media limits explicitly. Sigy checks the decoder when recording and does not download it during setup. `--destination null` is the tested playback path; system audio output depends on the installed FFmpeg build. Follow the [command reference](usage.md) for listening, bounded recording, podcasts and monitoring. The [setup contract](decisions/0070-first-use-setup.md) describes defaults and retry behavior.

## Updating

Allow active recordings to finish, then stop the service and confirm it has exited before replacing the binary. Stopping the service interrupts active captures. Keep a separate backup of any library you care about; `record archive` protects managed retention but does not make a backup. Then run:

```text
sigy update --check
sigy update
```

`--check` fetches and checks out the latest public `main` commit in the managed source tree, then compares it with the installed commit without installing. It refuses a managed tree with local changes or a different configured origin. It exits with an error status when no installed commit is recorded or a newer commit is available, so read its report even if a shell shows a nonzero status. `sigy update` builds that commit. On Windows it starts a helper that installs after the current `sigy` process exits; wait for that helper to finish before restarting the service or checking the new version. An update does not migrate an old running service. Restart the service with the new binary after installation. The [usage guide](usage.md#update) describes the command's exact behavior.

## Current limits and help

The source installer and updater follow `main`, which can change before a tagged release. The Windows build and local media fixtures do not qualify macOS, Linux, every audio device, station, format, or long-running capture. There is no operating-system startup service. For a quick local preflight use `sigy --data-dir PATH doctor`; for command syntax use `sigy --help`. Report suspected vulnerabilities through the [security policy](../SECURITY.md). Use only material and signals you are authorized to access; see [lawful use](usage.md#lawful-use).
