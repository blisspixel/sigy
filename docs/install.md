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

After installation, open a new shell and run `sigy --version`. PowerShell resolves `sigy.exe` through `PATH`. If the command is not found, add your actual installation root's `bin` directory to `PATH`; the default is `$HOME/.cargo/bin`, or `$env:USERPROFILE\.cargo\bin` on Windows. With a custom root, invoke its `bin/sigy` (`bin/sigy.exe` on Windows) explicitly when checking the version so an older PATH entry cannot select another installation. A failed fetch or build should be resolved before first use; a printed installer message alone is not a version check.

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

This sets the default 50 GB and 14-day managed-media limits explicitly. Sigy checks the decoder when recording and does not download it during setup. Windows retained system playback uses the bounded audio helper described in [the output decision](decisions/0084-bounded-windows-audio-output.md). Decoder completion and estimated presentation remain separate from acoustic audibility; available devices and profiles have limited qualification. Direct live and non-Windows output retain their existing paths. Follow the [command reference](usage.md) for listening, bounded recording, podcasts and monitoring. The [setup contract](decisions/0070-first-use-setup.md) describes defaults and retry behavior.

## Updating

Allow active recordings to finish, then stop the service and confirm it has exited before replacing the binary. Stopping the service interrupts active captures. Keep a separate backup of any library you care about; `record archive` protects managed retention but does not make a backup. Then run:

```text
sigy update --check
sigy update
sigy update --status
```

`--check` fetches and checks out the latest public `main` commit in the managed source tree, then compares it with the binary's embedded commit, falling back to the legacy per-user marker if absent. The legacy marker is not proof of a custom installation's identity. The command refuses a managed tree with local changes or a different configured origin. It exits with an error status when no installed commit is recorded or a newer commit is available, so read its report even if a shell shows a nonzero status.

`sigy update` prepares a separate checkout bound to the full fetched commit. On Windows it schedules a hidden helper after the current process exits. The helper reacquires an operating-system-owned installation lock, checks source identity before and after building into a private Cargo root, then replaces the executable with the validated staged artifact. Validation and build refusals leave the previous executable untouched. Replacement failures can leave it under a backup name; inspect the target and retained artifacts before retrying. A failure after replacement can leave the new executable installed with incomplete bookkeeping and is reported separately. Successful publication is recorded only after the published file's hash matches the staged file. Scheduling alone is not successful installation; `--json` reports that distinction. The [publication decision](decisions/0086-source-update-publication.md) records filesystem limits and acceptance evidence.

`sigy update --status` inspects the last bounded local receipt without fetching, building, resolving a library or contacting a service. It cannot be combined with `--check`. Add `--json` for a machine-readable receipt. Pending or running records do not establish helper liveness or completion after a crash. Failure reasons are fixed diagnostic codes rather than raw build logs. After a succeeded receipt, invoke the executable under its recorded `install_root` with `--version` before restarting your service; a bare command can select another installation on PATH. An update does not migrate an old running service.

The scripts honor `CARGO_HOME` and `CARGO_INSTALL_ROOT`, resolving relative paths before changing directories. PowerShell invocation restores its caller's location and temporary environment settings; the shell installer runs in a subshell. Windows updates use an explicit `CARGO_INSTALL_ROOT` when provided, otherwise the running executable's parent installation root when it is under `bin`, then Cargo's home. Cargo configuration-only custom roots are not inferred by the updater. Windows scripts and updates share the operating-system-owned lock, which releases on process death. A direct installer fences a prior pending or running helper under that lock before building; its old receipt records `installer-superseded`, even if the subsequent direct install fails. The old helper cannot later overwrite that installation. Independent POSIX script concurrency is not qualified. Non-Windows updates remain synchronous Cargo installs, with post-build source revalidation, rather than the Windows staged replacement contract.

Prepared work remains under `~/.sigy/update-work` for inspection, with a limit of 16 operation directories. If that limit is reached, inspect the receipt and preserve or remove completed work before retrying. Never delete a workspace while its helper may still be using it. No automatic retry, decoder download or diagnostic upload occurs. The [usage guide](usage.md#update) describes the command's interface.

## Current limits and help

The source installer and updater follow `main`, which can change before a tagged release. The Windows build and local media fixtures do not qualify macOS, Linux, every audio device, station, format, or long-running capture. There is no operating-system startup service. For a quick local preflight use `sigy --data-dir PATH doctor`; for command syntax use `sigy --help`. Report suspected vulnerabilities through the [security policy](../SECURITY.md). Use only material and signals you are authorized to access; see [lawful use](usage.md#lawful-use).
