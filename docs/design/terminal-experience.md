# Terminal experience contract

Updated: 2026-10-03. Status: the list explorer is implemented in [0016](../decisions/0016-list-explorer.md), the globe and flat map in [0044](../decisions/0044-terminal-globe.md) with [cell clusters](../decisions/0060-terminal-station-clusters.md), read-only monitors in [0061](../decisions/0061-classification-off-and-monitor-inspection.md), recording metadata timelines in [0062](../decisions/0062-terminal-recording-timeline.md), and named finding navigation in [0063](../decisions/0063-terminal-finding-navigation.md). The applied/draft cache filter editor is implemented in [0077](../decisions/0077-terminal-cache-filters.md), and offline country resolution/picking in [0080](../decisions/0080-offline-country-reference.md). City resolution, terminal audio controls, listening sessions, live captions, waveform and frequency views remain proposed. This refines [explorer and DVR](../planning/11-radio-explorer-and-dvr.md) using [current terminal research](../../research/22-modern-terminal-ux.md). Source, storage, and spending policy remain owned by the service.

## Product feel and hierarchy

[Focused cached context](../decisions/0082-focused-station-context.md) is now implemented: Enter inspects a frozen full station observation, Up/Down scrolls and Escape/Backspace returns to the same browser state. Wide nonlinear layouts keep the cached results alongside it; compact/linear views use one column. Actual player controls and exact station-registration/recording links remain open.

The interface is a listening and research desk: clear source identity, readable content, a useful geographic view, and dependable controls. Visual interest comes from the globe, real activity, maps, and timelines. Do not fabricate signal activity or obscure work beneath decorative effects.

The 2026-10-02 visual direction is retro-futuristic: late-1980s computer rooms, WarGames-style geographic command displays and hands-on radio instruments, with modern responsiveness and information design. The intended sense of power comes from moving fluently between worldwide discovery, simultaneous recordings, aligned language tracks and inspectable findings. Treat this as a design reference; it supplies no institutional affiliation or analysis capability.

Use generous instrument space, crisp typography, a restrained phosphor-inspired accent palette and strong focus/status contrast. Offer terminal-default and light/high-contrast alternatives with identical meaning. Character-cell maps, time rulers and real activity histories can establish the atmosphere. Keep text instantly readable: boot theatrics, forced typewriter output, fake traffic, scanline interference and cryptic-only controls cannot delay or distort operational information. A focused instrument may occupy the full screen, with source, clock, capture state and a reliable return action still visible.

The 2026-09-30 clarification emphasizes a professional instrument and operations desk with substantive utility, enjoyable exploration and education. Personal journal presentation is not desired. Status reports remain proposed; optional task achievements became confirmed intent in the 2026-10-03 [learning followup](learning-experiences.md), with mechanics still proposed. Both derive from actual operations and evidence, remain unimplemented and do not qualify analysis quality or user expertise. Follow [professional utility and exploration](../planning/01-product-and-experience.md#professional-utility-and-exploration).

Visual quality, modern interaction and enjoyment are product requirements from the first usable TUI. The interface should invite exploration and reward curiosity through responsive controls, expressive instruments and understandable discoveries. Visual polish belongs in each delivered workflow, including empty, loading, disconnected and error states.

The 2026-10-03 clarification confirms a passion project that combines serious signals-workstation capabilities with ambitious, playful exploration. Design expansive terminals as rich instruments with deliberate multi-pane composition, fluid linked navigation and real geographic/time/signal activity. Compact access is a compatibility requirement, not the visual ceiling. Preserve query, source, playhead and investigation context across views; keep expert detail reachable and routine exploration reversible without repeated setup. Assess operational correctness and interaction quality in the same increment. Receiver personalities, educational experiments and optional creative explanation extend the serious workflow through the same truthful source, authority and evidence contracts.

The design groups six task workspaces: Explore, Live, Recordings, Monitors, Findings, and System. The current terminal exposes Globe as a seventh workspace. Keep the selected source and current playback visible across workspaces. A compact header reports service connection and important resource/cost state; detailed diagnostics belong in System. Avoid filling every panel with borders and abbreviations.

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

## Full-screen explorer and place search

The 2026-10-02 clarification makes a fuller-screen, artistic and exceptionally usable signal explorer an explicit requirement. CLI and TUI remain the initial complete interfaces; a later native application reuses their service operations. No browser runtime or required image protocol belongs in this increment. [Research](../../research/35-reliability-scale-and-interpretation.md#explorer-place-search-and-directory-recovery) and the [engineering plan](../development/reliability-and-scale.md) distinguish desired behavior from current implementation.

Use the available height and width deliberately. Proposed breakpoints: at 80x24, one focused content pane with persistent source/actions; around 120x40, results beside source details; around 160x50, results, geography and contextual evidence. Validate exact layouts rather than treating dimensions as support claims. Let geography and passages expand into spare space. Avoid decorative panel density, tiny centered content on large screens and moving focus during background updates. Retain full source identity on demand, clear filter chips, an obvious search field, stable selection and a predictable return path.

Worldwide place discovery must work independently of the station page already cached. The implemented [country reference](../decisions/0080-offline-country-reference.md) supplies 257 entries and eight display locales from pinned CLDR data, explicit locale fallback and a shared bounded resolver. Every included entry remains selectable when its cached station count is zero or unknown. Unknown or unmapped directory country values remain accessible by literal code. This reference establishes no station coverage or city gazetteer. Later locality references need their own declared inclusion policy, versions and required legal notices.

Let users type a country name/code, a later city or station name and choose a visibly labeled interpretation. Country resolution gives raw two-letter ASCII codes their explicit literal meaning and evaluates ambiguity across the complete normalized name candidate set before paging; an exact label cannot hide other matching identities. Later place matching and typo suggestions need separate declared rules and explicit selection. A visible query distinguishes country/place, station text, language, tags, favorites and health/freshness. Filters combine predictably and can be removed individually or reset together. Show the match basis and retain query/selection when switching views or returning from a source. A saved query stores intent, not authority to acquire or process its results.

Keep country, declared language, directory observation time, upstream health and local playback/capture state distinct. [Selected name ordering](../decisions/0081-ordered-station-search.md) uses pinned normalization/full folding, explicit UTF-8 byte comparison, UUID tie-breaking and a cursor bound to query, ordering and catalog generation. Refresh must not silently reshuffle an active page; offer a new generation while preserving the selected identity. Facet and result counts state their cached scope and whether they are partial or unknown. Bound query text, filters, candidate count, rows scanned, response bytes and elapsed time. Search editing, filtering and keyboard navigation remain responsive during service or network delays. The same typed query/cursor/result semantics belong in the CLI.

Radio Browser does not provide a canonical city field. A city journey needs evaluated place resolution, not a state/name substring presented as geographic fact. A bounded offline gazetteer is a candidate: retain place ID, multilingual aliases, country/administrative hierarchy, coordinates, dataset hash/date, matching rules and required license attribution. Show ambiguous choices such as Paris in different countries/regions. A chosen place can filter stations by a stated radius over available directory coordinates, labeled approximate directory locations near that place. Missing-coordinate stations remain discoverable. Distance is not reception coverage, studio/transmitter location or proof of local audience. Public background geocoding is not assumed.

The minimum place reference must represent capitals and a declared major-city set across the world, rather than a hand-picked English-language list. Before selection, publish the inclusion rule and per-country/territory coverage manifest, including absent capitals, missing aliases and dated population fields. Evaluate a compact global city extract plus necessary administrative seats and language-tagged aliases against that contract; expand only within measured storage/import/search limits. The intended first complete installation includes its qualified compact reference, making place navigation useful even in an empty offline library. Smaller or unmatched places fall back to explicit country/region, station-text or manually selected coordinate/radius search. A resolved city may have no cached nearby station. Distinguish missing place data, missing station coordinates and no cached matches, and offer an explicit bounded directory refresh where it can help. None establishes that a place has no broadcasters.

As other source adapters arrive, the explorer keeps the same discover/inspect/capture/evidence journey with source-specific typed facets. Country and audible playback are radio capabilities, not mandatory properties of packets, IQ or symbolic material. A combined search labels each result's source type and coverage; retained-passage search remains distinguishable from source-directory discovery.

## Directory recovery and local validation

Recovery is service-owned and bounded. Opening a workspace reads cached state. The existing saved refresh policy can fetch its bounded page without a client; broader saved discovery needs explicit finite pages, requests, bytes, elapsed time, retries and cache growth. Keep last usable generations, favorites, labels and identity links through mirror failure or malformed/partial pages. Record latest attempt separately from last success, expose partial coverage and next eligible retry, and never delete unseen stations because an incomplete enumeration omitted them.

Separate cache freshness, upstream health, Sigy's local connection observation and actual bounded decode success. Upstream resolved URLs grant no network authority. A local validation policy needs an explicit finite grant over exact source revisions, allowed destinations/redirects, count, bytes, duration, decoder resources and retries. `HEAD` alone does not prove playback. An authorized short fetch/decode may produce dated evidence, never a permanent availability guarantee. Validation cannot contact private or otherwise ungranted destinations or open paid inference.

Healing a changed endpoint proposes and validates an immutable source revision. An accepted standing policy may authorize future use within its exact scope; otherwise expose an acceptance action. Preserve prior history, active captures and existing allowance consumption. Reconnect refreshes service state and reconciles known idempotent operations; it never blindly repeats a mutation or resets a budget. Failed validation remains inspectable and offers a useful next action rather than silently erasing the source.

Deliver in separate increments: shared search/name ordering/freshness and worldwide country reference; evaluated place search; broader bounded refresh; opt-in local validation and endpoint repair. Inspect offline, empty, partial, unavailable and recovering states as carefully as the successful view. Measure actual Windows/SSH/Linux terminal input latency, output bandwidth and capture integrity during inference; screen buffers alone do not establish this.

## Expressive views and equivalent access

Beauty comes from actual material and coherent control. Optional focused listening can reduce panel density while preserving source, target, capture state, timing and return paths. Original/target comparison retains scripts and cue alignment. Display wrapping is not authored poetry; a separately selected expressive interpretation cannot overwrite a faithful transcript or translation. Later sonification must disclose its data-to-sound mapping, units, range, clipping and gaps and remain distinct from received audio.

Every geographic or visual discovery needs a useful text route. Reduced motion, monochrome and linear views retain equivalent operational information. Evaluate long translated labels, CJK, combining marks, right-to-left text, narrow windows and assistive terminal combinations. Humor belongs in explicitly selected exploration/practice and requires locale/context review; failures, spending controls, sensitive evidence and structured CLI/MCP responses stay literal. No expressive view grants acquisition or processing authority.

## Calm listening sessions

The 2026-10-03 listening clarification refines the existing world listening desk. This is a proposed client composition over service operations, not a second scheduler or a new authority grant. A session names its inspected source, audible source and playback generation separately. At most one foreground audio destination is selected in the first increment; independent service-owned captures continue under their existing bounds. Opening a session starts inspection only. Current audible listening is through the CLI; the TUI does not yet provide these controls.

The focused view keeps source identity, play/pause/stop, retained or direct-live status, current playhead and a reliable return action visible. Geography, originals, chosen-target text and details are optional context. There is no required animated map, telemetry wall or transcript feed. Hide detail without hiding an admission refusal, missing media or unresolved stop. Silence and absent captions are legitimate states; activity animation needs actual measurements.

| Action | Proposed transition and bounded effect |
| --- | --- |
| Browse or inspect another source | Change inspection identity only; preserve the audible source and query return position |
| Listen | Explicitly admit one source/range and destination with finite limits; show starting until the matching generation confirms readiness |
| Replace audible source | Request old playback stop, wait for its proven completion, then admit the selected replacement; keep at most one pending replacement and never overlap implicitly |
| Pause or seek | Affect playback only; seek within admitted retained media, and explain a gap or expired range |
| Record or process | Separate explicit bounded admission; show its service-owned result independently of playback |
| End session or leave client | Stop client-owned audible playback; retain unresolved reader protection until proven completion, and leave durable captures/monitors running |
| Restore saved session | Restore finite view/source/range references for inspection; revalidate media and obtain fresh authority before playback |

Coalesce navigation, not mutations. Bind readiness, stop and failure events to the exact request/generation so delayed responses cannot replace current listening state. Repeated keys cannot multiply connections, readers or charges. A failed replacement leaves an explicit stopped/failed state with retry rather than automatically trying more stations. Automatic station hopping, RF scanning/retuning, refresh, clicks, transmission and paid fallback are separate explicit capabilities. Direct live listening cannot masquerade as retained DVR or silently create a temporary capture grant.

Preserve original scripts and aligned text when available. English remains the default translation target; other targets require the versioned target contract and separate quality evidence. Reading an older cue suspends visible auto-follow without pausing capture. A packet source opens an event inspector, IQ opens a sample instrument, and only a declared audio derivative enables the player. Sonification has an explicit mapping and remains labeled separately from received audio. [Research42](../../research/42-calm-listening-and-receive-only-sources.md) records current hardware/protocol constraints.

Deliver first a focused read-only source/recording composition after reliable ordering, then the protected retained-reader and player lifecycle already specified by DV-01, then explicit direct-live replacement and captions under measured bounds. Device adapters follow file replay and actual receive-only hardware qualification. Acceptance includes compact/wide and linear views, multilingual wrapping, browsing while another source is audible, slow readiness, replacement cancellation, disconnect, retention race and background capture continuity. Inspect actual audio and terminal behavior when implemented; rendering tests cannot prove listening or RF silence.

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

The current Recordings view reads metadata only. Current CLI playback decodes one published segment and does not stitch a recording timeline; the service's live action parks a playhead at the newest published end. Interactive DVR controls and continuous retained playback remain planned. The [DVR packages](../development/reliability-and-scale.md#explorer-increments) establish service playback leases, bounded segment navigation and client controls before the interface claims a complete DVR.

The intended desk links station selection to explicit listen/capture, a zoomable retained timeline, original/target cues and findings. Keyboard navigation moves by time, segment, cue or bookmark; exact seek reports its media coordinate and availability. Caption selection can move the playhead, and a finding can open its frozen citation without starting new processing. Keep auto-follow visible and stop it while the user reads history. Current source selection, audible source, selected recording and active captures can differ and must remain named.

Define next-segment progression, codec transitions, decoder restart, bounded prefetch and cancellation through service-owned media access. Session/wait ceilings, buffer bytes and leased segment count remain finite. Playback reads only published retained media with a bounded set of protected segments and explicit retention status. At a gap, pause and explain it; an explicit skip moves to the next available interval and records the jump. Never close a gap by silently compressing the timeline or treating missing audio as received silence. At the latest sealed interval, distinguish waiting for more retained audio from separately authorized direct live listening; an open tail is not playable merely because capture is running.

Lease expiry, cancellation and client disconnect fence future reads and request decoder shutdown; they do not prove an existing read has ended. Release segment protection only after verified handle closure or decoder-group completion. Otherwise retain the bounded affected media/cleanup claim, expose the hold and refuse additional work when its ceiling is reached. Define service-owned ownership and restart reconciliation before implementation; elapsed time or a client assertion cannot establish safe deletion.

Power users need a recordings/schedules workspace with multiple-source state, retained ranges, capture budgets, processing lag and actionable failures. Selecting a range can protect available segments, save an interval or propose a bounded analysis task through the same service operations. Show exact boundaries, gaps, retention effects and admission outcomes before committing. A batch of selected sources needs finite per-source and aggregate bounds, idempotent receipts and truthful partial acceptance; saved filters and a highlighted group grant no background work. Reuse the civil scheduler, capture ledger and task/monitor policy rather than introducing a TUI scheduler.

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
