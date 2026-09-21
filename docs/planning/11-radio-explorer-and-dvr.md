# Complete CLI, terminal explorer, and radio DVR

Last updated: 2026-09-20. Status: proposed detailed design for the requested terminal experience. No implementation or stack selection.

## 1. Experience

Sigy should feel like a world listening desk with a dependable recorder behind it. A user can rotate a terminal globe, switch to a flat day/night map, search and filter stations, inspect a source, listen, rewind retained audio, save a passage, and follow translated text while other stations continue recording.

The CLI is complete in its own right. The TUI is an optional interactive client over the same operations. A user can set up providers, refresh sources, manage favorites, record, schedule, process, monitor, inspect failures and costs, export, and recover work without opening the TUI.

The first-release radio experience includes searchable station lists, freshness management, a terminal globe/world-map view, basic truthful audio/activity visualizers, and the defined DVR workflow. Exact rendering tiers and buffer defaults require qualification. RF spectrum/waterfall views follow the hardware roadmap; full programme-guide integration is a separate future capability.

## 2. Explorer layout and interaction

Use three linked presentations of the same source query: list, globe, and flat map. Filters and selection persist between them. A wide terminal can show geography, station results, and source details together. A compact terminal can show one panel at a time while retaining the selected source and query.

Conceptual layout with placeholders, not an implemented screen:

```text
SIGY  Explore  Live  Recordings  Monitors  Findings  System
Sources: captured 8 | transcribing 2 | queued 6 | paid disabled
+--------------------------------+-------------------------------------+
| Globe / World map / List       | Search: [                          ]|
|                                | Country  Language  Tags  Health     |
| Rotatable Earth                | Filtered stations and source state  |
| Day/night shading              | Favorites and saved searches        |
| Station and active-work marks  |                                     |
|                                | Selected source: location and basis |
| View time: now, UTC            | Listen  Record  Translate  Monitor  |
+--------------------------------+-------------------------------------+
| Playback: 02:10 behind live | buffer: 30 min | capture continues       |
| Timeline: retained audio, gaps, bookmarks, speech/music, transcripts  |
| Original text                         | English translation          |
+----------------------------------------------------------------------+
Keyboard controls | command palette | costs | queue | directory age
```

The displayed counts and buffer duration are illustrative. Collection and analysis do not begin because a station becomes highlighted or a map rotates.

Proposed interactions:

- Arrow keys rotate/pan when the map has focus; a key switches map/list focus. Search fields retain ordinary text editing.
- Zoom changes detail and marker clustering. A cluster opens its station list without tuning every member.
- Optional mouse drag rotates the globe. Keyboard controls provide equivalent navigation.
- A bounded spin animation gives the globe a sense of movement; pause and reduced-motion controls are always available. Selecting a station can center it with optional animation.
- Enter opens source details. Listen is explicit and separate from Record. Browsing never interrupts another acquisition.
- Saved searches retain query intent and filters. Favorites retain source identity and user labels across catalog updates.
- An optional discovery action chooses a candidate within the visible filters. Audible scanning is a separately started, bounded session and never starts recording or paid analysis implicitly.

Core filters include name, country/region, declared language, tags/category, source type, favorites/collections, available format, and health/freshness. Observed language, current activity, retained recordings, and monitored topic are separate filters with explicit time windows. A country filter and a spoken-language filter must not be conflated.

Search reports whether results are cached, refreshed, partial, or unavailable. Unknown health/location/language remains a valid discoverable state. Visible/map-only results do not silently replace the full filtered result set; viewport filtering is an explicit option.

## 3. Globe, day/night map, and activity

Use an offline basemap candidate and local scene computation so the explorer requires no tile account or paid map requests. Text/Braille/block rendering, ASCII, and list modes are capability tiers. The same feature is being designed for the terminal; a future graphical client is not a prerequisite.

The globe displays a hemisphere with occlusion, geographic rotation, and cell-aspect correction. The flat map provides a worldwide overview. Coordinate origin, precision, and age travel with each marker. A directory's station location is not automatically the transmitter location; a streaming CDN address is not the broadcaster's origin. Unknown locations stay in an unmapped group, and coarse locations carry a visible approximation label.

Day/night shading uses an explicit UTC instant. Live, frozen, and historical view times are clearly labeled; changing the map time does not change service schedules. Geometric sunlight, optional twilight, and actual local weather are distinct concepts. The initial overlay is contextual, not a reception or propagation forecast.

Changing only the solar-view time does not replay activity history. Live activity badges remain labeled live unless a separate historical-activity filter is selected. Historical activity uses retained events and the location metadata applicable to those events, with gaps exposed; current directory metadata must not fabricate a historical source position.

Activity layers can show selected sources, audible playback, captures, processing, queue delay, and failures. Shapes/letters and a legend supplement color. A source may have several simultaneous states. Pulses indicate real state changes and remain optional. Any connection line represents a labeled logical association, not an inferred RF path or measured internet route.

The world view is not an automatic detector of unknown transmitters. It presents known source metadata and observed work. Receiver positions, mesh-node positions, and processing-host locations require their own provenance and privacy settings. User location is optional and must not be inferred merely to make the globe look populated.

## 4. Visualizers with explicit meaning

| View | Required input | Meaning and limitations |
| --- | --- | --- |
| Level meter / waveform | Decoded audio samples | Audio amplitude over a stated interval; not radio signal strength |
| Audio spectrum | Decoded PCM and analysis settings | Audio-frequency content; no claim about RF occupancy |
| Activity and language timeline | Source and processing observations | Captured intervals, gaps, detected speech/music, languages, transcript progress and delay |
| Processing view | Service metrics | Queued work, live/batch throughput, worker state and cost; not fabricated signal activity |
| RF spectrum / waterfall, later | IQ or qualified receiver measurements | Frequency, tuning epoch, gain/calibration, bandwidth, time and dropped samples |
| Packet/event view, later | Typed decoder output | Message/event timing and supplied telemetry; no invented waveform for non-audio data |

Reuse compatible decoded samples and bounded summaries. Visualizer subscribers cannot stall acquisition, retain unbounded history, or require the service to stream every raw sample to a slow terminal. Stale or unavailable measurements produce an explicit state rather than a decorative animation presented as real data.

## 5. Directory lifecycle

Store catalog metadata separately from user choices, historical source configurations, and local playback observations. Refresh on a bounded saved policy with a manual refresh operation. Provider failure preserves the last usable catalog and makes its age visible.

Refresh workflow: fetch bounded pages or supported changes, validate records, reconcile identities, publish a new usable generation, and update search indexes. Incomplete scans do not authorize bulk deletion. Periodically reconcile beyond recently changed listings so missed updates do not persist forever.

Retain renamed/unavailable favorites and historical aliases. Conflicting directory records need provenance. A new station URL is a candidate configuration revision, not permission to silently redirect an existing recording. Use it on a validated future acquisition or explicitly governed reconnect and record the transition.

Separate catalog freshness, upstream health checks, local connection success, and observed content freshness. A directory update is not a programme guide. Respect provider request limits; deduplicate refresh work and avoid probing every known stream merely because the TUI is open.

## 6. DVR contract

### Rolling retention and playback

Enable a finite rolling buffer through the user's saved playback/capture policy. Show its duration/byte allowance and retained range. Merely searching stations does not buffer their audio. Browsing away or closing a client follows explicit ownership rules for the temporary buffer; durable recordings continue independently.

Each playback session has its own playhead. Pause stops audible progression, while an authorized acquisition can keep collecting. Rewind/seek operates only within retained intervals. Return to live follows the newest playable captured position, with upstream buffering and local delay distinguished where measurable.

Show gaps and partial segments. If a paused position expires, mark it unavailable and offer the earliest retained point or live playback. A live stream cannot supply audio from before Sigy began collecting unless a separately qualified catch-up source actually provides it.

Captions follow the chosen mode: playback position for listening, or live incoming text for monitoring. Label the mode and transcription/translation delay separately from audio delay. Seeking into untranslated audio shows pending/unavailable text, not a caption from a different moment.

### Save and record

Users can save a retained interval, begin a recording now, or include a specified available pre-roll. Saving reserves storage and atomically protects segment references against expiry. Shared objects are counted correctly across rolling windows, recordings, monitors, and evidence pins. Clipping boundaries and missing intervals are explicit.

Do not duplicate upstream downloads for compatible playback and recording. Do not stop a shared acquisition when one client detaches. Conversely, a temporary listener does not authorize indefinite background storage. Ownership, stop rules, and retention remain inspectable from both clients.

### Scheduling

Provide one-time and recurring station/time recording rules, next occurrences, optional pre/post margins, and conflict previews. Store the intended time zone and resolved occurrence instants. Define behavior for daylight-saving gaps/overlaps, sleep, missed occurrences, restart, and simultaneous jobs before release. Do not record a past live broadcast retroactively after downtime.

Schedules can attach analysis profiles and finite resource/paid policies. Changing or deleting a rule states whether already accepted occurrences change. Repeated startup/reconciliation must not duplicate the same occurrence. An absent programme guide does not block station/time recording.

## 7. CLI completeness

Command names are proposals. Bare `sigy` should provide useful help/status; `sigy tui` explicitly opens the optional full-screen client. Scripts must never enter it because terminal detection changed.

| Workflow | Proposed CLI surface |
| --- | --- |
| Search, filters, favorites, collections, direct sources | `station search/show`, station/collection management |
| Refresh and inspect catalog state | `station refresh`, catalog status and provenance |
| Listen, pause, seek, return to live, detach | Playback session operations with explicit session IDs |
| Start/stop recording, save buffered interval | Capture operations and retained-range selection |
| One-time/recurring recording | Schedule create/list/inspect/revise/disable and occurrence history |
| View geography and solar/activity information | Source-coordinate/time queries, optional one-shot map, structured observation output |
| Transcribe, translate, classify, monitor, export | Existing model, monitor, library and evidence operations |
| Inspect providers, costs, storage, service, failures | Existing administrative operations with identical policy enforcement |

A parity matrix must list every TUI action's application operation and CLI equivalent. Pure presentation actions, such as rotating the current camera, need not be simulated in JSON; the source selection, coordinates, time and activity data they expose must be available. All meaningful mutations have noninteractive options, structured results, stable IDs, errors, cancellation, and documented exit behavior.

SSH supports service control and terminal display. Audible output requires an audio device in the selected playback context; streaming audio back to another machine is a separately configured capability. The CLI must make the playback destination clear.

## 8. Acceptance

Evaluate the globe/map/list journeys on supported OS and terminal profiles, with keyboard-only input, monochrome, reduced motion, small windows, Unicode/RTL names, resize, and slow remote links. Measure navigation latency, frame/output budget, CPU, memory, and capture integrity during simultaneous local inference. Render at a bounded cadence and reduce visual work before compromising capture.

Test fixed-date solar geometry, polar/dateline behavior, missing coordinates, clustering, state legends, and map/list query agreement. Test refresh pagination/failure, renamed sources, stale responses, changed URLs, and preserved favorites.

DVR acceptance includes pause/expiry, simultaneous clients, buffer-to-recording promotion races, seek gaps, clock changes, duplicate schedule prevention, storage pressure and crash recovery. Run complete station-to-recording-to-transcript-to-monitor journeys entirely through the CLI as well as the TUI. [Research and proposed experiments](../../research/18-terminal-explorer-and-radio-dvr.md).
