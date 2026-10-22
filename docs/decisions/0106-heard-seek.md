# 0106: Heard seek inside one admitted listen

Date: 2026-10-22

Status: implemented ten-second seek for one retained listen. A range editor, segment handoff, device presentation, acoustic output, and a stage exit are not claimed. Schema v52 and local IPC v54 are unchanged.

## Decision

[Decision 0105](0105-heard-pause.md) resumes one `listen file` at a decoder position. This increment moves that same admitted listen by ten seconds. It does not connect the metadata playhead to the decoder.

`[` steps backward and `]` steps forward. The step is 10,000,000 microseconds on the recording timeline. A whole-file position is the decoder sample. A cited finding position is the cited start plus the decoder sample, and the target must stay below the cited end. A target before the admitted start, or at or past the cited end, does not stop the child and does not start another.

A step that stays inside the range stops the current retained child and then starts one `listen file` at the target. A whole-file seek passes `--seek-us` when the target is above zero, and no `--end-us`. A finding seek also passes the same cited `--end-us`. Both use a new request id. The new reader receipt is an uncited range. A paused listen only moves the held position; the next `H` starts there. A child that has already exited keeps the stepped position and does not start audio.

While that new child is still running, a decoder sample commits the target. If the child and its drains finish with no decoder sample, the seek wrote no reader and the previous position is kept. `H` can start one listen there. A second key while a child is still stopping does not start a second decoder. `S`, quit, and a new confirmed play clear the hold. A station listen that is still active does not start a retained decoder.

The service still refuses a seek inside a recording gap or the open tail, and that refusal writes no reader. The terminal does not invent a gap from the decoder sample alone. Capture is not paused. Quit still leaves the recording running. The desktop desk does not gain Play or Stop. No MCP mutation is added. `listen seek` remains the metadata playhead.

On Windows, a finding seek can use the existing system excerpt path. Other platforms still refuse a system excerpt, so a finding seek there fails closed the same way. A whole-file seek stays on the ordinary `listen file` path. Neither result is an acoustic or platform qualification. [Decision 0107](0107-heard-segment-handoff.md) starts the next sealed segment after this one completes. This decision does not.

## Evidence

`cargo verify` exited 0 on this Windows host. It passed vendor and script checks, the changelog heading check, formatting, warnings-denied Clippy, workspace tests, the build, and `cargo audit --deny warnings` against 1349 advisories and 382 dependencies. The `sigy` binary tests passed 376, with 7 ignored, in 26.44 seconds. The `sigy-service` library tests passed 744, with 2 ignored, in 186.58 seconds. The command completed in 316.40 seconds. `cargo verify-media` then passed 20 ignored service fixtures in 111.18 seconds and the direct live PCM sample check in 0.07 seconds. The command completed in 119.87 seconds. `ffmpeg -version` reports `N-113513-gaa3cfd4b5a-20240202`. Coverage was not run. `cargo visual-qa` was not run. External spend remains USD 0. Headphones were not checked. This receipt was written after those commands.

A seek inside a recording gap or the open tail still writes no reader. Those checks already exist: `gap_cause_refusals_preserve_original_wording_without_admitting_readers`, `prefix_gap_refuses_zero_and_freezes_nonzero_timeline_without_inventing_audio`, `open_tail_requires_actual_running_open_reservation_and_creates_no_reader`, and `excerpt_checks_interior_gaps_and_accepts_gap_adjacent_exclusive_edges`. This slice adds no new gap fixture. Issues 13 and 14 stay open. S-03B stays open. No stage exit is claimed.
