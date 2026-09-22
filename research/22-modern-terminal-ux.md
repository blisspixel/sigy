# Modern terminal UX

Reviewed: 2026-09-20. Status: pre-selection research. The 2026-09-21 list measurement and backend selection are in [terminal stack](../docs/decisions/0015-terminal-stack.md). No Sigy TUI is implemented or usability-qualified yet. This extends [geography and DVR research](18-terminal-explorer-and-radio-dvr.md). Implementation behavior belongs in the [terminal experience contract](../docs/design/terminal-experience.md).

## Technology and compatibility

The crates.io metadata reviewed on this date reports these latest non-prerelease versions:

| Candidate | Version | Published | Role and decision |
| --- | --- | --- | --- |
| Ratatui | 0.30.2 | 2026-06-19 | Leading Rust renderer candidate; MIT; Rust 1.88 minimum |
| Crossterm | 0.29.0 | 2025-04-05 | Leading portable input/backend candidate; MIT |
| Termina | 0.4.0 | 2026-08-31 | Alternative backend to evaluate; MIT OR MPL-2.0 |
| unicode-segmentation | 1.13.3 | 2026-06-01 | Grapheme-boundary utility candidate; MIT OR Apache-2.0 |

Evidence: [Ratatui metadata](https://crates.io/api/v1/crates/ratatui), [Crossterm metadata](https://crates.io/api/v1/crates/crossterm), [Termina metadata](https://crates.io/api/v1/crates/termina), [segmentation metadata](https://crates.io/api/v1/crates/unicode-segmentation). Version recency alone does not select a backend.

Ratatui 0.30.2 adds a Termina backend and repairs wide-cell cleanup and widget thread-safety regressions. Its default backend uses Crossterm 0.29. Mixing incompatible Crossterm versions can split input queues and raw-mode state. Sigy should select one backend and inspect the resolved graph. Compare Termina's capability support against Crossterm's operational evidence before changing the leading choice. [Release notes](https://ratatui.rs/highlights/v0302/), [backend compatibility](https://ratatui.rs/concepts/backends/).

Use the default tested text-cell renderer first. Image protocols, unusual fonts, and accelerated terminal features are optional enhancements. The globe can use a Canvas-style projection and an offline basemap without introducing a web renderer. A dependency for animation, a component framework, or an image protocol needs a specific benefit beyond a demo.

## Rendering and input

Ghostty documents synchronized output as the application-side remedy for partial-frame tearing, together with updating changed cells instead of erasing the screen. Sigy should negotiate this capability and bracket complete draw operations. Buffered diff rendering remains necessary when a terminal or multiplexer does not support it. [Synchronized output](https://ghostty.org/docs/help/synchronized-output).

Kitty's keyboard protocol offers progressive enhancement, including disambiguation and event information. Support must be queried and scoped to the active session, then restored. Critical actions need legacy-key equivalents. Terminal, operating-system, input-method, and multiplexer shortcuts can intercept keys before the application receives them. [Keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/).

Use one input reader. Decode keyboard, paste, mouse, resize, and capability replies into typed events. Bracketed paste should become text input, never a sequence of command shortcuts. Async service responses need query/generation IDs so older results cannot overwrite a newer search. App-owned queues need bounds and a distinction between droppable display updates and ordered commands. Ratatui's state/update/view pattern is a useful basis; examples using periodic ticks or unbounded channels are not Sigy's resource policy. [Application architecture](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/), [event handling example](https://ratatui.rs/recipes/apps/terminal-and-event-handler/).

## Accessibility and text

Terminal accessibility is a property of the whole application, emulator, and assistive-technology combination. Microsoft's terminal accessibility design describes text/cursor/selection events and their screen-reader consumers. It does not give arbitrary full-screen layouts native accessible widget semantics. Test actual user journeys, and provide a linear presentation with explicit refresh and controlled announcements. [Windows Terminal accessibility design](https://raw.githubusercontent.com/microsoft/terminal/main/doc/terminal-a11y-2023.md), [NVDA 2026.2 guide](https://download.nvaccess.org/documentation/en/userGuide.html).

Adopt visible focus, keyboard equivalence, redundant non-color status, and pause controls for moving or automatically updating content. WCAG's focus and motion guidance is a design reference; applying parts of it does not establish terminal WCAG conformance. Support the user's terminal colors, explicit dark/light/high-contrast palettes, and a monochrome mode. Honor `NO_COLOR` unless explicitly overridden. [Visible focus](https://www.w3.org/WAI/WCAG22/Understanding/focus-visible.html), [pause/stop/hide](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide.html), [NO_COLOR convention](https://no-color.org/).

Unicode grapheme segmentation and display-cell width are different problems. Never slice a UTF-8 byte offset as a visible character. Combining marks, CJK wide characters, shaping, bidirectional text, fallback fonts, and width-table disagreements need terminal fixtures. UAX #11 warns that its width property is not an off-the-shelf solution for modern terminal emulators. Ghostty currently documents only left-to-right text even while supporting some RTL-script grapheme clusters. Do not claim correct Arabic/Hebrew layout merely because strings remain valid UTF-8. [UAX #29](https://www.unicode.org/reports/tr29/), [UAX #11](https://www.unicode.org/reports/tr11/), [Ghostty features](https://ghostty.org/docs/features/).

## Navigation and operational honesty

Yazi's documented help, search, pane navigation, and key hints illustrate discoverable keyboard operation. These are useful patterns to evaluate, not evidence that its particular bindings fit radio work. Sigy's proposal uses familiar arrows/Tab/Enter/Escape, contextual help, and a searchable action palette without requiring modal-editor knowledge. [Yazi quick start](https://yazi-rs.github.io/docs/quick-start/).

The CLI should remain scriptable, with structured output, useful help, predictable errors, and clear separation of stdout results from diagnostics. Full-screen entry is explicit. A TUI action should resolve to the same service operation and cost policy as its CLI equivalent. [CLI guidelines](https://clig.dev/).

Sigy-specific inference: focus, selection, audible playback, recording, processing, and monitoring are independent states. Make each inspectable. Search results must retain stable selection during refresh. A service disconnect means the latest state is unknown, not that collection stopped. Do not present an analysis queue as failed capture or label buffered playback as current broadcast time.

## Evidence required before selection and release

1. Run the same list, search, multilingual transcript, timeline, and globe workload through the leading backend and any proposed replacement.
2. Inspect rendered frames at 80x24, 120x36, and a larger terminal, plus continuous resize and a compact fallback. Include input focus, errors, empty/loading/stale/partial states, and wide-character replacement.
3. Exercise paste and international keyboard layouts, tmux/SSH, Windows Terminal, macOS and Linux profiles, monochrome, reduced motion, and relevant screen readers. Record exact versions and remaining limits.
4. Measure key-to-frame latency, idle CPU, allocation growth, frame bytes, and memory while capture and inference compete for resources. Deliberately slow the output stream.
5. Verify raw-mode/cursor/paste/mouse/alternate-screen restoration after exit, panic, recoverable I/O failure, suspend/resume, and reconnect. A hard kill cannot promise application cleanup.
6. Combine deterministic state and buffer tests with real terminal inspection. A snapshot can prove text placement in a chosen buffer; it cannot prove a font, input method, or screen reader works.

[Ratatui snapshot testing](https://ratatui.rs/recipes/testing/snapshots/) supports the buffer-test layer. Additional snapshot tooling is optional until it improves review beyond direct `TestBackend` assertions.
