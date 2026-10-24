# Changelog

## Unreleased

- Repository instructions now require a session-start check of worktrees, branches, and pull requests. A worktree or branch is removed only after its work is on `main` or pushed, and not with `--force`. This does not change the program. Catalog schema stays v54. Local IPC stays v58.
- `sigy claim candidates` lists a retained cue that no stored finding cites. The accept line names `--transcript`, `--revision`, `--translation`, and `--cue`. The search writes nothing. A cue that is not retained is omitted. A hit that a finding already cites keeps the finding accept line. Nothing is fetched and no score is set. Catalog schema stays v54. Local IPC is v58. Issue 14 stays open.
- `sigy claim sighting` can link one retained cue without creating a finding. Pass `--transcript`, `--revision`, `--translation`, and `--cue`, and do not pass `--monitor` or `--finding`. The same citation changes nothing. A missing cue, or a cue that is not retained, writes nothing. A later expiry does not rewrite the stored `retained` state. `show` labels a newer transcript or translation stale. `claim candidates` still lists only hits that a stored finding cites. Nothing is fetched and no score is set. Catalog schema is v54. Local IPC is v57. Issue 14 stays open.
- In Recordings, `L` then `y` plays the newest playable sealed interval on the loaded map. The listen starts at that interval and passes no `--end-us`, so the existing handoff can continue. The open object is skipped when an earlier sealed interval can play. A whole-segment listen already at or after that start is not restarted. A cited listen is replaced. The exclusive end is not a seek target. Capital `H` still pauses. Prefetch and a free-form interior editor stay later. Catalog schema stays v53. Local IPC stays v56.
- In Recordings, `K` then `y` records one hold on the selected sealed interval through `record hold`. The open object, a released interval, a gap, an empty length, and a partial map record nothing. A repeated confirmation after the reply records another hold. Capital `H` still pauses. The request starts no playback. The CLI command remains the wider path and can record a gap. Prefetch, a free-form interior editor, and a return to the latest playable position stay later. Catalog schema stays v53. Local IPC stays v56.
- In Recordings, `,` and `.` choose one sealed interval already on the loaded map. `I` then `y` plays that interval's own half-open bounds with one `listen file`. A start above zero passes `--seek-us`. The exclusive end always passes `--end-us`. The open object, a released interval, a gap, an empty length, and a partial map start nothing. `P` then `y` still plays one whole retained recording that is not capturing. A ranged listen does not hand off. Prefetch and a free-form interior editor stay later. Catalog schema stays v53. Local IPC stays v56.
- A completed whole-segment retained listen waits for the next sealed segment. One recording show runs each second for at most 30 minutes. A sealed interval starts one `listen file` at that exclusive end. The open tail starts no decoder. A gap, a released edge, or a capture that is no longer running starts nothing. `H`, `[`, `]`, `S`, quit, and a new confirmed play cancel the wait and do not call `listen pause` or `listen seek`. Prefetch stays later. Catalog schema stays v53. Local IPC stays v56.
- `podcast refresh` accepts one RSS 2.0 or Atom document. An entry `id` is a publisher guid. `published` fills the existing publication time, and `updated` fills it only when `published` is absent. A direct child enclosure link is stored. Summary, content, and alternate links are not stored and are not requested. Catalog schema stays v53. Local IPC stays v56. Issue 13 stays open.
- `sigy claim candidates` lists analysis-search hits that a stored finding already cites. It writes nothing. The user accepts with the existing `sigy claim sighting` command. A current search uses the newest translation. `--history` adds an older translation only when the term is in that older English text and not in the original, and that hit is labeled stale. Findings `c` opens stored claims. That page only reads and does not start playback. `claim_list` and `claim_show` remain the only claim MCP tools. Catalog schema stays v53. Local IPC is v56.
- `sigy claim show` reports when the evidence cited by a status needs review. The stored status, its revision, and its evidence id stay in place. Review names a later correction, a newer recognition or translation, missing media, or retention expiry for that finding only. Unverified stays `none`. Nothing is fetched and no truth or credibility score is set. `claim_list` and `claim_show` only read. Catalog schema stays v53. Local IPC is v55.
- Tokio moves from 1.53.1 to 1.53.3 on the same 1.53 line. That patch clears RUSTSEC-2026-0386, the Unix `AsyncFd` constructors. Sigy does not call those constructors. No other locked crate moves.
- A recognition or translation finish stores its publication body before the first finish commit. Restart publishes a matching body through the existing checks and starts no new worker. A body that cannot be read fails that job with `held-completion-unreadable` and inserts no cue. Process exit before the hold commit still drops the outcome. S-03B stays open. Catalog schema is v53. Local IPC stays v54.
- A completed whole-segment retained listen starts one `listen file` at that segment's exclusive end. The end comes from the reader receipt. A cited listen does not continue. No prefetch starts while the child is still running. Exit with no decoder sample keeps that edge and writes no reader. Capture keeps running. The desktop desk does not gain Play or Stop. Schema v52 and local IPC v54 are unchanged.
- `[` and `]` seek one retained listen by ten seconds inside its admitted range. A step that would leave the range does not stop the child. The new `listen file` keeps a cited end and does not carry the finding citation. Exit with no decoder sample keeps the previous position and writes no reader. Child exit is not a seek. Capture keeps running. The desktop desk does not gain Play or Stop. Schema v52 and local IPC v54 are unchanged.
- Capital `H` pauses one retained listen at the latest decoder sample. The next `H` starts one `listen file` at that recording-timeline position. A finding resume keeps the cited end. The new reader receipt does not carry the finding citation. A missing sample, a position outside the admitted range, and a second press while the client is still stopping write nothing. Child exit is not a pause and does not prove silence. Capture keeps running. The desktop desk does not gain Play or Stop. Schema v52 and local IPC v54 are unchanged.
- `sigy claim` stores a claim you write and links it to a finding that is already stored. Revisions append and do not erase history. A sighting that is not a stored finding is refused. Evidence and status are explicit, unverified is the absence of a status row, and the timeline is stated before the sightings. Carriers leave repetition and independence unknown. Nothing is fetched and no truth or credibility score is set. `claim_list` and `claim_show` only read. Catalog schema is v52 and local IPC is v54.
- The Windows installer resolves `git`, `cargo`, `gh`, and `ffmpeg` by walking `PATH` and checking that the file exists. The walk skips Windows application-execution aliases and stops after 256 directories. A missing FFmpeg program still prints the trusted-executable hint. Schema v51 and local IPC v53 are unchanged.
- A retained listen reports decoder output time on the `Retained:` row as `decoder N us`. `N` is FFmpeg output time from the admitted start, written to stderr as `decoder-timeline-us N` at most once per second, plus a later final frame. The number does not move the metadata playhead and does not prove audible output. Pause, seek, and segment handoff stay later. Schema v51 and local IPC v53 are unchanged.
- Recordings and Findings can start one retained listen after `P` then `y`. The action is one whole retained recording that is not capturing, or one cited finding with a stored interval. Enter, selection, filters, reload, and Back do not start it. A retained listen adds one plain `Retained:` row and does not replace the listen-session row. The row is omitted while idle. Pause, seek, and segment handoff stay later. `listen file` and `listen finding` accept `--cancel-on-stdin` and `--parent-pid`. Schema v51 and local IPC v53 are unchanged.
- `sigy panel desk` is a read-only keyboard surface for service, directory, favorite, capture, and listen state. Selection does not play. Open full Sigy prints `sigy tui` and does not spawn it. The listen station stays unknown until a catalog lookup exists. Play and stop remain the existing commands. Qualification flags stay false. Schema v51 and local IPC v53 are unchanged.
- Repository instructions lead with purpose, routing, ownership, invariants, and verification. Dated increment history stays in the decisions and in active work. The visual QA gallery count in those instructions is 78 frames. Schema v51 and local IPC v53 are unchanged.
- The README first screen shows a connected globe, a selected station, and a listen in progress, with light and dark PNGs. Install commands come before that image. `cargo readme-hero` regenerates both files from the production renderer. The frame is synthetic. It is not a live station, a coverage survey, or acoustic proof. Schema v51 and local IPC v53 are unchanged.
- The [analysis reliability repair](docs/decisions/0092-atomic-worker-responses.md) preserves exact translation history on replay and prevents a failed response read from leaving a committed claim or result. Automatic recovery of an unsettled worker result remains the next reliability increment.
- A new Linux library uses the XDG data directory, `$XDG_DATA_HOME/sigy/library` or `~/.local/share/sigy/library`. The default library's socket uses `$XDG_RUNTIME_DIR/sigy` when that directory exists. Update state and the source checkout use the XDG state and cache directories. An existing `~/.sigy/library` stays until `sigy init --move-xdg`, and `sigy doctor` names that command. Decision 0100 records the contract. Windows and macOS keep `~/.sigy/library`. Schema v51 and local IPC v53 are unchanged.
- Commands exit 0 when they complete, 1 for another failure, 2 for usage, 3 when the background service is unavailable, 4 when the service refuses the job, and 5 when a task result is partial. An external stop exits 130. `--json` errors include `error`, `code`, and `next` when a next command is known. `sigy --help` lists the codes. Schema v51 and local IPC v53 are unchanged.
- `sigy tui` needs a terminal on stdin and stdout. A pipe, a redirect, or `TERM=dumb` prints one plain line and `Next:` with the current search command, then exits 1. That path writes no escape sequence, does not open the library, and does not change terminal modes. A zero-size terminal is rejected before raw mode. Normal quit, recoverable failure, panic, and an external stop restore the cursor and the main screen. An external stop exits 130. Schema v51 and local IPC v53 are unchanged.
- On the Globe canvas, arrow keys turn the view. A step, center, and explore ease along the short path for 400 ms. Spin ramps to 6 degrees per second over 400 ms and eases off when `r` stops it. A startup flourish shows Sigy and turns back once, when the canvas can hold it. The playing station can show a steady or slowly pulsing marker. Each frame begins and ends synchronized output, and that mode is cleared on exit. Reduced motion, linear, monochrome, pipes, and CI stay still. Schema v51 and local IPC v53 are unchanged.
- `sigy --help` keeps the journey guide and shows `Usage: sigy [OPTIONS] <COMMAND>`. A blocked native-containment check names the operating-system cause and says the limits are unsupported on that host. Schema v51 and local IPC v53 are unchanged.
- The explorer keeps a short chrome. At 80 by 24 the strip is the connection, the workspace tabs, a live row only while playback or a recording is active, a plain `Retained:` row only while a retained listen exists, and one hint line. The hint lists key actions and ends in `? more` when that tail fits. Below 80 columns the tabs stay short and every workspace stays reachable. At 132 by 40 the station list and the selected station sit side by side. Quota stays in System and appears in the header only when little space remains. Status text keeps its tail. A catalog restart keeps `g first page` visible. Schema v51 and local IPC v53 are unchanged.
- Doctor, search, explore, and the explorer share one next command. An empty cache suggests `sigy radio search --fetch`. Sigy generates the fetch id. A passed `--data-dir` is included in the human command. The doctor report leads with blocked and attention counts, uses short labels, and groups idle checks. `sigy --help` groups commands by journey and wraps to the terminal width. `?` lists the focused workspace keys. Typo suggestions use a distance of 1 or 2 and add no completion crate. Schema v51 and local IPC v53 are unchanged.
- Semantic color uses one role table. Live playback is green, failure stays red, and the recording marker is bright red. A favorite is a bright blue star, or an asterisk in linear and monochrome modes. Field labels use the default foreground. Accent marks the brand, the current workspace, the focused filter, and the selected map marker. `--color=auto|always|never` and `--high-contrast` follow `NO_COLOR`. Linear keeps color unless monochrome is also on. High contrast bolds the default foreground, keeps failure and live color, and reverses focus. Unmeasured doctor text says `not measured yet`. Panel status text stays uncolored. The desk uses the same role slots. The globe rim uses the muted slot; the outline remains the braille circle. Official Catppuccin Latte and Mocha examples miss contrast for Latte green at 2.96, Latte yellow at 2.31, and Mocha muted and rim at 2.46. Those ANSI slots stay. Schema v51 and local IPC v53 are unchanged.
- Human copy uses one vocabulary: play, fetch, inspect, favorite and first page. Search, explore, the panel and the explorer share one cache count. Library status and budgets show compact exact USD, with sub-cent digits kept and no floating conversion. Recording timelines lead with `m:ss` and keep exact microseconds in the detail line. Block marks and the plain ASCII fallback classify the same cells. Command and flag names stay. Schema v51 and local IPC v53 are unchanged.
- The pinned toolchain is stable Rust 1.99.1. The installer and verify workflow use that release. `spawn_with_attributes` remains nightly-only, so creation-time Job Object assignment stays the Decision 0099 limitation.
- The Linux CI decoder fixture retries each pinned FFmpeg download three times and deletes a partial file after a failed attempt. The same SHA-256 and release signature still run once. This fixture is not installed by Sigy.
- Windows native workers still join their Job Object after a suspended `CreateProcess`. Creation-time assignment stays a documented limitation. The stable Rust command API does not expose `PROC_THREAD_ATTRIBUTE_JOB_LIST`, and ProcessKit 3.3.4 assigns only after the process exists. Decision 0099 records that review. Schema v51 and local IPC v53 are unchanged.
- Recognition and translation finish tokens carry the lease owner stored by their committed claim. Finish refuses a token whose attempt or owner differs from that row and publishes nothing. A drained outcome waits in a completed slot. Busy and locked settlement is retried at most three times within 15 seconds, using the existing five-second catalog lock wait. Uncertain I/O is not published again while that claim is still running. Exhaustion drops the slot and leaves the running or cancelling row. Process exit still loses the in-memory outcome. Schema v51 and local IPC v53 are unchanged.
- The unresolved completion hold is the in-memory slot plus the existing running or cancelling lease. No outcome payload is stored, and recovery is unchanged. Decision 0098 implements the live slot. S-03B remains open.

## 0.1.0-dev.20261020 (development prerelease, source only)

Source-only checkpoint for responsive metadata reads, random cached exploration, optional globe rotation, MCP evidence schemas and the published rustls advisory lock. Catalog schema remains v51. Local IPC is v53. No binaries or models are distributed, and no roadmap stage exit or supported release is claimed.

- Explorer search, linked context, monitor, finding and reload reads no longer block resize, Back, stop or quit. A late reply keeps the newer view. A favorite change is not cancelled by a newer read.
- `sigy explore` and terminal `e` select one cached station that matches the applied filters and has coordinates. The result reports the eligible count and does not contact a stream. Accepting it stops globe rotation. Local IPC advanced to v53 for that operation.
- Globe `r` toggles optional eastward rotation at six degrees per second. Reduced motion, monochrome, linear, noninteractive and one-frame modes stay still. Native key-to-frame timing remains unmeasured.
- MCP tools keep JSON stdout as structured content when a command exits non-zero, and keep stderr in the text. Six diagnostic and evidence tools now declare output schemas. Failures without JSON stdout use a structured error envelope for those tools. Other tools retain plain-text failures.
- Locked `rustls` 0.23.46, `rustls-webpki` 0.103.16 and `webpki-root-certs` 1.0.10. Those releases clear RUSTSEC-2026-0346, RUSTSEC-2026-0347, RUSTSEC-2026-0348, RUSTSEC-2026-0349, RUSTSEC-2026-0382 and RUSTSEC-2026-0383. Tokio stays 1.53.1.
- Repository instructions require a short evidence-backed note before an issue is finished or closed. `cargo verify` and the verify workflow reject a release tag that has no CHANGELOG heading.
- Hosted verify of `cac8890` passed on Windows, Ubuntu and macOS: https://github.com/blisspixel/sigy/actions/runs/39487469870. Coverage was not repeated after the metadata reads and TLS lock. The last coverage receipt remains the 2026-10-13 MCP schema run. Language, platform and acoustic qualification remain unchanged.

## 0.1.0-dev.20261011.3 (development prerelease, source only)

Source-only checkpoint for atomic recognition and translation responses. Catalog schema and local IPC remain v51 and v52. No binaries or models are distributed, and no roadmap stage exit or supported release is claimed.

- Claimed work and completed jobs are read inside their transaction and returned only after commit succeeds. A failed response read rolls back its claim or publication.
- Successful translation replay checks the original job, artifact, zero-USD metadata and every ordered cue. Different results are refused.
- Eleven fault and replay regressions cover response reads, commit refusal, cancellation, reopen, all 256 cues, Unicode differences and corrupted membership. Linux and macOS CI run them with the Windows gate.
- Automatic actor completion retry, attempt and claim-owner binding, durable exhaustion and finite shutdown handling remain open. Cumulative external spend is not newly incurred by this checkpoint.

## 0.1.0-dev.20261011.2 (development prerelease, source only)

Source-only checkpoint for responsive player observation. Catalog schema and local IPC remain v51 and v52. No binaries or models are distributed.

- Independent bounded status and stop lanes keep local navigation and quit moving while a player reply is delayed. Repeated keys coalesce. Exact listen and generation checks reject stale results.
- Stop closes owned client stdin immediately. Uncertain delivery stays visible. A fresh same-ID running receipt allows another explicit stop, without automatic resends.
- The installer lock owner-death fixture uses an owned Rust child. Session-hint JSON is limited to 4,096 bytes with one-byte lookahead.
- The completion-settlement audit records post-commit read failures and shutdown cases. That repair is not in this checkpoint. The proposed native 100 ms key-to-frame target remains unmeasured.

## 0.1.0-dev.20261011.1 (development prerelease, source only)

Source-only checkpoint for desktop discovery and native listening. Catalog schema and local IPC remain v51 and v52. No binaries or models are distributed. Linux and Omarchy qualification flags remain false.

- Explorer startup filters and `R` then `y` fetch the exact applied directory scope, with bounded asynchronous observation, cache preservation and copyable guidance. Locale suggestions stay optional.
- Windows live playback uses checked PCM and the bounded native output helper. One live CLI jazz listen through headphones was confirmed by the user.
- Linux native commands use experimental delegated systemd user units, with checked CPU, memory and task limits, separate process limits, pre-exec placement and manager-owned cleanup after main-process death. Missing authority refuses before effects.
- Linux decoding checks a known one-second media clock inside its bounded group and refuses incompatible conventions. Null decoding and one Windows acoustic observation do not establish a device or codec matrix.

## 0.1.0-dev.20261011 (development prerelease, source only)

Source-only checkpoint for everyday radio discovery and explicit listening. Catalog schema and local IPC remain v51 and v52. No binaries or models are distributed. Ordinary Linux and macOS native containment remains unavailable.

- Filtered `radio search --fetch`, positional name search, country and tag setup seeds, automatic refresh IDs and bounded `--wait`. Transient directory reads retry inside one admitted request and keep the last cache.
- `sigy play STATION_UUID`, `sigy stop`, and confirmed explorer `P`/`y` playback with `S` stop observe the actual service receipt.
- Panel favorite admission, Unix PID handling, absent-fence cancellation and large CLI future stack frames are repaired.
- Doctor reports native containment failures and refuses new listen or recording attempts before contact or reservation.
- macOS control-socket startup keeps owner-only directory and socket permissions and peer-user checks. Oversized socket paths are refused with shorter-library guidance.

## 0.1.0-dev.20261010.1 (development prerelease, source only)

Source-only checkpoint that restores the Unix installer. Catalog schema and local IPC remain v51 and v52. No binaries or models are distributed.

- `scripts/install.sh` closes its conditional with `fi`, restoring the macOS and Linux install path, including Omarchy and Arch.
- `cargo verify` checks `scripts/install.sh` with `sh -n` when `sh` is present. The verify workflow runs the same syntax check, including Git Bash on Windows.

## 0.1.0-dev.20261010 (development prerelease, source only)

Source-only checkpoint for desktop panel status. Catalog schema and local IPC remain v51 and v52. No binaries or models are distributed. Platform qualification on Arch, Omarchy and Linux audio routing remains open.

- `sigy panel status` reads library and service state, including an offline SQLite read when the service is absent. It does not start a daemon or use the network. It reports service presence, directory freshness, up to four favorites and up to four active captures.
- `sigy panel bar` emits one JSON object with `text`, `class` and `tooltip` for the states `offline`, `failed`, `playing`, `recording`, `stale` and `idle`.
- Panel output states `"qualified_platform": false`, `"qualified_linux": false` and `"qualified_omarchy": false`. Language recognition and acoustic delivery are not qualified by the panel.
- `sigy panel play STATION` admits playback only for a cached favorite with exactly one registered `http_audio` revision and no active prior panel listen. `panel stop` ends that listen. Stdin close and a dead parent process cancel playback.

## 0.1.0-dev.20261004.2 (development prerelease, source only)

Source-only checkpoint for exact cited audio excerpts. Catalog schema is v51 and local IPC is v52. No binaries or models are distributed.

- `listen file --seek-us START --end-us END` plays an explicit retained excerpt. The end is exclusive. `listen finding MONITOR FINDING --request ID` resolves the immutable citation and protects the original file in the same admission.
- New excerpts require exact finite PCM samples and checked completion. Silent playback uses 48 kHz stereo without a device. Decoder, native group, output and original-reader evidence stay distinct.
- Failed original-reader cleanup keeps completed stage observations and the admitted receipt, reports `reader_closure_failure`, and does not claim closure.
- Backup audits reader identity before creating its destination. Restore rejects a malformed range or identity hash even when the outer backup hash agrees.
- The same-rate WAV oracle does not establish a general compressed or resampled profile, acoustic delivery, cold-file latency or sustained capacity.

## 0.1.0-dev.20261004.1 (development prerelease, source only)

Source-only checkpoint for terminal review and Windows source updates. No binaries or models are distributed.

- Windows source updates build an exact prepared commit, stage and hash the executable before publication, fence older helpers against later installations, and expose bounded local status through `sigy update --status`.
- Installer repairs preserve caller paths and failure results. Custom-root instructions name the installed executable.
- The terminal review adds a reproducible 64-frame production-render gallery across seven workspaces, three sizes, dialogs and failure states.
- Scheduling an update is not the same as a successful installation. File replacement does not establish universal rollback or power-loss durability.

## 0.1.0-dev.20261004 (development prerelease, source only)

Source-only checkpoint for protected Windows retained output and linked station context. Catalog schema is v50 and local IPC is v51. No binaries or models are distributed.

- Retained Windows system output uses a bounded decoder and helper group, fixed endpoint selection, strict PCM framing, and separate decoder, presentation, native-closure and original-reader outcomes. Replay starts no new output.
- Explore and Globe keep local Enter inspection and exact Back restoration. `i` and `radio linked UUID` inspect immutable registrations and exact recording metadata, with four-by-four bounds and truthful more or unknown labels.
- Indexed catalog migration keeps historical identities. Plain linked and name-ordered CLI routing and bounded unknown decoder timestamps are repaired.
- Acoustic delivery, sustained capacity and language quality remain unqualified. Exact cited end bounds and interactive DVR controls remain open.

## 0.1.0-dev.20261003.3 (development prerelease, source only)

Source-only checkpoint for service-owned protected retained playback. Catalog schema is v49 and local IPC is v50. No binaries or models are distributed.

- Admission freezes one retained segment and protects its original file until the reader actually closes. Replaying an explicit request starts no new reader or decoder.
- Reader inspection and cancellation are in the CLI. MCP can inspect the receipt and does not start playback.
- Nonzero timeline-origin seeks, single-frame progress, discarded-prefix pacing, and backup of published sealed segments from a still-running recording are fixed.
- Audio-format checks used null output. Audible output on the validation machine remained unqualified because that FFmpeg build had no usable Windows audio output muxer.

## 0.1.0-dev.20261003.2 (development prerelease, source only)

Source-only checkpoint for focused station context. No binaries or models are distributed.

- Enter in Explore or Globe opens full cached station context, with compact inspection and a wide results and detail view. Back restores the same query, draft, page and selection. Inspection starts no network, capture or playback.
- Directory statistics survive ordered reads inside the observed namespace and become unknown after that namespace is replaced.
- Task snapshot fitting counts exact serialized citation bytes once. Existing byte and work limits, truncation and immutable JSON identity stay intact.
- Executable lessons, cryptographic profiles, protected playback and linked recording context remain future work in this checkpoint.

## 0.1.0-dev.20261003.1 (development prerelease, source only)

Source-only checkpoint for ordered station discovery. Catalog schema is v48 and local IPC is v49. No binaries or models are distributed. Browsing starts no station connection.

- Cached station discovery supports revision-bound name ordering in the terminal explorer, `radio search --order name`, and read-only MCP `radio_search`. CLI and MCP defaults keep UUID ordering.
- A changed catalog refuses a stale continuation and keeps the previous page visible. `g` restarts cached page one.
- Stop an older service with its existing binary before replacing it. Task and evidence history, favorites, source provenance and spending authority are preserved.
- City lookup, complete interactive DVR playback, SDR and LoRa adapters remain planned. English remains the implemented translation target.

## 0.1.0-dev.20261003 (development prerelease, source only)

Source-only evidence workstation checkpoint. No binaries or models are distributed. External task spend was USD 0 of the USD 20 ceiling, excluding the development harness.

- Finite tasks can freeze exact collected evidence, publish up to 64 literal findings, and read a briefing bound to that snapshot's coverage. Historical checkpoint execution stays compatible and shares the same lifetime run allowance.
- Explicit processing-interest withdrawal preserves shared jobs. Unresolved native completion after restart holds protected work rather than guessing cleanup.
- The CLI and TUI add an offline country reference with 257 entries, eight display locales, script-preserving aliases, explicit ambiguity and English fallback. Country selection applies only the existing cached query.
- City lookup, station name ordering and complete interactive DVR playback remain planned in this checkpoint. Later Strange Signals Desk proposals are not an implemented SETI detector.

## 0.1.0-dev.20261001.3 (development prerelease, source only)

First-use and documentation checkpoint. Catalog schema and local IPC remain v43. This checkpoint establishes no supported release or roadmap stage exit and distributes no binaries or models. The Windows shutdown failure remains tracked separately.

- Simplify first use with `sigy init`, a default per-user library, optional explicit first-page radio discovery and an offline setup option. Existing budgets, policies and recordings are preserved; MCP still requires an explicit library.
- Lead the README with a current interface render and concise product description. Link detailed capabilities and setup instructions, and provide one-line source installers with failed-download checks.
- Preserve task validation before library access and retain meaningful errors for independent backup operations. Add focused setup, retry, ownership and recovery fixtures. One real directory refresh and exact setup replay complete without recording or paid inference. Cumulative external spend remains USD 0.

## 0.1.0-dev.20261001.2 (development prerelease, source only)

Documentation and diagnostic checkpoint over the verified task-collection implementation. Catalog schema and local IPC remain v43. This checkpoint establishes no supported release or roadmap stage exit and distributes no binaries or models.

- Preserve and analyze two existing Windows client heap-corruption dumps, including an earlier ordinary build. Correct the previous statement that no actionable dump was available.
- Record 37 bounded debugger fixture passes, one deliberate storage-guard interruption, symbol and debugger limitations, and the distinction between a detection stack and a proven cause.
- Prioritize the unresolved shutdown defect for the next session. Set aside the unverified transport draft; publish no speculative repair or acceptance waiver. Cumulative external spend remains USD 0.

## 0.1.0-dev.20261001.1 (development prerelease, source only)

Catalog schema and local IPC advance to v43. Stop an older service before replacing its binary. This increment does not establish a roadmap stage exit or supported release.

- Add explicit task-owned collection: one lifetime zero-USD grant creates up to two new finite UTC once schedules, bound to the task's inspected monitor version, complete action prefix and exact source revisions.
- Commit the grant and all new rules together. Fence actual capture admission inside the shared DVR reservation transaction, preserving full planned UTC-day and lifetime seconds and worst-case bytes without refunds or refill.
- Add separate collection inspection and cancellation. Status follows only owned occurrence and recording identities; cancellation stops future admissions and preserves admitted captures and independent work. Ordinary schedule inspection identifies task ownership and refuses rule revision.
- Harden replay, clock regression, cancellation ordering, corrupted scope and orphan ownership. Reopen and verified restore preserve the original grant, recording provenance and conservative reservations.
- Add parser, status, transaction, migration, recovery and native loopback checks. Task-owned processing interests, collection-to-evidence reconciliation, general planning and language/platform qualification remain open. Cumulative external spend remains USD 0 of the authorized USD 20.

## 0.1.0-dev.20261001 (development prerelease, source only)

Catalog schema and local IPC remain v42. This checkpoint has no supported release profile or roadmap stage exit. No prebuilt application binary or model is distributed. Verification evidence and remaining gates are recorded in [active work](docs/development/progress.md).

- Commit canonical recognition or translation admission with its immutable monitor receipt and exact usage charge in one transaction, before worker scheduling. Reuse the existing enqueue validators and durable job pool.
- Recheck current policy, complete action count, pause, followed sources, profile identity and stored audio duration at fresh admission. Temporary clock, policy and capacity holds create no permanent skipped outcome. Translation requires the monitor's exact recognition result.
- Preserve exact historical replay and shared jobs, with separate caps for each monitor. Replaying a request starts no worker and never charges a new UTC day. A failed second-monitor attachment leaves the existing job and first monitor's charge intact.
- Exercise job, step and deferred commit failures, reopen, daily and lifetime bounds, queue pressure and post-commit scheduling. Metadata-only analysis pins remain separate and may persist after processing refusal.
- Describe processing status as admitted audio and historical admission counts. Completed, failed and cancelled work remains charged; the counters are not live queue depth or successful processing.
- Update the roadmap and research-linked admission contract. Task-owned collection, general local planning, language qualification and platform containment remain open. Cumulative external spend remains USD 0 of the authorized USD 20.

## 0.1.0-dev.20260930.1 (development prerelease, source only)

Catalog schema and local IPC are v42. This checkpoint has no supported release profile or roadmap stage exit. No prebuilt application binary or model is distributed. Verification evidence and remaining platform, language, capacity and containment gates are recorded in [active work](docs/development/progress.md).

- Added immutable finite task scope, frozen coverage and exact citation checkpoints, with original-script goals, policy drift detection, exact replay, bounded listing, restart and verified backup/restore fixtures.
- Added explicit finite execution delegation from one selected checkpoint. The existing service tick publishes bounded task-owned literal findings and an exact-membership briefing, with atomic effect receipts, generation-fenced cancellation, policy revocation and truthful partial outcomes. This is a fixed evidence workflow; general planning, task-owned collection and model transports remain unimplemented.
- Preserved historical findings after whole-recording deletion and segment release. New citations use current media availability; old citations remain immutable. Task briefings exclude unrelated monitor findings and retain the selected checkpoint's frozen coverage.
- Added opt-in monitor-owned capture schedules through the existing civil scheduler, with separate full-window UTC-day and lifetime reservations that never refill. Processing pause leaves independent capture authority intact.
- Added terminal station clusters, monitor inspection, recording metadata timelines and named finding navigation. Hostile terminal text is sanitized while original stored evidence remains unchanged.
- Strengthened native task preflight before staging, including resource and parameter bounds; recognition decoding requests an operating-system CPU cap. Native network isolation, Linux delegated containment and the Windows suspended-spawn assignment window remain open.
- Added per-crate exact-integer 80% coverage enforcement without source exclusions. Offline language scoring reproduces frozen calibration references; a 126-control local judge screen failed all seven tested language groups and establishes no language qualification.
- Updated the roadmap, privacy and recovery requirements, interoperability research and qualification matrix. Paid processing remains unavailable, and cumulative external spend remains USD 0 of the authorized USD 20.

## 0.1.0-dev.20260930 (development prerelease, source only)

Not a supported release and no roadmap stage exit. Catalog schema and local IPC are v39. Windows x86_64 has the test evidence; Linux passes the suite in a container but cannot yet run local models; macOS is untested.

- `monitor briefing` freezes coverage with the generation. A later generation keeps the earlier one. A correction marks a cited finding stale and writes no ledger event. `monitor briefing export` prints a redacted snapshot that is not the catalog and writes nothing. `monitor coverage` stays a live read. The same window changes nothing. Classification stays off. `sigy mcp` has no tool that publishes or exports a briefing. Wording remains uncertain. Catalog schema and local IPC are v39.
- `monitor briefing` stores one generation over a monitor's findings. Coverage is stated first and read again for the stored window. A repeated original script counts once. A different script stays an unresolved conflict. Classification stays off. The same window changes nothing. A different window conflicts and writes nothing. Transcript text creates no job, and `sigy mcp` has no tool that publishes a briefing. Wording remains uncertain. Catalog schema and local IPC are v38.
- `monitor finding` stores one citation of a translation revision, an original transcript revision, and a retained cue interval, or an explicit statement that the original is expired or missing. A missing range is rejected and writes nothing. The same citation changes nothing. A different citation for that finding id conflicts. A newer transcript or translation is reported stale and the finding is not rewritten. Transcript text creates no job, and `sigy mcp` has no tool that publishes a finding. Wording remains uncertain. Catalog schema and local IPC are v37.
- `analysis correct` appends one transcript revision that replaces one cue's original script and copies the other cues and their times. The expected revision must be the one last read. A second edit of that revision conflicts and writes nothing. The previous revision stays readable. Wording remains uncertain. The command does not start recognition, translation, or a paid request, and it does not restore a recording that is no longer retained. A translation or bound language-evidence row from an older revision is reported stale and is not rewritten. Monitor matches follow the corrected text. Catalog schema and local IPC are v36.
- `sigy doctor` compares recognition arrivals on this library with the busy time of completed recognition. The newest 256 local recognition jobs supply the admission span and their retained audio. Completed work is the same sample as the pace report, before per-profile rounding. The sentence says whether admissions brought more, the same, or less audio per wall millisecond, or that one side is unmeasured. It is not a clock time for an empty queue and it is not a host budget. The queue sentence and the worker-cost sentence stay. Catalog schema and local IPC stayed v35. The check does not admit work.
- Catalog schema and local IPC advanced to v35. When a recognition group is proven empty, that snapshot is stored. On a Windows job object it keeps peak committed memory and total CPU time. An empty cgroup stores no numbers and stays out of the figure. `sigy doctor` reports the highest peak and the CPU sum. The figure is not a host budget. No paid request is created.
- Chunked recognition advanced catalog schema and local IPC to v34. One recognition job hears windows of at most 30 seconds, up to 1,024 windows, and publishes one original-script transcript. A phrase that reaches a window edge while audio remains waits, and the next window starts at that phrase. A phrase with no earlier break is stored through the window end, so a word there can still be cut. Published coverages abut, stay within 30 seconds, and tile each segment. The 64 MiB byte cap, the cue cap and the text cap are unchanged. A queued job from an older manifest fails as no longer current. No language is qualified.
- Queued jobs rotate across sources. Every fourth claim takes the oldest waiting batch job, so one busy source cannot hold the single slot. Jobs on one source still run oldest first. Host budgets remain open.
- A monitored recognition job is live while the current time is still within the recording's last seal plus its retained audio at that profile's measured pace. An unmeasured profile, a paused monitor, verification, and translation stay in arrival order. The deadline is not stored. Schema and local IPC stayed at v34.
- `sigy doctor` reports the pace of completed recognition on this library, in milliseconds of wall time per second of retained audio, or that the pace is unmeasured. The check stays ok. That pace is the delay used for the live window above. It also reports the wall time of queued recognition audio at that pace: the processing time of audio already waiting. A running or stopping job is a count. Audio with no measured pace stays out of the wall-time total. The figure is not a clock time for an empty queue. A separate sentence compares arrivals with the busy time of completed recognition. A new recording still publishes while recognition is queued and after its monitor is paused. That recording stays out of the sentence until a recognition job exists, and a capture gap is not audio. Schema and local IPC stayed at v34. The check does not admit work.

## 0.1.0-dev.20260925 (development prerelease, source only)

Not a supported release and no roadmap stage exit. Catalog schema and local IPC are v33. Windows x86_64 has the test evidence; Linux passes the suite in a container but cannot yet run local models; macOS is untested.

- Ran local speech recognition in the service: a hash-pinned whisper.cpp profile in a contained process group with process, memory, CPU and deadline limits, a required speech-activity model, and original-script cues on the media clock. The recognizer's language label is stored as unevaluated evidence.
- Translated recognized transcripts into English locally with a hash-pinned llama.cpp profile, one contained process per cue, keeping untranslated cues with reasons. CPU profiles now pin CPU execution even when a GPU backend is present.
- Recorded calibrations: 32 FLEURS clips for recognition and translation with a sacrebleu-matched scorer, eight live public stations, and a Vulkan measurement on the Radeon 780M. None qualifies a language.
- Recorded live HLS stations and listed master playlist variants for explicit acceptance; tolerated slow stream edges, repeated `CODECS` claims and the `audio/x-aac` alias.
- Added exact provider pricing, provider routes that store secret names only, and an offline billing-fault dispatcher proving that a lifetime paid allowance never refills. No paid request can be sent.
- Queued recognition, translation and verification in a durable job pool with leases, attempts and content-addressed task specs, and removed the lifetime job-history cap.
- Added topic monitors: user-authored versions, an append-only action log where rules and models can only propose, coverage counted stage by stage, literal term matches with exact citations, and automatic recognition and translation of new recordings within daily and total audio caps, shared across monitors. Analysis pins are no longer capped over the library's lifetime.
- Drew a terminal globe and flat day/night map with offline coastlines, and added offline library backup and verified restore.
- Let MCP agents transcribe, translate, read results, read monitors and record monitor proposals; no tool creates a monitor, adds a profile or enables paid processing.
- Fixed Unix issues found by a Linux container run: `library init` now makes the library directory private, and the control socket naming passes Clippy.
- Designed the scaling architecture: a stage graph of level-triggered reconcilers, warm workers, measured capacity, and a `sigy worker` for a spare GPU machine.

## Through 0.1.0-dev.20260923

- Transcribed one published recording locally. The revision is original script. No measured recognizer is selected, so each cue script is empty and the wording label is uncertain. A gap has no cue. Replay and a rollback before commit do not publish a second transcript. The decision is 0 USD. The global limit stays unchanged, and no paid request is reserved. Catalog schema is v23. Local IPC is v24.
- Bound analysis to one published recording. The pin stores the retained checksum and the media clock, including gaps for the uncovered plan. A cleanup receipt does not admit analysis. An unpublished or mismatched recording is refused. A replaced worker cannot publish over the newer revision. The pin has no source URL. Catalog schema is v22. Local IPC is v23.
- Saved a directory refresh policy. One rule stores one bounded page and an interval of 1 to 168 hours. The service admits the current due slot. A missed slot is not backfilled. Opening a client does not fetch, probe a station stream, or send a click. A failed or interrupted slot leaves the last cache and is not repeated. Favorites stay. Catalog schema is v21. Local IPC is v22.
- Scheduled one source by civil time. A rule is once, daily, or weekly in an IANA zone. Only the next occurrence is stored. Replay and restart do not admit a second job. A missed window stays missed. A spring-forward civil time is missed. A fall-back hour uses the earlier offset once. A late start records a prefix gap. Changing a rule does not rewrite an admitted plan. No analysis profile can be attached. Civil time uses the IANA data bundled with jiff 0.2.37. Catalog schema is v20. Local IPC is v21.
- Held a saved timeline range and pruned aged temporary segments. The hold protects every published segment it intersects and records a gap inside that range. Keep and Archive stay exempt and still count. The open tail is not pruned. A processing receipt does not protect the file. Released bytes are charged once. Catalog schema is v19. Local IPC is v20.
- Stated the full product in the README opening. Sigy is a local-first signals workshop for discovering, inspecting, capturing, decoding, translating, monitoring, and explaining signals. Internet radio and podcasts lead. Music identification, wider text feeds, receive-only hardware, packets, Morse, historical ciphers, and supplied-key cryptography follow on the same evidence model. The first complete release is the listening desk.
- Colored the list explorer and human command output with the terminal palette. Cyan, green, yellow, and red reinforce connection, health, favorites, and failures. The words stay. `NO_COLOR`, `--monochrome`, `--linear`, `TERM=dumb`, a pipe, and `--json` stay plain. `NO_COLOR` wins over `FORCE_COLOR`.
- Added a GitHub CLI one-command install and `sigy update`. The installer builds the latest `main` commit. A logged-in GitHub CLI supplies Git credentials for that private repository. `sigy update --check` reports the recorded commit and exits with an error when the install is missing or older. This is not a release binary and not `cargo verify`.
- Played one sealed segment at a time from independent in-memory playheads. Playback pause does not stop the capture. Seek stays inside a published segment and fails in a gap or the open tail. Return to live parks at the newest published end. Leaving `listen play` drops that playhead and leaves the capture running. Catalog schema stays v18. Local IPC is v19.
- Journaled capture gaps for disconnect, recovery, codec change, refused renewal, capture pause, and a backward clock. A gap has no audio file. Seeking inside it fails. Catalog schema is v18 and local IPC is v18.
- Sealed segments on one running radio capture. The open ceiling is 32 MiB, or 5000 ms of receive time, whichever comes first. That 5000 ms bound is the candidate uncommitted window, not a measured durability result. The whole byte budget stays one escrow. A stale token cannot seal, and renewal stays on the same socket. Catalog schema is v17 and local IPC is v17.
- Added `sigy doctor`, a read-only preflight for catalog integrity, the decoder, quota, directory cache age, and podcast snapshot age. It does not refresh or use the network. Stations older than 24 hours stay cached, including a row with no observation time. `radio status` shows how many cached stations are stale. Recording refuses to start when the configured decoder is not a file.

- Rewrote the README as a product introduction. It shows `sigy --help` and an empty list-explorer frame from this Windows host. Command detail lives in the usage guide.
- Added `scripts/install.sh` and `scripts/install.ps1`. They install Rust 1.98.1 for the current user when `cargo` is missing, then install the `sigy` binary from this checkout. They do not install an operating-system service or download FFmpeg, and they are not the verification entry point.

- Added a Rust workspace with a dependency-free domain core, transactional service storage, and an initial maintenance CLI.
- Added exact USD accounting, global and named lifetime budgets, atomic reservations, submission uncertainty, idempotent reconciliation, and billing-breach freezes. Provider dispatch remains unavailable.
- Added exclusive library ownership, schema identity checks, SQLite migrations, and append-only ledger events.
- Added a bounded local controller with foreground/detached operation, client reconnect, explicit shutdown, protected local IPC, and restart reconciliation. Native Windows behavior is tested; OS startup installation remains pending.
- Added capture intent, atomic admission, revision/generation checks, append-only job history, interrupted-attempt recovery, integrity audits and bounded reads.
- Added schema v3 immutable HTTP source revisions, exact replay/conflict handling, explicit network grants and paginated CLI registration/list/inspection through the shared controller.
- Added finite HTTP acquisition with checked destinations, disabled implicit proxies/retries, explicit bounded redirect policy, strict response handling, byte/time limits and shared admission. Successful recordings retain origin/peer/status provenance in metadata envelope v2.
- Added service-owned finite recordings with decoder validation, original-byte hashes, 14-day/50 GB rolling retention, Keep/Archive protection, processing acknowledgments and staged deletion.
- Added bounded Radio Browser refresh, cached multilingual search and immutable source registration with directory provenance. Refresh does not contact station streams.
- Added persistent radio favorites and filtered offline search through shared service/maintenance operations. Catalog and IPC are v7; saved choices survive refresh and restart.
- Added truncated-catalog rejection, conservative build/test concurrency, bounded CLI test processes, and regression coverage for detached output-handle inheritance.
- Added local tests for accounting, concurrency, restart recovery, injected storage failures, library locking, and real CLI operations.
- Pinned Rust 1.98.1 and verified SQLite 3.53.4 source through a documented temporary native dependency override.
- Extended the roadmap with optional user-controlled proxy routing and external VPN compatibility for personal machines and servers. Routing profiles, remote access, model processing, and monitoring remain unimplemented.
- Recorded the build order from retained-file playback through the first complete release. That order is the implementation sequence.
- Added foreground playback of one retained recording through the configured FFmpeg decoder. The client owns the playhead. Playback does not contact the source, reserve quota, or stop capture. Station listening remains unimplemented.
- Added bounded playlist resolution on the shared acquirer. One request reads a playlist document, rejects HLS, and does not open entry URLs. Accepting an index registers a new audio revision without another fetch. Catalog and IPC are v8.
- Verification is `cargo verify` and `cargo verify-media`, implemented by the Rust `sigy-xtask` package. PowerShell is no longer required.
- Added an explicit Radio Browser click. One command sends `GET /json/url/{stationuuid}` and discards the returned stream URL. Search, refresh, and favorites do not send it. Replay does not send it again. Catalog and IPC are v9.
- Added a finite listen of one direct audio revision. The service fetches through the shared acquirer and writes a private local pipe. The client decodes that pipe and does not receive a source URL. The listen is not a recording, does not reserve quota, and is not resumed after restart. Replay does not reconnect. A playlist or ICY response fails before decode. Catalog and IPC are v10.
- Corrected the development instructions so they match retained-file playback, the roadmap build order, and the current verification commands.
- Pushing `main` runs `cargo verify` on one standard GitHub-hosted Windows runner. `cargo verify-media` stays local. There is no product release.
- Added finite HLS media-playlist recording. `record hls` fetches one ended media playlist on the shared acquirer, at most 32 segments, and publishes the concatenated audio through the existing recording path. A master playlist fails before a variant fetch. Live playlists, encryption, maps, byte ranges, and discontinuity are rejected. Segments do not become source revisions, and the decoder does not receive an `.m3u8` address. Catalog schema remains v10. Local IPC is v11.
- Added explicit ICY observations on `record start --icy`. Metadata blocks are removed before the audio file is hashed. The published SHA-256 matches the audio bytes. Titles do not rename a source or grant a URL. The default, every listen, and HLS recording still reject interleaved metadata. Export schema is version 3 only when observations exist. Catalog schema is v11 and local IPC is v12.
- Recorded one local Windows format ladder with FFmpeg 9.0.1. The session decoded direct WAV, MP3, AAC (ADTS served as audio/aac), FLAC, and Ogg Vorbis. It also decoded WAV from one accepted playlist entry behind one same-origin redirect and played that revision to `--destination null`. The fixture contacts only 127.0.0.1. One smoke session is not a support matrix. Catalog schema stays v11 and local IPC stays v12.
- Selected Ratatui 0.30.2 with the Termina 0.3.3 backend for the later list explorer. Crossterm 0.29.0 was rejected because that bracketed paste produced no paste event. On ConPTY the wait timed out. Inside Windows Terminal the query became `navajo[20~`. The measurement ran on ConPTY and in Windows Terminal 1.24.11911.0. No terminal crate was added, and the client still opens no second catalog.
- Added `sigy tui`, the list explorer, on Ratatui 0.30.2 with the Termina 0.3.3 backend. It shows the service connection, partial cache, observation age, refresh outcome, search, favorites, source detail, recordings, quota, and playback. Directory health stays labeled as directory health. Selection does not start audio, capture, refresh, or a click. Quit does not stop the service or a recording. Reduced motion, linear order, and monochrome are available, and this client draws no animation. Monitors, Findings, the globe, and the map stay unavailable. The Windows inspection was Windows Terminal 1.24.11911.0 at 80x24 with the Consolas font. The client still opens no second catalog. Catalog schema stays v11 and local IPC stays v12.
- Added local podcast subscribe and unsubscribe. The feed URL, network scope, address pin, and redirect policy are immutable. Unsubscribe stops future polls and deletes nothing. Subscribe does not resolve DNS, download the feed, register an audio source, or start a capture. Catalog schema is v12 and local IPC is v13.
- Added one RSS 2.0 feed refresh on the shared acquirer. A refresh lists episodes from the last good snapshot and does not download enclosures, transcripts, or chapters. A failed document leaves that snapshot in place. Omission is not deletion. Catalog schema is v13 and local IPC is v14.
- Added one explicit enclosure download through the existing recording path. The request registers an immutable audio revision, reserves 512 MiB and 30 minutes before connect, and publishes only a clean ended, decoded, hashed file. Radio attempts stay 15 minutes and 256 MiB. A declared length above 512 MiB is rejected before connect. Replay does not download twice. Catalog schema is v14 and local IPC is v15.
- Added `podcast text` for one publisher transcript or chapter document. Refresh still does not fetch those URLs. The text is stored as unverified publisher cues, outside the recording quota. Restart marks a running fetch interrupted and keeps the previous snapshot. Catalog schema is v15 and local IPC is v16.
- Projected each published recording onto one measured interval. The planned window and an unpublished part are not that interval. Catalog schema is v16 and local IPC stays v16.
- Documented that the service sweep deletes a temporary recording after `record processed`, by age, or under quota pressure. `record keep` and `record archive` stay.
- Added `sigy mcp` for MCP 2026-07-28 and an Agent Plugins 1.0.0 package. Tools run the existing commands against the configured library. They cannot select another directory, change a budget, or pass a shell command.
- Added playback of one retained episode through `listen file`. The episode has no live edge, so `listen source` on that enclosure revision fails before a request. Subscribe and refresh still do not download or play. The tested file stays temporary under the default 14-day and 50 GB policy. Playback does not change quota. Catalog schema stays v14 and local IPC stays v15.
