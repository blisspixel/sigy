# Product and experience

Status: proposed product specification. Confirmed scope is recorded in the [planning index](README.md).

## 1. Product purpose

Sigy provides practical signals intelligence for everyday people: discover a source, listen or inspect it, collect useful material, and turn observations into understanding. Persistent monitoring lets a user follow a topic across sources without keeping an interface open.

The core object is a source with a history of observations. Audio recordings, messages, transcripts, translations, and reports retain their connections to that history.

Most expected listening and music are non-English. Identify languages within content blocks and preserve original scripts, with English as the primary translation target. Exploration and play are also explicit goals: a satisfying radio explorer and a replayable Enigma or Morse experiment are valuable experiences in their own right.

The initial audience includes curious newcomers, listeners, hobbyists, and independent researchers running one installation. Prior radio, DSP, cryptography, or model expertise should not be required for supported everyday workflows. Shared teams, multi-tenant hosting, and fleets of remote receivers are possible expansions, not assumed first-release requirements.

### Breadth through coherent workflows

The product ambition is broad practical coverage. Maintain a representative task catalog spanning discovery, collection, decoding, understanding, comparison, monitoring, and experimentation. Mark each task as supported, conditional on a device/model/profile, planned, or outside current scope. The informal "90%" ambition is not a measured share of all signals-intelligence capabilities.

A new capability should fit the same journey: choose a source or artifact, choose an understandable action, inspect progress and results, then replay or save the evidence. Extension breadth must not produce a disconnected collection of tools with unrelated controls and libraries.

### Approachability contract

- Start with actions such as Listen, Record, Translate, Follow a topic, and Explore a signal. Explain specialist terms where they become relevant.
- Provide guided presets with visible source, resource, destination, and spending limits. Advanced controls remain inspectable and editable.
- Make missing prerequisites actionable: identify the needed model, decoder, or device and explain the next step without silently installing it.
- Present useful results early, with progressively available original material, uncertainty, timing, and processing detail.
- Make experimentation reversible through replay, saved settings, clear reset behavior, and separate practice sessions.
- Preserve equivalent CLI automation for experienced users. Friendly defaults and precise control should reinforce each other.
- Validate common journeys with people who did not design the system, including newcomers to radio and local models.

## 2. First complete release

| Area | Required experience | Completion evidence |
| --- | --- | --- |
| Explore | Search/filter refreshed station catalogs; browse a terminal globe, day/night map, or list; save favorites and collections; add a direct stream | Find and tune entirely by keyboard; preserve filters, freshness, location provenance, and usable fallback modes |
| Listen | Start, stop, mute, adjust volume, select output, see buffering and reconnect status | Device changes and failed streams recover without disrupting recordings |
| Collect | Record multiple streams; pause/rewind retained audio, return to live, save intervals, schedule sessions, and bookmark moments | Independent playheads and capture; bounded buffers, visible gaps, restart-safe schedules, and durable saved recordings |
| Translate | Identify languages within each block, show original transcript and live English translation by default, retain timestamps, process recordings later | The UI distinguishes mixed/unknown languages and provisional, final, delayed, unsupported, and failed processing |
| Monitor | Define a topic, choose or discover sources, run within limits, review findings and coverage | A report links each substantive finding to retained source material |
| Organize | Search captures and text; group by source, topic, date, language, and collection; export | A user can move from a search result to the original audio interval |
| Operate | Inspect jobs, storage, model availability, and failures from CLI or TUI | No active work depends on the continued existence of a terminal client |

Music identification and sampled airplay rankings are planned for after the first release. Hardware adapters follow the initial internet-radio release. A smaller explorer-only milestone does not satisfy the confirmed first-release scope.

Podcasts are prioritized alongside internet radio before hardware. Subscriptions, finite episodes and publisher transcripts/chapters share the source, artifact, language, queue and evidence contracts. RSS/Atom text analysis extends this path later; not every feed is an audio stream.

Morse, the historical cipher workbench, and modern cryptographic operations are confirmed planned capabilities with unresolved release placement. Their full design is in [Signal extensions and workbench](08-signal-extensions-and-workbench.md).

## 3. Source types

| Source | Primary observations | User experience | Special constraints |
| --- | --- | --- | --- |
| Internet radio | Audio, stream metadata, connection events | Tune, record, rewind within a retained buffer, transcribe, translate | Stream availability, redirects, playlists, codec changes, geoblocking |
| Local media | Existing audio plus import metadata | Import, replay, transcribe, translate, investigate | Source provenance and original capture time may be unknown |
| Podcasts | Finite episode media, show notes, optional supplied transcripts/chapters | Subscribe, queue episodes, replay, translate, follow topics | Feed/episode identity, media revisions, download/backfill limits, transcript alignment |
| RSS/Atom feeds, later | Entries, included text/summaries, update metadata and links | Subscribe, filter, translate, compare, monitor changes | Summary versus full content, bounded polling, duplicate/revised entries, separately permitted linked-content retrieval |
| Meshtastic | Messages, node information, positions, telemetry | Inspect channels and events, record a log, translate text, monitor topics | Device access, configured channels, duplicated packets, transport disconnects |
| Other LoRa systems | Protocol-specific packets or decoded events | Inspect using a supported protocol adapter | LoRa modulation alone does not define a message protocol |
| SDR | Complex samples, spectrum measurements, demodulated audio, decoded events | Tune, inspect spectrum, demodulate, capture, analyze | Device bandwidth, gain, sample format, exclusive tuning, storage rate |
| Timed or encoded signals | Keyed transitions, symbols, framed data, opaque bytes | Decode Morse, inspect timing/packets, retain for a later decoder | Input type, timing precision, decoder profile, error and uncertainty reporting |
| Imported signal data and practice sessions | Recorded IQ/events or synthetic observations | Replay, compare decoder revisions, practice, inspect cipher traces | Recorded versus synthetic provenance; capability and resource limits |

Capabilities are explicit. A text source offers text translation directly. An SDR source offers audio transcription only when a demodulator produces audio. Unknown packets remain unknown; an unavailable decoder is a visible capability limitation.

## 4. Essential user journeys

### 4.1 Explore and listen

1. Open the TUI into Explore, with current service status and active jobs visible.
2. Browse region and country, or search a name, language, or tag.
3. Inspect station details, including the source of directory metadata and its age.
4. Tune deliberately. Moving the selection does not open a network stream.
5. See connecting, buffering, playing, reconnecting, or unavailable states.
6. Favorite the station or add it to a named collection.
7. Start recording without restarting an existing compatible capture pipeline.

Directory data and observed stream health are separate. A station listed as healthy by a directory can still fail locally. The UI reports both accurately.

### 4.2 Record several sources

Choose stations, duration or schedule, retention policy, and optional processing. Review expected bandwidth, storage, and whether the machine can keep up. Submit durable jobs, receive their IDs, and close the client. Reopen later to see captured duration, missing intervals, processing backlog, and completed results.

Stopping speaker playback does not stop a recording. Stopping one monitor does not terminate a shared capture still needed by another monitor. The UI shows these relationships before a user stops shared work.

### 4.3 Follow a live translation

Select automatic language detection or a scoped override, target language (English by default), and processing profile. Display audio position, observed language spans, incoming original text, translation, and delay. Let the user pin a section to inspect it while capture continues, then return to the latest text.

The initial interpretation is translated captions. Spoken translated audio is a separate decision because it adds speech synthesis, audio mixing, and additional delay.

Original words remain available beside translations. Revised captions are visibly revised. Reports consume finalized text by default. A missing model or unsupported language pair produces a clear state with queued work, rather than an empty pane that appears healthy.

Directory language is a hint. A French station can contain an Arabic interview and an English advertisement; a block may contain multiple languages or too little speech to identify. Display the available detection granularity honestly. Silence, instrumental music, an unknown language, and a failed detector are different states. Translation updates reference the original revision rather than replacing it.

### 4.4 Investigate an existing recording

Open a session, inspect its timeline and gaps, play an interval, search its transcript, edit a transcription as a new revision, request another translation, and create a note with a source reference. Reprocessing preserves previous outputs and their model versions.

Export an evidence bundle containing selected media, machine-readable metadata, original and translated text, and the generated report. An export states when referenced audio has expired or was never available.

### 4.5 Monitor French news topics

Interpret the request into explicit fields: topic, station geography, spoken languages, time window, time zone, source selection policy, capture budget, processing budget, and output cadence.

France-based stations, French-language stations, and news about France are different filters. The task preview shows the interpretation. Once a monitor is activated, it may discover and adjust sources automatically inside the user's source, time, and resource limits. Each adjustment records its reason and its effect on coverage.

During collection, show stations attempted, stations successfully recorded, minutes captured, minutes transcribed, and the languages covered. Group recurring stories while preserving source distinctions and contradictory accounts. Repeated syndicated broadcasts should not be treated as independent confirmation.

A briefing describes what was observed in the monitored sample. It reports the collection window and missing coverage. An empty result means no supported finding from the collected evidence, not that the event did not happen.

### 4.6 Discover music across African radio

Clarify country selection, genres, languages, observation window, and whether the requested output is sampled airplay, editorial discovery, or an external chart.

Collect station-provided track metadata and, where supported, fingerprint observations. Preserve detection method, uncertainty, recording offset, and catalog identifier. Reconcile alternative spellings, remixes, repeated segments, and duplicate station feeds.

Provide play count, distinct stations, countries represented, monitored hours, and identification coverage. A sampled airplay ranking must identify its denominator and scope. General popularity requires additional representative data. Unidentified tracks remain unidentified.

Evaluate predominantly non-English music and regional catalogs. Preserve original-script artist and track names alongside optional aliases/transliterations. A fingerprint match identifies a recording; it does not establish sung language or general regional popularity. Report unidentified airtime and avoid inflating rankings by discarding poorly covered catalogs.

### 4.7 Add hardware later

Connect a device, inspect capabilities, run a connection diagnostic, select an appropriate receive profile, and begin a session. Unplugging the device creates a visible interruption and an explicit reconnect policy. Existing internet streams continue.

Hardware support is demonstrated on actual devices before being labeled supported. File replays and simulated devices allow earlier development without claiming physical compatibility.

### 4.8 Explore a signal or cipher

Open a retained artifact or a clearly labeled demo in Workbench. Inspect Morse timing, compare uncertain decoded symbols with the original, or step through an Enigma character while watching rotor state and its signal path. Save and replay the experiment. Adjustments create reproducible revisions.

Modern cryptographic workflows use supplied key references and supported operation profiles. They show encryption, authentication, signature, or key-establishment results with distinct semantics. Historical demonstrations and operational keys have separate handling. These workflows are detailed in [Signal extensions and workbench](08-signal-extensions-and-workbench.md).

## 5. TUI information architecture

| View | Main content | Primary actions |
| --- | --- | --- |
| Explore | Globe/world map/list, day/night, refreshed station results, filters, favorites, source details | Rotate/pan, filter, inspect, tune, favorite, collect, start monitor |
| Live | Active sources, DVR timeline, audio/activity visualizers, captions, health and delay | Pause/seek retained audio, return to live, save interval, switch audible source, mute, bookmark, stop a job |
| Library | Captures, text search, timelines, notes, exports | Replay, transcribe, translate, organize, export |
| Monitors | Topics, schedules, source policy, coverage, run history | Create, inspect plan, pause, resume, revise limits |
| Findings | Briefings, topic clusters, evidence references | Open evidence, compare sources, correct, export |
| Workbench (planned) | Replayable signal views, Morse practice, historical cipher traces, modern cryptographic operations | Inspect, step, compare, reset, save an experiment |
| System | Service, providers, storage, devices, diagnostics | Configure, test connection, inspect queue, recover |

Use a persistent status area for service connectivity, active captures, processing lag, and storage pressure. Keep source health, capture health, and analysis health distinct.

Proposed wide-terminal layout, with illustrative placeholders rather than actual results:

```text
SIGY   Explore  Live  Library  Monitors  Findings  System
Service: connected   Captures: 3   Analysis: 8 s behind
+----------------+-----------------------------+-------------------------+
| Regions        | Search and filters          | Selected source         |
| Favorites      | Station / Country / Language| Directory information   |
| Collections    |                             | Observed health         |
| Recent         | Station results             | Tune / Record / Monitor |
+----------------+-----------------------------+-------------------------+
| Listening: selected station   Volume: 60%   Recording: active         |
| Original:   timestamped speech                                        |
| Translation: timestamped text   Status: provisional                    |
+----------------------------------------------------------------------+
Tab focus   / search   Enter open   ? help   Command palette
```

Explore includes a rotatable terminal globe and flat world-map mode with day/night context, backed by the same source query as the list. Lists and filters remain fully usable when geographic rendering is disabled or dimensions are small. The globe belongs to the planned TUI experience. Detailed interactions, freshness, visualizer meaning, and DVR semantics are specified in [Complete CLI, terminal explorer, and radio DVR](11-radio-explorer-and-dvr.md).

### Interaction contract

- Every core action is keyboard accessible and discoverable through contextual help or a command palette.
- Focus, selection, audible source, and recording source are separate visible states.
- Global shortcuts do not consume characters while typing into a field.
- Navigation preserves filters and selection. Long lists and transcripts are virtualized.
- Search shows loading, stale-cache, empty, and failed states distinctly. Old responses cannot replace newer searches.
- Small terminals switch to a focused single-pane layout. Resizing preserves work and focus.
- A plain output mode and optional ASCII presentation support limited terminals. Color never carries the only status information.
- Unicode grapheme handling, text selection, full-width characters, and right-to-left content require dedicated terminal tests.
- Original-script names and captions remain usable alongside English translation. Animation can be paused or disabled and cannot consume capture resources without bounds.
- Mouse support is optional. Copyable station URLs, job IDs, and timestamps remain accessible without a mouse.
- Closing the TUI detaches. An explicit service-stop action names the jobs it affects.
- Destructive library actions show their scope; retention policies operate according to the user's saved policy.

## 6. CLI contract

The CLI is a complete automation surface over the same operations as the TUI. Proposed command families:

| Family | Proposed operations |
| --- | --- |
| `sigy` | Show useful help/status; do not implicitly enter a full-screen interface in automation |
| `sigy tui` | Explicitly open the optional interactive terminal client |
| `sigy station` | Search, filter, inspect, refresh catalog, favorite, manage collections, add direct streams |
| `sigy listen` | Start or control an interactive playback session |
| `sigy playback` | Inspect session/retained ranges, pause, seek, return to live, choose output, detach |
| `sigy capture` | Start, list, inspect, stop, schedule, export |
| `sigy schedule` | Manage recording rules, next occurrences, conflicts and occurrence history |
| `sigy transcript` | Create, follow, search, inspect revisions, export |
| `sigy translate` | Translate an existing session or follow a live source |
| `sigy monitor` | Plan, create, run, inspect, pause, resume, revise |
| `sigy findings` | List briefings, inspect evidence, compare, export |
| `sigy provider` | Configure, inspect capabilities, test, select task defaults |
| `sigy device` | List, inspect, diagnose; hardware operations arrive later |
| `sigy service` | Install, start, status, stop, logs, uninstall integration |
| `sigy doctor` | Diagnose configuration, dependencies, permissions, storage, and model availability |
| `sigy library` | Search, retention preview, backup, restore, integrity check |
| `sigy source` / `sigy decode` | Future typed source inspection and decoder operations beyond stations |
| `sigy workbench` / `sigy crypto` | Future saved experiments and operation-specific cryptographic workflows with protected key references |
| `sigy podcast` | Local subscribe, unsubscribe, one RSS 2.0 refresh, offline episode listing, one explicit enclosure download, `listen file` playback of that retained recording, and one explicit publisher transcript or chapter fetch are implemented. Bounded backfill, queued processing, and export remain later |

Illustrative workflows:

```text
sigy station search --country FR --language fr --tag news
sigy capture start <station-id> --duration 1h
sigy transcript follow <capture-id> --translate-to en
sigy monitor plan "Follow energy policy coverage on French radio"
sigy monitor create --from-plan <plan-id>
sigy monitor inspect <monitor-id> --coverage
sigy findings show <briefing-id> --with-evidence
```

Automation requirements:

- Stable IDs, versioned JSON output, and newline-delimited event output for follow commands.
- Structured results on stdout; diagnostics and progress on stderr. No terminal controls in machine output.
- Documented exit codes for usage errors, unavailable service, rejected jobs, partial results, and terminal failure.
- Submission and completion are separate: a successful job submission returns an ID, not a false claim that recording completed.
- `--wait` and bounded timeout options for callers that need final results.
- Repeated submissions with the same idempotency key return the same accepted job.
- Noninteractive commands never block on an unexpected prompt. Conflicting or missing options produce actionable errors.
- Interrupting a following client detaches by default. Stopping a durable job requires an explicit operation.
- Shell completions, searchable help, configuration discovery, and examples for supported shells are release requirements.
- Maintain a TUI-action/application-operation/CLI-command parity matrix. Complete all functional setup and operational journeys without the TUI; presentation-only camera changes do not require an animation equivalent in machine output.

## 7. Settings and first use

First use identifies the library location, available audio support, service mode, storage policy, and optional model providers. Radio exploration must work without configuring a model. Analysis capabilities are enabled only when their dependencies are ready.

Once a qualified local profile is configured, a zero paid budget still permits local language detection, transcription, translation, and analysis within machine limits. Show captured streams separately from live processing slots and queued audio. A monitor's overview offers current findings, changes, coverage, and replayable evidence. Optional semantic-classifier controls belong in advanced settings. [Detailed analysis experience](10-analysis-and-knowledge.md).

Model downloads show size, source, license, and storage location before starting. The application does not silently download large assets, change a provider's global settings, or switch a local job to a remote model.

Provider configuration distinguishes local process, loopback service, user-managed LAN server, and hosted service. A loopback URL alone does not establish that inference stays on the machine.

## 8. Product boundaries requiring decisions

The following are deliberately not presented as settled:

- Whether live translation includes synthesized speech.
- Required behavior before login and after logout, beyond surviving client closure.
- Qualified launch language/task profiles and representative station sets, with a majority non-English evaluation corpus.
- Maximum simultaneous capture and translation load on target machines.
- Remote control of another Sigy installation, beyond future architectural compatibility.
- Publishing model-generated output or delivering it to external accounts.
- Default retention, handling of pinned evidence, and deletion of derived text.
- Distribution model and long-term platform support window; Apache License 2.0 is confirmed.
- Workbench/Morse/cryptography release placement, initial cipher/operation profiles, and whether archive encryption is a separate deliverable.
