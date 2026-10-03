# Terminal usability review

Reviewed: 2026-10-02. Host: the Windows 11 development machine, with six other build streams sharing it. This records a newcomer walkthrough of the command line and explorer before and after the [terminal usability](../../docs/decisions/0076-terminal-usability.md) changes. It does not measure terminal latency, screen-reader behavior, other operating systems or capacity.

## Method

The baseline was the `day/polish` base commit `f5c891d`. The result walkthrough and frames predate the final rebase onto `fabfbaa` (catalog v44, local IPC v45), which merged other streams' transport, archive search and task changes without a conflict in the files reviewed here. A fresh private library was created under a path containing a space and `é`, then used for every journey: help at every level, typing mistakes, a missing library, setup without a service, service start, one first-use directory page, searches with original-script and accented text, and the explorer. Mistakes included unknown commands and flags, missing arguments, absent records, a service that was not running, a held library, a bare `ffmpeg` decoder name, an invalid playback destination and out-of-range page sizes.

The only network request was the first-use page `sigy-init-radio-v1` from `https://de1.api.radio-browser.info`, completed in 1,961 ms with 69 records accepted and 31 skipped. No station stream, click, playback, recording, inference or paid request was made.

Explorer frames were drawn by the production explorer modules through Ratatui's test backend over that library: the baseline through the running service, the final result with no service. Each of the seven workspaces, a search with `東京 Café`, a malformed and a missing finding lookup, help focus and a disconnect were drawn in color, monochrome and linear modes at 200 by 60, 120 by 36, 80 by 24, 59 by 20, 40 by 12 and 20 by 8 cells: 253 baseline frames, and 271 result frames that add a page turn. Selected frames were rasterized with the existing private cell rasterizer and inspected. The README and usage previews were redrawn the same way over a private copy of the retained 16-station page through a temporary service, which was stopped and its exit checked.

## Findings

The highest-impact baseline defects were raw operating-system errors for `service status`, `service stop` and an unresolved decoder; a bare `record not found` for every unknown identifier, including a missing source revision on `record start`; and an explorer list that showed seven stations regardless of terminal height, with only 16 of the 69 fetched stations reachable. Monochrome focus reversed the whole list, so the selection was marked only by `>`. Number keys did not match the tab order, `motion on` meant reduced motion was on, the quota was printed as unitless byte counts, the help line ignored the workspace, and several lines were cut at 80 columns. Doctor and empty views named commands without `sigy`, and an empty `schedule list` printed nothing. Most identifier, unit and page arguments of the radio, record, schedule, source, podcast and listen commands had no description, and an oversized page failed only after reaching the catalog.

Each of these is repaired with a regression test. After the change, the same mistakes print the cause and the next command, local and service-reported failures read the same, and every line of the final 80 by 24 frames is whole except provider station names, which are cut by cell width with `~`. Linear mode gives each station two labeled lines. The full ranked list, including findings left open, is in the stream report.

## Limits

These are cell-buffer renders, not terminal pixels. Rasterization used a fixed dark palette and Cascadia Mono, not the user's terminal theme. One directory page from one mirror is not a station survey. The private frames, receipts and walkthrough logs are under `.agents/polish/`.
