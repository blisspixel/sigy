# 0122: Recordings list

Date: 2026-11-03

Status: implemented. Recordings opens on a list. Enter opens the timeline. Catalog schema stays v54. Local IPC stays v58. Issue 47's acceptance checklist is met. Issues 38, 40, and 45 stay open. No stage exit is claimed.

## Decision

Recordings opens on one row per recording. Arrows select. Enter opens that recording's timeline. Esc or Backspace returns to the list and keeps the selection. Pressing Recordings again, including digit `3`, returns to the list. A cited recording opened from Findings starts on its timeline. An empty page stays on the list and uses the existing empty sentence. Reloading keeps the open timeline and the chosen interval when that recording is still on the page. An empty reload returns to the list.

The row shows the source revision id already carried on the recording page. When that revision is empty, the row shows the recording id. It shows `REC` while the stored state is scheduled, starting, running, retrying, or stopping, and the stored state word otherwise. Length is the existing timeline clock, `m:ss` or `h:mm:ss`. A truncated timeline says `unavailable` and does not invent a clock. The recording page has no source label and no created time, so the row has neither. The exact microsecond range stays on the timeline.

The timeline keeps the human ruler, the legend, the published, gap, and unpublished summary, and the exact microsecond range. Linear and monochrome keep the plain glyphs `#`, `x`, `!`, `.`, and `+`. Linear mode marks the timeline body. The identity line stays unlabeled by that marker.

Space lists Play, Latest, Hold, Pause, Seek, Interval, and Copy. Each direct key still works, and the menu uses the same confirmation or refusal. Enter on Seek seeks backward, the same as `[`. `]` remains the direct forward key. The Seek row names both keys. Pause is `H`. A blocked row shows the refusal the direct key would show.

This is the recordings list [Decision 0121](0121-listen-screen.md) left for later. The action rows extend [Decision 0120](0120-action-menu-and-confirmations.md).

Schema stays v54. No migration. Local IPC stays v58. The recording page did not gain a source label or a created time.

## What this does not do

Source labels and ages stay off the list until a later page carries them. The empty-cache welcome, the `:` palette, Jobs, and further globe browsing stay later. This slice does not change playback admission, recording publication, or HTTP acquisition. It does not change the catalog or spending.

## Interface

At 80 by 24 the gallery shows the recordings list, the timeline, the menu, and the plain linear and monochrome timelines. The list shows at least 12 rows. No list row shows a microsecond range. The timeline shows that range. Linear and monochrome frames use the plain glyphs.

## Evidence

Warnings-denied Clippy for `sigy` exited 0. The `sigy` binary tests passed 448, with 5 ignored, in 14.08 seconds. `cargo visual-qa` exited 0. The compile finished in 1.73 seconds. The gallery test finished in 4.11 seconds and wrote 94 synthetic frames, including the recordings list, timeline, menu, and plain linear and monochrome timelines at 80 by 24. The command wall time was 5.91 seconds. Full `cargo verify` was not re-run. Coverage was not run. `cargo verify-media` was not run. This slice does not change playback admission, recording publication, or HTTP acquisition. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

Issues 38, 40, and 45 stay open. Issues 41, 43, 44, 48, and 49 stay open. Issue 47's checklist is met. Native key-to-frame timing remains unmeasured. The gallery is synthetic offline frames and is not native terminal fidelity. The Linux service-library failures recorded for the Home increment are this host's missing delegated unit. This slice does not change that crate. No acoustic, Omarchy, language, or platform qualification is claimed. No stage exit is claimed.
