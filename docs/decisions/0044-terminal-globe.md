# Terminal globe and day/night map

Date: 2026-09-24. Status: implemented and tested on Windows x86_64. This renders roadmap operation 36 on the [globe geometry](0038-globe-geometry.md). Operation 36 stays open until map and list agreement on one filter, clustering and a terminal measurement with capture running are recorded.

## Decision

The explorer gains a Globe workspace (key `7`). It draws with the Ratatui canvas in braille, so each terminal cell holds 2 by 4 dots and one data unit stays square on screen, assuming a cell twice as tall as it is wide.

- **Globe:** orthographic projection centered on the view center, a rim, and coastline arcs clipped at the horizon.
- **Flat map (`m`):** equirectangular, two wide by one high, with segments split at the antimeridian.
- **Night:** one sample per cell column and every other dot row is marked where the sun is more than 90 degrees from the zenith at one explicit UTC instant, the explorer's clock. The instant is printed in the header. Twilight bands are not drawn.
- **Stations:** stations on the current page with directory coordinates are drawn as `*`, the selected one as `@`. Stations on the far side of the globe are not drawn. The header states how many stations on the page have coordinates, so a partial page is never presented as the world.
- **Controls:** `h`/`l` rotate longitude by 15 degrees with wrap, `j`/`k` rotate latitude by 15 degrees clamped at the poles, `c` centers on the selected station's coordinates. None of these contacts a directory or station.
- **Color:** coast, night, rim and markers use terminal palette colors; monochrome, `NO_COLOR` and linear modes draw the same shapes without color. There is no animation or automatic rotation.

Coastlines are the Natural Earth 1:110m coastline, public domain, vendored as compact text with a pinned source hash and a reproducible xtask converter (see `crates/sigy/assets/README.md`). Directory coordinates describe where a directory says a stream is located, not where its speakers or subjects are.

## Evidence

Tests cover exact UTC labels including a leap day and a pre-1970 date, complete parsing of the vendored coastline, key handling (rotation, wrap, clamping, centering, workspace scoping), rendered 160 by 48 frames containing braille coastline and night and the selected marker, a far-side station hidden and then shown on the flat map, color only when monochrome is off, 80 by 24 and 40 by 10 frames, and an unmapped station counted but not placed. A rendered frame was inspected by eye.

## Limitations

Only the current page of stations is plotted; there is no clustering of stations that share a cell. Braille needs a font with those glyphs. The cell aspect is assumed, not measured per terminal. Night uses the geometric horizon without refraction. The rendering cost with capture running has not been measured.
