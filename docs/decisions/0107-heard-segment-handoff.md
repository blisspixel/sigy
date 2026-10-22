# 0107: Heard segment handoff

Date: 2026-10-22

Status: implemented handoff from one completed sealed segment to the next. Prefetch, latest-sealed follow, a range editor, codec-transition detection, device presentation, acoustic output, and a stage exit are not claimed. Schema v52 and local IPC v54 are unchanged.

## Decision

[Decision 0106](0106-heard-seek.md) moves one admitted listen by ten seconds. This increment starts the next sealed segment after that listen's reader completes. It does not stitch two segments into one reader, and it does not connect the metadata playhead to the decoder.

A whole-segment retained listen whose service receipt is `completed`, whose child has exited, and which stored a decoder sample starts one new `listen file` at that segment's exclusive end. The end is the receipt's object timeline end. The terminal timeline projection is not a second authority. The new listen passes `--seek-us` and no `--end-us`, uses a new request id, and does not carry a finding citation.

A cited finding, or any other excerpt, does not continue past its range. A child that is still running does not prefetch the next segment. A station listen that still occupies the foreground client, a stop that is still in progress, and a second attempt while a child is stopping do not start another decoder. A missing decoder sample, a failed receipt, a stale visible request, and an end that would not move forward do not start one either.

The service still admits or refuses the next position. A gap, the open tail, expiry, or a missing segment writes no reader. Exit with no decoder sample keeps the position at the completed segment's end. `H` can start one listen there. Capture is not paused. Quit still leaves the recording running. The desktop desk does not gain Play or Stop. No MCP mutation is added. `listen seek` remains the metadata playhead.

A different format is a new admission and a new decoder, not a stitched transition. This slice does not detect codec changes. Latest-sealed follow is not included. Neither result is an acoustic or platform qualification.

## Evidence

`cargo verify` exited 0 on this Windows host. It passed vendor and script checks, the changelog heading check, formatting, warnings-denied Clippy, workspace tests, the build, and `cargo audit --deny warnings` against 1349 advisories and 382 dependencies. The `sigy` binary tests passed 381, with 7 ignored, in 25.88 seconds. The `sigy-service` library tests passed 744, with 2 ignored, in 195.19 seconds. The command completed in 340.83 seconds. `cargo verify-media` then passed 20 ignored service fixtures in 112.51 seconds and the direct live PCM sample check in 0.07 seconds. The command completed in 121.99 seconds. `ffmpeg -version` reports `N-113513-gaa3cfd4b5a-20240202`. Coverage was not run. `cargo visual-qa` was not run. External spend remains USD 0. Headphones were not checked. This receipt was written after those commands.

A gap, the open tail, expiry, and a missing next segment still write no reader. Those checks already exist: `gap_cause_refusals_preserve_original_wording_without_admitting_readers`, `prefix_gap_refuses_zero_and_freezes_nonzero_timeline_without_inventing_audio`, `open_tail_requires_actual_running_open_reservation_and_creates_no_reader`, and `excerpt_checks_interior_gaps_and_accepts_gap_adjacent_exclusive_edges`. This slice adds no new gap fixture. Issues 13 and 14 stay open. S-03B stays open. No stage exit is claimed.
