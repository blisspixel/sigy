# 0121: Listen screen

Date: 2026-11-03

Status: implemented. Listen shows now playing, cached favorites, and stations this client started. Enter or `P` then `y` uses the existing play confirmation. Catalog schema stays v54. Local IPC stays v58. Issue 46's acceptance checklist is met. Issues 38, 40, and 45 stay open. No stage exit is claimed.

## Decision

Opening Listen requests one cached favorites page through the existing ordered search and starts no playback. The Find stations page, its cursor, and the applied filter stay. Pressing Listen again while it is already open reads nothing further. When the explorer is disconnected, that read is not submitted.

The body shows the active listen, then favorites, then stations started in this client. Favorites come from that one cache read. Recent stations are memory in this client. They survive switching screens. A new client starts with none. This screen writes no recent list to the catalog.

Enter on a Listen row opens the same station play confirmation as `P`. `y` starts that play. An empty Listen list leaves the Find stations row alone and plays nothing. `n` or Esc cancels. A cancelled play is not remembered.

The footer lists play, stop, and favorite. `H` pause and `[` `]` seek are listed only while a retained listen is active. Those keys dispatch the existing retained effects. `S` stops the retained listen when one is active, and stops the live listen otherwise.

When nothing is playing and no favorite is cached, the body says: `Find stations (1) or surprise me (e). f on any station saves it here.` `e` submits the existing random read and opens Globe when that read is submitted.

This is the Listen screen [Decision 0120](0120-action-menu-and-confirmations.md) left for later.

Schema stays v54. No migration. Local IPC stays v58.

## What this does not do

One recordings row per recording, the empty-cache welcome, the `:` palette, Jobs, and further globe browsing stay later. This slice does not change playback admission, recording publication, or HTTP acquisition. It does not change the catalog or spending.

## Interface

At 80 by 24 the gallery shows Listen playing, idle with favorites, and empty. Playing names the station, says live, the state, and the elapsed clock, and puts `S stop` on that line. The next line shows the session id and the source revision. Idle says `Nothing playing.` and lists the cached favorites. Empty uses the sentence above. The footer lists `P play`, `S stop`, and `f favorite`. `H pause` stays off the footer until a retained listen is active.

## Evidence

Warnings-denied Clippy for `sigy` exited 0. The `sigy` binary tests passed 428, with 5 ignored, in 14.17 seconds. The retired-synonym scan was compiled again after the usage and README sentences and passed. `cargo visual-qa` exited 0. The compile finished in 2.82 seconds. The gallery test finished in 3.79 seconds and wrote 89 synthetic frames, including Listen playing, idle with favorites, and empty at 80 by 24. The command wall time was 6.68 seconds. Full `cargo verify` was not re-run. Coverage was not run. `cargo verify-media` was not run. This slice does not change playback admission, recording publication, or HTTP acquisition. External spend is USD 0. This receipt was written after those commands. Hosted verify of `a72b402` passed on Windows, Ubuntu, and macOS: https://github.com/blisspixel/sigy/actions/runs/41715446017.

## Limits

Issues 38, 40, and 45 stay open. Issues 41, 43, 44, and 47 through 49 stay open. Issue 46's checklist is met. Native key-to-frame timing remains unmeasured. The gallery is synthetic offline frames and is not native terminal fidelity. The Linux service-library failures recorded for the Home increment are this host's missing delegated unit. This slice does not change that crate. No acoustic, Omarchy, language, or platform qualification is claimed. No stage exit is claimed.
