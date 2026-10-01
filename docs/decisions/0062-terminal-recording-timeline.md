# Terminal recording timeline

Date: 2026-09-30. Status: implemented and locally tested. This is the first metadata visualizer slice of roadmap operation 37. It does not provide a waveform, spectrum, sample subscription or stage exit.

## Decision

The Recordings workspace renders a timeline from the recording metadata already returned by the service. Press `3`, select a recording with the arrows, and use `r` for an explicit snapshot reload. Selection starts no playback, capture or processing. Recording selection is separate from station selection, and a reload preserves a still-present recording identity. The existing catalog owner and IPC paths are unchanged.

Each terminal cell covers a half-open interval on the recording's media clock. `#` means published audio marked available in the catalog, `x` means a published interval is unavailable after segment release or recording deletion, `!` means a recorded gap, and `.` means time with no published interval or gap. The open tail is unpublished. `+` means the cell contains more than one state, including a partly published interval beside unpublished time. A coarse display therefore does not turn a short publication into coverage of the whole cell. The view reads no media file and does not reverify its checksum or infer silence.

The axis includes the larger of planned duration and the ends of published intervals or gaps. The planned duration remains a separate number: decoded finite audio can extend beyond that plan, and an unfulfilled plan is not measured audio. Counts of available and unavailable published intervals and gaps remain separate from the axis. Capture and storage states keep their existing labels.

The projection keeps at most 1,024 intervals and 1,024 gaps and draws at most 120 cells. When either metadata limit is exceeded, counts are explicitly partial and the axis and bar are unavailable: an omitted publication cannot become unpublished time, and an omitted late interval cannot shorten the claimed clock. Clock arithmetic uses integers and is bounded at extreme values. It adds no service request, queue, subscription or network activity. Existing recording list pagination and the 256 KiB IPC response ceiling remain. Dense recording pages can still exceed that existing ceiling; this slice does not claim otherwise.

## Verification

Independent fixtures check a late prefix gap, retained and released segments, a suffix gap and an unpublished tail; mixed and partly published cells; decoded media beyond the planned duration; zero-width, tiny and maximum-size views; whole-recording deletion and an oversized synthetic snapshot. State fixtures check that recording selection stays independent of station selection and produces no work admission. Render fixtures cover 80 by 24, 132 by 40 and 40 by 10 layouts and write text and actual buffer cells under ignored `.agents/` when explicitly requested through `SIGY_TUI_SNAPSHOT_DIR`.

The existing native segmented-recording fixture now holds staggered local IPC clients with partial request headers while it observes a further sealed interval and plays retained segments. Every stalled handler keeps the existing five-second request deadline. The capture keeps one upstream socket, remains running and publishes its next interval. This fixture passed in the full 15-test native suite. This is a bounded local capture test, not a sustained multi-source or platform capacity claim. Integrated results are recorded in [active work](../development/progress.md), and [render inspection](../../research/experiments/terminal-render-2026-09-30.md) records the actual buffer artifacts.

## Remaining work

Live waveform and frequency visualizers, explicit sample subscriptions with independent bounded drop behavior, interval navigation, dense-page pagination and measured long-running capture independence remain open. The visualized snapshot adds no retention protection and cannot make a missing or expired file playable.
