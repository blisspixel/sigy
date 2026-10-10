# 0088: Omarchy and desktop panel integration

Date: 2026-10-10. Status: implemented increment; verification and limitations belong in [active work](../development/progress.md).

## Context and intent

Desktop environments such as Omarchy (an agentic Arch Linux distribution utilizing Hyprland, Waybar, and developer-focused tooling) require lightweight, reliable status bars and quick-launch widgets. Integrating Sigy into these environments requires exposing ambient signal intelligence—service status, directory freshness, active recordings, and current playback—in standard formats like Waybar JSON, as well as enabling bounded, one-click playback of favorite stations without requiring full TUI initialization.

Desktop integrations must preserve Sigy's core invariants:
1. Operations remain local-first with zero unapproved network contact.
2. Status inspections remain strictly read-only and truthful about platform and language qualification boundaries.
3. Playback processes launched by desktop bars must clean up promptly if the desktop shell, launcher, or input pipe closes.

## Desktop projections: status and bar

The `panel status` and `panel bar` commands provide read-only projections of the local library and running service:

- **Service presence**: Distinguishes between `running`, `stopping`, and `absent`. If the background service is absent, status commands read directly from the local SQLite catalog without starting background daemons.
- **Directory freshness**: Evaluates station cache state as `empty` (0 cached stations), `stale` (any stations older than 24 hours), or `current`.
- **Bounded summaries**: Lists up to four favorite stations and up to four recent recordings, marking truncated pages when additional items exist.
- **Waybar integration**: `panel bar` outputs a single JSON object containing `text` (status class), `class` (CSS styling class), and `tooltip` (plain-text summary). Status classes follow a deterministic priority ladder:
  1. `offline`: service absent or stopping.
  2. `failed`: active listen failed or interrupted.
  3. `playing`: active listen running.
  4. `recording`: one or more active stream captures running.
  5. `stale`: directory empty or stations older than 24 hours.
  6. `idle`: service active and nominal.

### Truthful qualification boundaries

In accordance with product principles, status outputs explicitly declare qualification limits:
- `"qualified_platform": false`
- `"qualified_linux": false`
- `"qualified_omarchy": false`
- `"read_only": true`
- Language note: "Recognition and translation are not part of this panel. No language is qualified."
- Acoustic delivery note: "Device follow is none. The next explicit play uses the current default output. Acoustic delivery is not qualified."

## Bounded playback and session hint

`panel play STATION` enables single-station playback for favorites:
- **Admission**: Requires that the station is present in the local directory cache, marked as a favorite, not an HLS playlist, and possesses exactly one registered `http_audio` source revision. Refusal messages provide actionable next steps.
- **Session hint**: The active listen ID, station UUID, revision ID, and state are recorded in `panel-session.json` within the library root, secured with `0600` permissions on Unix. Before launching, admission verifies that no prior panel listen remains in a `running` state.
- **Parent and pipe lifecycle bounds**:
  - `--cancel-on-stdin`: Monitors standard input. If standard input is closed before playback starts, the command fails immediately with `STDIN_CLOSED`. If input closes during playback, playback cancels cleanly.
  - `--parent-pid PID`: Verifies that the launching process is alive before starting (`PARENT_GONE`) and monitors the parent PID during playback, terminating playback if the parent process exits.
- `panel stop [--id ID]`: Reads the active session hint (or accepts an explicit ID) and dispatches an orderly stop through the service. Upon completion, cancellation, or failure, `settle` updates `panel-session.json`.

## Interface and remaining work

The CLI exposes:
- `sigy panel status [--json]`
- `sigy panel bar`
- `sigy panel play STATION [--id ID] [--cancel-on-stdin] [--parent-pid PID] [--destination DEST]`
- `sigy panel stop [--id ID]`

Full Linux audio device routing, native Wayland notification daemons, interactive player controls, and formal platform qualification on Arch/Omarchy remain open future increments.
