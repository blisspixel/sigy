# 0124: Watch topics, Findings landing, and the Jobs screen

Date: 2026-11-08

Status: implemented. Watch topics opens on a list, then Coverage and Passages. Findings shows stored claims on the first open and a five-line summary. Digit 8 opens a read-only Jobs screen. Catalog schema stays v54. Local IPC stays v58. Issues 48 and 49 acceptance checklists are met. Issue 38 stays open because issues 43 and 44 stay open. Issues 13, 14, and 53 stay open. No stage exit is claimed.

## Decision

Watch topics (digit 4) opens on a list. A row names the topic. After that topic has been opened, the row also names the version, paused or active, and the passage count. Enter opens Coverage, one line per source. Tab switches to Passages. A passage row shows the original, English when present, and media time as `m:ss` or `h:mm:ss`. It does not show a microsecond range. Enter on a passage shows revisions and the exact half-open interval. `o` opens that passage's recording. Esc or Backspace leaves one level: the passage, then the topic, then the list. The screen reads the existing monitor show, coverage, and matches. Passages are the captures started in the last 24 hours. The screen changes no monitor and starts no capture or processing.

Findings (digit 5) reads the stored claims list on the first open. A second 5 while Findings is already open stays there. `/` still edits a named lookup. A loaded finding leads with five content lines: the finding, the transcript state, the cited interval as a clock range, the original, and English, plus a short hint. `d` shows the full text, the exact microsecond interval, and the boundary notes. `P` then `y` still plays the cited interval. The claims list and the summary do not show a microsecond range. Exact values stay on the detail view and in `--json`. Uncertainty stays on the detail view.

Jobs (digit 8) is read-only. Three sections list running captures, schedule rules, and tasks. Each page reads at most 16 rows through the existing list operations. `n` and `p` follow the service cursor and keep the rows already shown when a read fails. Before the first read the header says the snapshot is unread. After a read, disconnect keeps that snapshot and its age. Enter shows the same fields as `record show`, `schedule show`, or `task show`. Each list row names the managing `sigy` command, including `--data-dir` when the client was started with one. A schedule row shows the rule clock: the stored zone and the stored civil time, weekday, or recurrence. It does not compute a next occurrence, because the list has no next-fire instant. A task row shows the scope window and checkpoint from a bounded show of that page, at most 16 shows. A failed show says the scope or checkpoint is unread. No Jobs key starts, stops, or changes a capture, schedule, or task. `H`, `[`, and `]` are not listed on Jobs. They still pause and seek a retained listen on the other workspaces and on Home.

Home lists eight tasks. Digit 8 opens Jobs. After a Jobs read, the Home row counts schedules, tasks, and running captures.

The gallery is 104 synthetic frames. It adds the topic list, Coverage, Passages, the finding summary, and Jobs populated, empty, and disconnected, each at 80 by 24.

## What this does not do

This does not change catalog schema, migrations, local IPC, media acquisition, playback admission, or spending limits. It does not start a capture or a processing job. It does not invent a next schedule fire. It does not add the command palette (issue 43) or the first-run script route (issue 44). Issue 38 stays open.

## Interface

At 80 by 24, Watch topics reaches a passage by Enter, then Tab, then Enter. Findings shows the claims list before a lookup is typed. Jobs shows three sections and the managing command on each row. A narrow Findings pane puts the original or English script on its own line so the phrase stays whole.

## Evidence

Warnings-denied Clippy for `sigy` (`cargo clippy -p sigy --bin sigy --tests -- -D warnings`) exited 0. The check finished in 2.34 seconds. The `sigy` binary tests passed 461, with 5 ignored, in 14.25 seconds. `cargo visual-qa` exited 0. The compile finished in 5.96 seconds. The gallery test finished in 4.27 seconds and wrote 104 synthetic frames, including the topic list, Coverage, Passages, the finding summary, and Jobs populated, empty, and disconnected at 80 by 24. Full `cargo verify` was not re-run. Coverage was not run. `cargo verify-media` was not run. This slice does not change playback admission, recording publication, or HTTP acquisition. External spend is USD 0. This receipt was written after those commands. Hosted verify of this commit is not recorded here.

## Limits

Issues 13, 14, 38, 43, 44, and 53 stay open. Issues 48 and 49 checklists are met. Native key-to-frame timing remains unmeasured. The gallery is synthetic offline frames and is not native terminal fidelity. No acoustic, Omarchy, language, or platform qualification is claimed. No stage exit is claimed. No source prerelease is published from this increment: a tag needs local `cargo verify` and hosted verify of the heading commit, and local `cargo verify` was not re-run.
