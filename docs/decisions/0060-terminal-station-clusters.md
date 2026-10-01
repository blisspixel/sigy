# Terminal station clusters

Date: 2026-09-30. Status: implemented and locally tested. This extends the [terminal globe](0044-terminal-globe.md) within roadmap operation 36. Actual terminal latency during capture still needs measurement before that operation exits.

## Decision

The globe and flat map group stations that occupy the same terminal text cell. The grouping uses the canvas label coordinate conversion, independently of the finer braille grid used for coastlines. The input remains the exact current filtered station page used by the list; a group changes presentation and preserves every station identity.

A single station uses `*`. A group of two through nine uses its count, and ten or more uses `+`. A selected station uses `@`, including inside a group, and is never overwritten by a later station in the same cell. The selected station's projected position is the group's anchor. The list remains the way to inspect and select each member.

Unmapped or invalid coordinates create no marker. Stations on the far side of the globe stay hidden. The flat map preserves the domain's longitude wrapping at the antimeridian. Grouping is recalculated for the current projection and terminal dimensions; it is not a geographic claim about proximity, shared ownership, or independent reporting.

The map header uses separate lines for the view and explicit UTC instant, the count of valid directory coordinates on the current page, and controls with the group legend. On a small drawing area, the first two lines take priority. Directory coordinates remain directory observations about the stream location, not speaker or subject locations.

The work adds no dependency, network request, service operation, recording, or processing admission. Allocation is bounded by the existing station page, and coordinate conversion rejects non-finite and out-of-bounds values without an unchecked numeric cast.

## Verification

Focused fixtures cover crowded cells, count overflow into `+`, selection independent of row order, invalid and missing coordinates, far-side exclusion, poles, antimeridian wrapping, the shared favorites-filtered page, and stale search rejection. Rendered buffers at 40 by 10, 80 by 24, and 160 by 48 check marker visibility and monochrome output. Final verification results belong in [active work](../development/progress.md).

Independent review found that floating-point operation order must match the canvas label renderer exactly to avoid overwriting a selected marker at a cell boundary. A regression uses an adjacent boundary coordinate and checks the actual rendered marker. A [buffer and capture check](../../research/experiments/terminal-render-2026-09-30.md) records bounded warm-render samples while a finite capture receives deterministic loopback audio, with exact final byte and hash agreement.

## Limitations

Only the current bounded directory page is plotted. A selected group does not display its count in the marker, and grouping changes with viewport size. Terminal cell aspect remains an assumption. A buffer render is not a measurement of terminal input or output latency, accessibility, capture throughput, or host capacity.
