# 0120: Action menu and confirmation dialogs

Date: 2026-11-03

Status: implemented. Space opens an action menu for the selection on Find stations, Globe, Recordings, and Findings. At 40 columns by 12 rows or larger, a confirmation names its target in a dialog. Catalog schema stays v54. Local IPC stays v58. Issue 42's acceptance checklist is met. Issues 38, 40, and 45 stay open. No stage exit is claimed.

## Decision

Space opens the menu when the terminal is at least 40 columns by 12 rows and the workspace is Find stations, Globe, Recordings, or Findings. Home, Listen, Watch topics, and System have no menu. Text entry, the key overlay, and an open claim page do not open one. The rows come from that workspace's action table, in table order. Each row shows its direct key, such as `P Play` or `Enter Inspect`. Copy CLI command has no key. A row that cannot run stays listed with the same refusal the direct key would show.

Esc or Backspace closes the menu and leaves the status and the selection unchanged. Up and Down move only the menu row. Enter runs the highlighted row. That row's own key runs it when another row is highlighted. Other keys are swallowed while the menu is open. `q` still quits the client.

Play, fetch, hold, a sealed interval, and the latest sealed interval still need `y`. Enter and Backspace neither confirm nor cancel a dialog. `n` or Esc still cancels, and the status sentence stays the existing words. The dialog is display. It names the target, says what the action contacts or stores, and shows `y Play   n Cancel`, `y Fetch   n Cancel`, or `y Hold   n Cancel`.

Favorite, inspect, center, linked history, and metadata dispatch as the direct key does. Copy writes the substituted command on the status line and starts nothing. Latest uses the newest playable sealed interval. An open tail under the cursor does not change that choice. Interval and hold use the cursor, so an open tail stays refused.

Below 40 by 12 the popup is not drawn. Space writes one action line on the status row. A menu that no longer fits collapses to that line, and the next Esc stays on the workspace. When the window fits again, the menu returns.

Linear mode draws plain lines and no box. The selected menu row is reversed in every mode, including monochrome. The workspace underneath stays drawn, and the status row stays visible. The hold dialog strips control characters from the displayed recording id. The id submitted with `y` is the stored id.

This supersedes the later-work note for the Space menu and named confirmations in [Decision 0119](0119-home-and-lean-chrome.md). The direct keys and confirm-before-contact behavior stay as they were.

Schema stays v54. No migration. Local IPC stays v58.

## What this does not do

One Listen screen, one recordings row per recording, the `:` palette, Jobs, and further globe browsing stay later. One combined connection line, a hint on the last row, and a globe path back to the previous station list stay later. This slice does not change playback admission, recording publication, or HTTP acquisition. It does not change the catalog or spending.

## Interface

At 80 by 24 the gallery shows the Find stations action menu and the play dialog. The menu lists Fetch, Favorite, Play, Inspect, Linked history, and Copy. The play dialog names the station, says nothing is recorded, and shows `y Play   n Cancel`. The status line still says `y confirms`. In the filter editor, Space still toggles a yes/no field.

## Evidence

Warnings-denied Clippy for `sigy` exited 0. The `sigy` binary tests passed 417, with 5 ignored, in 13.96 seconds. `cargo visual-qa` exited 0. The gallery test finished in 3.81 seconds and wrote 86 synthetic frames, including the action menu and the play dialog at 80 by 24. The command wall time, including compile, was 10.10 seconds. Full `cargo verify` was not re-run. Coverage was not run. `cargo verify-media` was not run. This slice does not change playback admission, recording publication, or HTTP acquisition. External spend is USD 0. This receipt was written after those commands. Hosted verify of `0bb7e39` passed on Windows, Ubuntu, and macOS: https://github.com/blisspixel/sigy/actions/runs/41702837146.

## Limits

Issues 38, 40, and 45 stay open. Issues 41, 43, 44, and 46 through 49 stay open. Native key-to-frame timing remains unmeasured. The gallery is synthetic offline frames and is not native terminal fidelity. The Linux service-library failures recorded for the Home increment are this host's missing delegated unit. This slice does not change that crate. No acoustic, Omarchy, language, or platform qualification is claimed. No stage exit is claimed.
