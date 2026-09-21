# Terminal geography, station freshness, and radio DVR

Reviewed: 2026-09-20. Status: desk research and proposed interaction/verification design. No renderer, recording engine, or stack selected.

## Concept

A geographic listening desk combines a navigable globe or world map, searchable station results, persistent playback controls, and a view of active collection. Location becomes an inviting way to discover sources. The same selected source can be heard, recorded, translated, or added to a monitor without changing workspaces.

The terminal interpretation should use its own layout, typography, map assets, and interaction model. A spinning globe, day/night shading, source markers, and a recording timeline can convey the experience without a browser or a photographic Earth texture. All source operations also belong in the complete CLI.

## Terminal rendering evidence

Ratatui's official Canvas example demonstrates terminal maps, points, lines, layers, and Braille markers. This establishes relevant drawing primitives in one candidate ecosystem; it does not demonstrate Sigy's rotatable globe, performance, or a reason to select Rust. [Canvas example](https://ratatui.rs/examples/widgets/canvas/).

Windows documents virtual-terminal sequences for cursor and color control. Terminal support, cell dimensions, Unicode glyph appearance, SSH bandwidth, and multiplexers still vary. Design a capability profile and fallback modes rather than depending on a particular terminal's image protocol. [Windows VT documentation](https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences).

Proposed rendering alternatives:

| Mode | Strength | Qualification work |
| --- | --- | --- |
| Text/Braille or block-cell globe | Works within a conventional TUI, compact geometry, no image decoder required | Cell aspect ratio, hidden hemisphere, projection/hit-testing, glyph visibility, rendering cost |
| Flat world map | Shows worldwide activity together, useful for day/night and dense sources | Antimeridian wrapping, polar distortion, marker clustering, label overlap |
| Optional terminal graphics | Potentially richer appearance | Terminal/protocol/multiplexer matrix, memory/bandwidth, cleanup and portability |
| ASCII or list mode | Broad compatibility and accessible equivalent data | Useful hierarchy, readable status, keyboard workflows |

Text-cell graphics are the proposed baseline to evaluate. Optional enhanced rendering cannot be required for discovery or collection. The globe may use an orthographic projection: rotate a spherical scene, hide its far side, project visible features, and compensate for cell proportions. This is an implementation-independent design proposal, not renderer code.

## Geographic assets and truthful location

Natural Earth publishes map data under public-domain terms. A versioned, simplified coastline dataset is a candidate for an offline basemap without a paid tile service. Verify the exact asset, origin, and distribution record; no assets were copied in this research phase. [Data terms](https://www.naturalearthdata.com/about/terms-of-use/).

Keep station-declared locality, directory coordinates, transmitter coordinates, receiver location, and processing host location as different fields. Internet endpoints often describe delivery infrastructure, not the originating studio or transmitter. Country/city approximations should be labeled and never presented as precise measurement. Unknown locations remain discoverable in a list. A coordinate of zero is not inherently missing.

Map counts need scope: directory matches, sources with usable coordinates, markers in this viewport, active acquisitions, and currently analyzed sources are different quantities. Clusters must expose their underlying records and prevent map resolution from hiding low-density regions.

## Sun and night

NOAA documents solar-position calculations, their assumptions, and accuracy limitations, including atmospheric effects relevant to apparent sunrise/sunset. Use an independently checked method with a defined date range and time convention; the visual overlay is not an observatory instrument. [Calculation details](https://gml.noaa.gov/grad/solcalc/calcdetails.html).

Proposed display: compute the Sun's direction from an explicit UTC instant and shade the spherical surface according to whether it faces the Sun. The geometric day/night boundary is distinct from atmospheric sunrise/sunset and optional twilight bands. A flat-map view uses the same underlying calculation. Label the selected instant and whether it is live, frozen, or historical.

Updating the overlay must not change recording schedules or the service clock. Scene rotation and clock progression are independent. The map should work offline with local time and bundled geometry. Incorrect host time can make the overlay inaccurate and must not be disguised as network-verified time.

Day/night context can be useful for world listening and later radio exploration. It is not a forecast of reception, ionospheric conditions, signal strength, or internet-stream availability. Any later propagation model needs separate inputs and validation.

## Station freshness

Radio Browser exposes identifiers, optional coordinates, change/check timestamps, search filters, recently changed stations, and historical versions. Its health information is a directory observation, not a guarantee of local playback or a programme schedule. [API reference](https://docs.radio-browser.info/).

The proposed adapter combines cached results, bounded background refresh, explicit refresh requests, and direct user sources. Treat recently changed listings as a reconciliation aid until ordering, pagination, deletion, and mirror consistency are verified; do not assume they are a complete durable event stream.

Stage refreshes before publishing a new catalog generation. Keep previous usable results on partial failure, record freshness, and reconcile favorites by identity. A vanished search result does not prove a station was deleted. Changed URLs should create source-configuration revisions without rewriting historical recordings or abruptly switching a healthy acquisition.

Refresh policy should bound requests by provider/host and use backoff and jitter. Search should debounce/cancel stale requests. Discovery must not open every stream to determine health, automatically subscribe to every match, or inflate directory popularity through synthetic playback clicks.

## DVR semantics to specify

A DVR-style experience needs more than a record button: an enabled rolling buffer, independent playback position, bounded pause/rewind, return to live, saved intervals, scheduled captures, and a library timeline. Its capabilities depend on material Sigy actually retained.

Proposed storage design reuses immutable media segments through references for the rolling window, explicit recordings, and evidence pins. Promotion into a saved recording must reserve retention and atomically protect its selected intervals against concurrent expiry. A paused listener can fall behind the oldest retained point; show that outcome and offer a deliberate jump rather than silently playing a different time.

Radio often lacks dependable programme metadata. A station/time recording planner is useful without an electronic programme guide. Show a programme title only with a known source and update time. Programme-aware recurring recordings and publisher catch-up archives are separate adapter capabilities to qualify later.

## Experiments

Before choosing a TUI library, specify the same globe/list/DVR workload for viable stacks. Test keyboard-only rotation and selection, reduced motion, small terminals, Unicode names, monochrome, resize, SSH/multiplexers, and slow clients. Rendering must stay inside a measured resource and output-bandwidth allowance during multi-stream capture and local inference.

Check projection at the poles and antimeridian, marker visibility on the hidden hemisphere, fixed-date solar reference cases, missing/approximate locations, and rapidly changing active-source state. Test catalog refresh failures and identity corrections without losing filters or favorites.

For DVR, test codec boundaries, reconnect gaps, buffer expiry, save/expiry races, two independent playheads, service restart, clock changes, recurring schedules, storage exhaustion, and explicit recording-stop behavior. The complete CLI must cover equivalent operations without ever entering a full-screen interface.
