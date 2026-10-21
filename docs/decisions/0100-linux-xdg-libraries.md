# 0100: Linux XDG library locations

Date: 2026-10-21

Status: implemented. The path rules and the restore-on-failure move are tested on Windows. The Linux home-directory integration fixture is compiled only on Linux. This does not qualify a Linux install, a platform, or a release stage. Schema v51 and local IPC v53 are unchanged.

## Decision

Issue 32 keeps an existing Linux `~/.sigy/library` when no XDG library exists. `sigy doctor` adds one attention check, `library location`, whose next command is `sigy init --move-xdg`. That note appears only for the resolved default library, and only when the user did not pass `--data-dir`. A strict doctor treats the attention check as a failure. There is no automatic move.

Windows and macOS stay on `~/.sigy/library`. The Linux choice uses `cfg(target_os = "linux")`, so other Unix hosts do not follow it.

A new Linux library is `$XDG_DATA_HOME/sigy/library`, or `~/.local/share/sigy/library` when that variable is unset, empty, or relative. Update state (`update-status.json` and `installed-commit`) is `$XDG_STATE_HOME/sigy` or `~/.local/state/sigy`. The source checkout is `$XDG_CACHE_HOME/sigy/src` or `~/.cache/sigy/src`. State and cache are per user, not per `--data-dir`. While the default library is still the legacy directory, state and cache stay under `~/.sigy`. If both libraries exist, the XDG library is the default. Outside legacy mode, a state file or the source checkout is read from `~/.sigy` when that path exists and the XDG path does not.

The control socket is `$XDG_RUNTIME_DIR/sigy/control.sock` only when that runtime directory is absolute and already exists, and only for the resolved default library. Every other library keeps `control.sock` beside the catalog. The runtime directory itself is not created and its mode is not changed. The `sigy` subdirectory is created at mode 0700 when the service listens, and a missing subdirectory is not created on connect. There is no silent fallback to the library socket after that create fails. The library lock stays in the library. IPC does not gain a socket path. `XDG_RUNTIME_DIR` has no default.

`--data-dir` selects the library and does not relocate state or cache. `sigy init --move-xdg` refuses `--data-dir`, a non-Linux host, a missing legacy library, an existing XDG library, a held library lock, an existing destination state or cache path, and a cross-volume rename. The move is a rename. Files already renamed are moved back if a later step fails. The command does not start the service or reset policy, budgets, or recordings.

The shell installer uses the same library, state, and cache choice. Windows `install.ps1` stays on `~/.sigy`.

This supersedes the Linux default path in [first-use setup](0070-first-use-setup.md). MCP still requires an explicit `--data-dir`.

## Evidence

Active work records the checks that ran. Hosted verify is the Linux execution of the move fixture. Full local `cargo verify` and coverage were not re-run for this slice.
