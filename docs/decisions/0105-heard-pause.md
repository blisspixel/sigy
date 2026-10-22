# 0105: Heard pause on one retained listen

Date: 2026-10-22

Status: implemented terminal pause for one retained listen. Seek, a range editor, segment handoff, device presentation, acoustic output, and a stage exit are not claimed. Schema v52 and local IPC v54 are unchanged.

## Decision

[Decision 0103](0103-decoder-timeline.md) shows FFmpeg output time from the admitted start. That number is not the in-memory playback playhead. This increment does not connect those clocks. Capital `H` uses the decoder sample already on the retained row.

The first `H`, while that listen's child is running and a sample is stored, requests the existing retained stop and remembers one recording-timeline position. For a whole recording the position is the decoder sample, because that listen started at zero. For a cited finding the position is the cited start plus the decoder sample, and it must stay below the cited end. The next `H` starts exactly one `listen file` at that position. A whole-file resume passes `--seek-us` and no `--end-us`. A finding resume also passes the same cited `--end-us`. Both use a new request id. The new reader receipt is an uncited range: the finding citation is not copied onto it. The terminal still shows that listen.

No sample, an addition that overflows, or a position at or past the cited end does not stop the child and does not start another. A second `H` while the child is still stopping does not start a second decoder. A child that exits on its own is not a pause and is not proof the device went silent. A later `H` can keep that last sample when the same admission is still the visible listen; it still does not start audio until the following `H`. `S`, quit, and a new confirmed play clear the hold. A station listen that is still active leaves a paused hold where it is.

The service still refuses a seek inside a recording gap or the open tail, and that refusal writes no reader. The terminal does not invent a gap from the decoder sample alone. Capture is not paused. Quit still leaves the recording running. The desktop desk does not gain Play or Stop. No MCP mutation is added. `listen pause` and `listen seek` remain the metadata playhead.

On Windows, a finding resume can use the existing system excerpt path. Other platforms still refuse a system excerpt, so a finding resume there fails closed the same way. A whole-file resume stays on the ordinary `listen file` path. Neither result is an acoustic or platform qualification.

## Evidence

`cargo verify` exited 0 on this Windows host. It passed vendor and script checks, the changelog heading check, formatting, warnings-denied Clippy, workspace tests, the build, and `cargo audit --deny warnings` against 1349 advisories and 382 dependencies. The `sigy` binary tests passed 369, with 7 ignored, in 27.38 seconds. The `sigy-service` library tests passed 744, with 2 ignored, in 198.54 seconds. The command completed in 370.58 seconds. `cargo verify-media` then passed 20 ignored service fixtures in 114.16 seconds and the direct live PCM sample check in 0.07 seconds. The command completed in 126.84 seconds. `ffmpeg -version` reports `N-113513-gaa3cfd4b5a-20240202`. Coverage was not run. `cargo visual-qa` was not run. External spend remains USD 0. Headphones were not checked. This receipt was written after those commands.

A seek inside a recording gap or the open tail still writes no reader. Those checks already exist: `gap_cause_refusals_preserve_original_wording_without_admitting_readers`, `prefix_gap_refuses_zero_and_freezes_nonzero_timeline_without_inventing_audio`, `open_tail_requires_actual_running_open_reservation_and_creates_no_reader`, and `excerpt_checks_interior_gaps_and_accepts_gap_adjacent_exclusive_edges`. This slice adds no new gap fixture. Issues 13 and 14 stay open. S-03B stays open. No stage exit is claimed.

Hosted verify of `6b7b5cc` passed on Windows, Ubuntu, and macOS: https://github.com/blisspixel/sigy/actions/runs/39801593857. The Windows job runs `cargo verify`. Ubuntu and macOS run build and lint. A clean detached worktree install wrote `installed-commit` `6b7b5ccd0ace207fa476ed1f20b56d16ec1e211d`, 42 bytes with the SHA and a CRLF. `sigy --version` prints `sigy 0.1.0-dev`. The installed executable contains `Press H to start one listen` and `Heard pause cleared`. The install used that checkout and left `SIGY_SRC` unset. The rustup note names the pinned toolchain file. This later receipt does not change the program, so the installed binary remains that commit. Full `cargo verify` was not re-run for this receipt.
