# Interface image provenance

Reviewed: 2026-10-21.

## README hero

The README first screen uses [hero-dark.png](../images/hero-dark.png) and [hero-light.png](../images/hero-light.png). Both files are one 100 by 32 production render. `cargo readme-hero` regenerates them. The command runs the ignored `readme_hero` test, draws the explorer through Ratatui's `TestBackend`, and rasterizes every cell. It does not open a library, contact a station, start audio, or use the private cell rasterizer.

The frame is synthetic. Service process 1842 is connected. The selected station is North Coast, a favorite, at directory coordinates 2.2 W, 48.2 N. Ribeira and Kyst are two other synthetic markers on the same page. Listen session `play-1842` is playing. The clock is 2026-10-21 16:00 UTC, so the geometric night layer is fixed. Spin is off and the live marker stays lit. The connection line uses the service role. It does not use the local-catalog warning, and the frame has no fixture refresh, listen, or recording id.

The image is not a live station, a coverage survey, native terminal pixels, or acoustic proof. Coastlines are the vendored offline geometry. Directory coordinates are not reception coverage. Selecting a station does not start playback; the listen strip is part of this constructed frame.

Each cell is 8 by 16 pixels with a 12 pixel margin. ASCII uses the public-domain `font8x8` basic Latin table, with each row doubled. Braille dots and the live marker are drawn from their code points. Other scripts are not shaped. The dark and light files use the same roles. The terminal palette supplies the hues. Sigy has no second theme.

Dark reset foreground `#d9dfe7`, background `#0c0f12`. ANSI slots 0 through 15: `#0c0f12`, `#c50f1f`, `#13a10e`, `#c19c00`, `#0037da`, `#881798`, `#3a96dd`, `#cccccc`, `#87919d`, `#e74856`, `#16c60c`, `#f9f1a5`, `#3b78ff`, `#b4009e`, `#61d6d6`, `#ffffff`.

Light reset foreground `#211e18`, background `#f4f0e6`. ANSI slots 0 through 15: `#2a261f`, `#9f1d1d`, `#1b7a32`, `#8a5a00`, `#1a4f8b`, `#6d3a86`, `#0d6e78`, `#5e584e`, `#7a746a`, `#c4453c`, `#2d8a40`, `#a67c00`, `#2b62b0`, `#8d4e9c`, `#1a8c93`, `#211e18`.

## 2026-10-04 checkpoint

Images captured: 2026-10-04.

These images preserve that dated checkpoint. Current quick playback and confirmed filtered fetch keys are documented in [usage](../usage.md#list-explorer) and [desktop controls](../desktop.md); the images are not evidence that those later controls were exercised on a native terminal.

[globe.png](../images/globe.png) remains in the README below the install section. [tui.png](../images/tui.png) is no longer the first screen. Both are production terminal-cell renders at 100 by 32 cells, rasterized with a fixed dark terminal palette and Cascadia Mono. They show a retained real directory page through the canonical local library loader, current state/key handler, layout and offline geography. The disposable capture uses copies of the production explorer modules, with visibility widened only in its module entry point. The interface images are not native operating-system pixel captures.

The original bounded Radio Browser refresh completed on 2026-09-23 at 04:12:38.345 UTC. It searched `france` with the upstream healthy filter and a limit of 16. All 16 station records remain from that observation; only two contain coordinates. The selected station is Douce France. Its displayed 11-day age and two-of-sixteen geographic coverage preserve the limits of that stale page. Directory health and language fields are provider metadata, not measured playback or detected speech. The geometric night layer uses the capture's 2026-10-04 UTC instant and vendored offline coastlines.

The capture migrated a private quiescent library copy through the current canonical owner to schema 50. No service was started. There was no refresh, station request, click, playback, capture, inference, model download or paid request. Recording and playback are idle. Original images, input records, cell JSON, text and capture logs remain in private ignored artifacts. The images do not qualify worldwide coverage, terminal font shaping, audio devices or a language profile.

For a repeatable review of all implemented workspaces, dialogs, narrow layouts and failure states, use [the offline visual QA gallery](visual-qa.md). Those gallery observations are synthetic and have separate source and frame hashes. They are not the retained real metadata used in `globe.png`, and they are not the README hero.
