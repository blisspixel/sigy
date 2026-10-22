# 0103: Decoder timeline during a retained listen

Date: 2026-10-21

Status: implemented observation of decoder output time during one retained listen. Pause, seek, a range editor, segment handoff, device presentation, acoustic output, and a stage exit are not claimed. Schema v51 and local IPC v53 are unchanged.

## Decision

[Decision 0102](0102-terminal-retained-playback.md) starts one whole retained recording or one cited finding and shows a phase on the `Retained:` row. This increment does not add a control. While that new listen decodes, the child writes `decoder-timeline-us N` to stderr. `N` is FFmpeg `out_time_us`, measured from the admitted start. The child publishes at most one sample per second of that clock, and it also publishes a later final frame. A full observer drops the sample. Decoding does not wait. Replay, a refusal before decode, and an idle row write no sample.

The terminal keeps the same plain row. When a sample exists it reads `Retained: PHASE decoder N us REQUEST`. The first sample adds one note: decoder output is measured from the admitted start and does not prove audible output. Later samples update the number only. A smaller or malformed line does not move the number backward. Other stderr text stays in the existing 8 KiB diagnostic and is shown when the child exits non-zero. Stdout remains the final receipt.

This number is not the recording timeline, the in-memory playback playhead, the Windows helper presentation estimate, or proof that a device played the samples. The metadata playhead still does not control the decoder. [Decision 0105](0105-heard-pause.md) pauses this output time and resumes one `listen file` at that recording-timeline position. It does not make the metadata playhead the same control. Seek and segment handoff stay later. The desktop desk does not gain Play or Stop. No MCP mutation is added. S-03B stays open.

## Evidence

`cargo verify` exited 0. It passed vendor and script checks, the changelog heading check, formatting, warnings-denied Clippy, workspace tests, the build, and `cargo audit --deny warnings` against 1348 advisories and 382 dependencies. The `sigy` binary tests passed 361, with 7 ignored, in 24.55 seconds. The `sigy-service` library tests passed 737, with 2 ignored, in 170.06 seconds. `cargo verify-media` then passed 20 ignored service fixtures in 110.70 seconds and the direct live PCM sample check. `ffmpeg -version` on `PATH` reports `N-113513-gaa3cfd4b5a-20240202`. Coverage was not run. `cargo visual-qa` was not re-run. No acoustic, Omarchy, language, or platform qualification is claimed. External spend remains USD 0. Hosted verify of `6112bcd` later failed on Windows: https://github.com/blisspixel/sigy/actions/runs/39639992575. Ubuntu and macOS build and lint passed. The Windows failure was `direct_installer_fences_a_real_helper_before_build`, after this listen had already published its decoder timeline. The repair is recorded in active work.
