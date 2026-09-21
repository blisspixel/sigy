# Terminal experience contract

Updated: 2026-09-21. Status: implementation target, not implemented UI. This refines [explorer and DVR](../planning/11-radio-explorer-and-dvr.md) using [current terminal research](../../research/22-modern-terminal-ux.md). Source, storage, and spending policy remain owned by the service.

## Product feel and hierarchy

The interface is a listening and research desk: clear source identity, readable content, a useful geographic view, and dependable controls. Visual interest comes from the globe, real activity, maps, and timelines. Do not fabricate signal activity or obscure work beneath decorative effects.

Visual quality, modern interaction and enjoyment are product requirements from the first usable TUI. The interface should invite exploration and reward curiosity through responsive controls, expressive instruments and understandable discoveries. Visual polish belongs in each delivered workflow, including empty, loading, disconnected and error states.

Use six workspaces: Explore, Live, Recordings, Monitors, Findings, and System. Keep the selected source and current playback visible across workspaces. A compact header reports service connection and important resource/cost state; detailed diagnostics belong in System. Avoid filling every panel with borders and abbreviations.

Explore links one query and selection across list, globe, and flat map. Wide layouts show results beside source details or geography. Compact layouts use one focused pane with explicit navigation. At 80x24, search, source identity, primary actions, playback state, and help remain usable. Below that, reduce columns and previews before hiding controls. Very small dimensions must render a safe recovery prompt without panicking.

## Visual direction and discovery

- Use deliberate spacing, alignment, text emphasis and restrained borders to establish hierarchy within terminal cells. Give the main instrument or content room; advanced measurements expand on demand.
- Use a coherent palette with vivid accents, readable contrast and stable meanings for selection, recording, uncertainty and failure. Mode-specific accents must not redefine status colors. Themes use the same shared components and semantic color roles.
- Make motion explain interaction: globe rotation follows navigation, a playhead follows retained media, and a meter reflects measured activity. Transitions are brief, interruptible and subject to the existing frame/resource limits. Reduced motion preserves every operation.
- Keep exploration direct: linked map/list selection, frequency bookmarks, quick comparisons and a discoverable action palette. Opening details or changing a view never starts collection implicitly.
- Offer explicitly selected local practice material for first-use exploration and signal experiments. Label synthetic or prerecorded observations and simulated time clearly; demos do not need an account, live network or paid provider.

| Presentation | Character and useful visual focus |
| --- | --- |
| World listening desk | Rotatable globe, day/night geography, station detail and a persistent player; useful even when a station has no known coordinates |
| Broadcast radio | A clear frequency dial, presets, measured audio activity and source identity, integrated with recording and translation |
| CB receiver, later | Channel grid, scan/hold controls, squelch and activity history with a distinct instrument layout |
| Signal laboratory, later | Waterfall, spectrum, timing or packet views with bookmarks, replay and side-by-side interpretations |

Receiver presentations are contextual views within the shared workspaces, not separate applications or command systems. RF displays require a qualified RF source; an internet audio waveform must not masquerade as measured radio spectrum. Obscure signals remain interesting to inspect even when their meaning is unknown.

Before accepting an explorer layout, inspect real rendered design studies at compact and wide sizes with representative multilingual data. Assess whether the primary action is discoverable, visual hierarchy survives busy and empty states, and exploration feels responsive and enjoyable. Deterministic layout checks and performance measurements complement this visual review; passing snapshots alone does not establish design quality.

## Interaction contract

| Intent | Baseline interaction | Invariant |
| --- | --- | --- |
| Navigate | Arrows, Tab/Shift-Tab, Enter, Escape | Visible focus and a predictable return path |
| Find stations | Explicit search field, filters, clear/reset | Ordinary editing while focused; stale query responses cannot replace current results |
| Discover actions | Contextual footer, help, searchable palette | Available and unavailable actions explain their scope/reason |
| Inspect a source | Enter opens details | Selection alone never starts audio, capture, processing, or paid work |
| Listen / pause / seek | Explicit player controls | Playback destination, playhead, retained range, and live delay are visible |
| Record / monitor | Bounded settings and a clear start action | Finite resources and applicable spending policy before admission |
| Leave TUI | Quit action, `q` outside text input, Ctrl-C | Detach the client; durable jobs keep their service ownership |
| Stop a job or service | Distinct named action | Never overloaded onto leaving a view or closing the TUI |
| Copy / paste | Explicit copy; native selection where possible; bounded paste | Untrusted text cannot become terminal commands or action shortcuts |

Bindings are configurable, with conflict validation and reset. Optional modal-editor aliases cannot be required. Avoid mandatory function keys, Alt-only combinations, or bindings the platform reserves. Support key-repeat for navigation without repeating expensive mutations. Mouse interaction is optional and has a discoverable keyboard equivalent.

Keep station selection anchored to identity during resort/refresh. When an item disappears, show why and select a deliberate nearby result. Preserve queries, filters, and scroll positions on return. Do not steal focus for background events. Coalesce recurring failures into an inspectable activity entry; use inline messages for local input errors. Notifications that require action must not expire before they can be read.

## Live state and DVR

Expose selection, audible playback, recording, queued analysis, and monitor membership separately. Show capture gaps, processing lag, unknown language, and unavailable translations without replacing them with artificial activity. Busy analysis must not make successful capture look stopped.

The timeline identifies retained audio, unavailable intervals, bookmarks, and processing coverage. Pausing playback does not pause capture. A playhead that expires receives a clear choice of retained audio or live playback. Caption mode identifies whether text follows the playhead or incoming content. Side-by-side original/translation is a wide-layout option; compact mode switches between both without losing alignment.

As detectors become available, aligned tracks show language changes, candidate ads and song boundaries with uncertainty and revisions. Speech and music can overlap; an unresolved song still has an inspectable interval. Follow the [broadcast analysis contract](broadcast-analysis.md), keeping these planned views distinct from currently implemented behavior.

When disconnected, mark the snapshot's age and last confirmed state. Disable commands that cannot be safely submitted. Reconnect retrieves fresh state before accepting dependent actions. Preserve local editing, but never silently replay a start, deletion, or paid-processing request. A retryable operation needs service-defined idempotency, not a UI guess.

## Localization, accessibility, and terminal safety

Use the [language contract](languages.md). Interface language is independent of source language, translation target, and model capability. Layout uses measured cells and grapheme boundaries, not English string length. A translated label must not change the underlying command ID, stored enum, or JSON field.

Provide a linear mode with stable reading order, explicit refresh, controllable announcements, and no map animation. Keep complete CLI parity and plain output. Test the actual terminal/screen-reader combination; neither a CLI nor keyboard-only navigation alone proves accessibility. Use text or shape alongside color, clearly identify focus, and support terminal-default, light, dark, high-contrast, and monochrome presentation. No custom font or icon package is required.

Reduced motion freezes automatic rotation and decorative pulses; geographic controls still work. Let users pause rapidly updating lists/transcripts for reading while collection continues. Auto-follow is a visible mode and turns off when browsing history. Search and controls remain available during refresh.

Use one canonical terminal-text boundary for station names, transcripts, file names, provider errors, and model output. Preserve original evidence in storage; presentation rejects/escapes terminal controls and isolates untrusted content from trusted status. Bidirectional formatting needs a reviewed renderer policy, not removal of all non-ASCII text. Never execute embedded escape sequences, terminal hyperlinks, or clipboard requests from source content. Copy/open actions use validated targets and explicit intent.

## Runtime and verification

The TUI owns view state, a typed update loop, one input reader, and one renderer. Service I/O runs outside rendering and returns bounded events. Do not create another scheduler, retry policy, catalog connection, or budget implementation inside widgets. Coalesce obsolete visual updates while preserving ordered command results.

Draw on meaningful changes. Initial measurement targets are p95 local key-to-frame latency under 100 ms, zero continuous redraw while idle, and a 15 fps ceiling for active visualizations. These are targets to test, not measured promises. Coarse health counters can refresh at 1 Hz. Slow output or heavy processing reduces visual work first. Globe rotation, capture, and analysis capacity must be measured independently on desktop and small-host profiles.

Negotiate enhancements with deadlines and fallbacks. Use synchronized output where supported, diff rendering everywhere, and no requirement for image protocols. Restore acquired terminal modes on normal exit and recoverable failures, including panic paths; document the hard-kill limit. Diagnostics must not write into the active drawing surface.

Acceptance requires state-transition tests, bounded-event and stale-response tests, buffer assertions, and actual terminal inspection. Test 80x24/120x36/large layouts; continuous resize; long names; combining marks; CJK/RTL content; Canadian French, Navajo, and Klingon fixtures; paste; disconnection; no-color/reduced-motion/linear modes; and simultaneous capture/processing. Record exact OS, terminal, font, input method, and assistive-tool versions. Do not approve new snapshots merely because they match the current renderer.
