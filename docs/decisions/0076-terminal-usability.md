# Terminal usability

Date: 2026-10-02. Status: implemented and locally tested on Windows x86_64. It changes neither the catalog schema nor local IPC, which are v44 and v45 on the base it was verified on. This refines the [list explorer](0016-list-explorer.md), [terminal globe](0044-terminal-globe.md) and [first-use setup](0070-first-use-setup.md). It adds no service operation, network request, dependency or authority, and it exits no roadmap stage.

## Decision

The explorer and the command line explain their state in terms a newcomer can act on, without changing what any action is allowed to do.

**Explorer.** The tab bar shows every workspace with its number key, in key order (`1` Explore through `6` System and `7` Globe); arrows cycle in that order. The compact layout names the current workspace in its title line. The connection line says `no service, local catalog` when no service holds the library, and `reduced motion` replaces the ambiguous `motion` label. The animation frame count moves from the screen to the `--inspect` report.

The station list fills its area. Name, favorite, directory health and directory language columns align by terminal cell width without splitting graphemes; a cut name ends in `~`. Narrow views drop the language column, and station IDs appear only when they fit whole. The selected row is one uniform reversed bar while the list has focus, so selection never depends on color; a focused body marks only its first line instead of every row. Linear order gives each station two lines with every value labeled, so the labels stay whole at 80 columns. The selected station's observation age moves beside its directory health, and the identity label reads `Selected station`.

`n` and `p` turn pages of the current cached search in Explore and Globe. They reuse the existing `radio search` cursor: a stack of earlier cursors returns without a second index, a stale response cannot move the cursor, and reload keeps the current page. A new search or the favorites filter returns to page 1. Paging is a local cache read like search.

Help follows the workspace. Quota sizes use decimal units because the quota is configured in GB. Empty Explore, Recordings and Live views name the command to run. Status messages are full sentences. After a disconnect the status line leads with what was kept and that `r` reconnects, and ends with the cause. Recording timelines add a readable length beside exact microseconds.

**Command line.** Common failures keep their cause and add the next command: unknown identifiers name the listing command, `service status` and `service stop` say when no service is running, a service-only command says to start the service, a held library says which command applies, an invalid station ID points to `radio search`, and an unresolved decoder path asks for an absolute ffmpeg path. The `--data-dir` reminder appears when one was supplied. Doctor states whether the service is running and prefixes each next step with `sigy`. Setup prints the library without the Windows verbatim prefix. Every common argument documents its meaning, unit or range; page sizes, playback destinations and publisher text kinds are checked by the parser against the service's bounds, so an invalid value fails with exit status 2 and the accepted range before any library access. The top-level help ends with three first steps.

## Boundaries kept

Selection, paging, reload and workspace changes start no audio, capture, refresh, click or processing; the command-effect fixture still permits only the existing reads and the favorite toggle. Quit detaches without stopping the service or a recording. JSON output, exit status for runtime failures, IPC requests and stored data are unchanged.

## Verification

State fixtures cover page turns, the cursor stack, stale page responses, reload cursor reuse, refusal outside Explore and Globe, and agreement between number keys, tab order and arrows. Render fixtures at 20 by 8 through 200 by 60 check list height, column alignment with wide characters, the reversed selection in color and monochrome without color cells, per-workspace help within 80 columns, empty views, linear labels and the local-catalog line. Unit and process fixtures cover the guidance texts for local and service-reported failures, decoder resolution, parser ranges, doctor and setup output; the service-reported path was also observed through live local IPC. Rendered frames over a real 69-station first-use page and the retained 16-station page were inspected; the private review is recorded in [terminal usability review](../../research/experiments/terminal-usability-2026-10-02.md).

## Limits

The station list still follows the cache's identifier order, not names. Screen readers, real terminal latency and other operating systems were not measured. The README and usage previews are cell-buffer renders, not terminal pixels.
