# 0102: Terminal retained playback

Date: 2026-10-21

Status: implemented terminal action for one whole retained recording and one cited finding. [Decision 0103](0103-decoder-timeline.md) reports decoder output time during that listen. Pause, seek, a range editor, segment handoff, acoustic output, and a stage exit are not claimed. Schema v51 and local IPC v53 are unchanged.

## Decision

[Decision 0087](0087-exact-retained-excerpts.md) already admits a CLI range and a cited finding. This increment does not add a second admission path. The terminal action is separate from Enter, selection, filters, reload, and Back.

In Recordings, `P` then `y` plays one whole sealed object only when its storage state is `retained` and capture is not active. The child is `listen file RECORDING --request NEW --destination system --cancel-on-stdin --parent-pid PID`. There is no terminal range editor. In Findings, `P` then `y` plays only when the loaded page stores a non-empty interval, through `listen finding MONITOR FINDING` with the same destination and fences. A missing interval is noted and starts nothing. Enter still inspects. `o` still reads recording metadata. `n` or Escape discards the confirmation and contacts nothing.

The explorer allocates the request id before the child starts and shows it. Reuse of an id on the CLI remains inspect-without-audio. One foreground client covers live play and retained listen. A second deliberate play closes the current child's stdin, requests stop when the listen id or reader generation is known, and does not start the next decoder until that child has exited and its diagnostic drain has finished. Child exit is not an independent native-closure receipt. Original-reader protection follows service completion. Browsing does not replace audio.

The retained row is omitted while no retained listen exists. While one exists, the row is plain text: pending, decoding, stopping, closed, or unresolved, plus the request id. It does not replace the listen-session row. Admission, show, and stop run off the terminal thread. A late receipt whose request id is not the visible one is ignored.

Quit and an external stop request stop, leave capture running, and after the terminal is restored print one stderr line only when a client was active or a next listen was queued. `listen file` and `listen finding` accept `--cancel-on-stdin` and `--parent-pid`. Without those flags, playback keeps its blocking path. Cancel is polled by the future that owns the native group. Dropping the decoder future on the existing deadline or Ctrl-C path is unchanged.

The desktop desk does not gain Play or Stop. No MCP mutation is added. Linux system excerpts still refuse, and the terminal shows that failure. Up to four unresolved readers can remain. The status line says a previous reader may still hold protection. S-03B stays open. No acoustic, Omarchy, language, or platform qualification is claimed.

## Evidence

`cargo verify` on this Windows host passed vendor and script checks, the changelog heading check, formatting, warnings-denied Clippy, workspace tests, the build, and `cargo audit --deny warnings` against 1348 advisories and 382 dependencies. The `sigy` binary tests passed 360, with 7 ignored, in 25.00 seconds. The `sigy-service` library tests passed 734, with 2 ignored, in 175.60 seconds. The command completed in 274.84 seconds.

`cargo verify-media` then passed 20 ignored service fixtures in 115.31 seconds and the direct live PCM sample check. It used `ffmpeg` on `PATH` at `N-113513-gaa3cfd4b5a-20240202`. The command completed in 167.81 seconds. The first media run failed `contained_recognition_publishes_bounded_text_and_fails_closed_on_faults` because a pending `task evidence` result exits 5. The fixture now accepts that class while it waits, and requires exit 0 once the outcome is complete. The rerun passed.

Coverage was not run. `cargo visual-qa` was not re-run. No acoustic, Omarchy, language, or platform qualification is claimed. External spend remains USD 0. Hosted verify of `bd2a679` passed on Windows, Ubuntu, and macOS: https://github.com/blisspixel/sigy/actions/runs/39629841546. A clean detached worktree install wrote `installed-commit` `bd2a679f5d60ccab4eca87b1ac22787e455d37ae`.
