# Implementation progress

Updated: 2026-09-21. Status: active evaluation and implementation.

## Current objective

Build the complete Rust CLI/TUI product in verified increments. Radio exploration, recordings/DVR, live multilingual translation and bounded topic monitoring remain the first complete release contract. Internet radio and podcasts precede hardware. The user chose to keep Sigy on 2026-09-21; naming exploration is deferred. An internal milestone is not a release.

## Work sequence

| Work | Current state | Evidence and remaining scope |
| --- | --- | --- |
| Foundation | Rust 1.98.1 and SQLite 3.53.4 selected | Initial Rust/Go probes and dependency review; native platform matrix incomplete |
| Catalog and financial ledger | Implemented with local tests | Exact amounts, atomic reservations, replay protection, uncertain liabilities, rollback and exclusive ownership; paid dispatch remains unavailable |
| Persistent controller and CLI | Implemented on Windows | Independent clients, detached/foreground operation, explicit stop, peer checks and bounded IPC; Unix and OS startup qualification pending |
| Capture journal and source authority | Implemented | Immutable source revisions, opt-in bounded redirects, exact worker generations, interrupted recovery, checked destinations at every hop and shared HTTP limits |
| Finite recording and retention | Implemented and locally tested | Decoder-backed publication, original-byte hashes, versioned metadata export, full byte reservations, 14-day/50 GB defaults, Keep/Archive, processing acknowledgment and staged deletion |
| Radio directory discovery | Implemented and locally tested | Bounded Radio Browser mirror refresh, offline multilingual search, visible observation age, immutable source provenance, interrupted recovery and one live metadata smoke check |
| Podcast discovery and listening | Next integration | Direct RSS subscriptions, episode metadata, bounded playlists and integrated playback |
| Continuous DVR and terminal experience | Planned | Segment timelines, pause/seek, saved intervals, schedules, CLI/TUI parity, multilingual rendering and geography |
| Local analysis and monitoring | Planned | Per-span language shifts, candidate ads, ASR/translation, publisher-text reuse, fair queues, processing receipts, grounded findings and bounded autonomy; music boundaries/identity follow later |
| Hardware and signal workbench | Planned after internet sources | Receive-only CB/AM/FM/shortwave and spectrum modes, typed IQ/packet observations, LoRa/Meshtastic and later decoders; actual device qualification required |

## Spending ledger

Cumulative authorized external-spend ceiling: **USD 10**, excluding coding-session costs. Settled spend: **USD 0**. Outstanding commitments: **USD 0**. Available: **USD 10**.

No paid provider, hosted CI or recurring service is enabled. Use local builds, synthetic fixtures, public documentation and free package retrieval. Paid experiments require a mechanically bounded allocation and recorded reservation before dispatch. The application's provider budget remains separately disabled by default.

## Verified evidence

The preceding directory checkpoint is commit `bced2d3`. The current workspace passes **89 ordinary tests plus 4 explicit native-media tests** on Windows x86_64. `./scripts/verify.ps1` passed native source hashes, formatting, workspace tests, warnings-denied Clippy, build and dependency auditing. The advisory check reviewed 239 dependency entries against 1,258 loaded advisories without findings. Redirect integration adds no packages or lockfile changes.

`./scripts/verify-media.ps1` passed using the installed FFmpeg 9.0.1 executable. Real CLI processes record a bounded local WAV fixture, preserve its exact bytes, validate positive decoded duration, export metadata, reconcile replayed requests, protect kept media, acknowledge processing and reclaim temporary media. Other fixtures reject non-audio bytes with an audio MIME label and kill the service after partial reception. Restart retains partial bytes and their reservation, marks the attempt interrupted, and does not issue another source request on replay.

The new media fixture follows an explicitly permitted same-origin redirect and publishes exact audio bytes with envelope v2 route observations. Transport tests cover all supported redirect statuses, no cookie/auth/referer forwarding, malformed/duplicate Location fields, denied secondary connections, loops, hop and byte caps, shared deadlines and stop during headers. Policy cases cover public/private destinations and downgrade. Migration and storage tests preserve v5 permissions, reject conflicting policy replay, roll back failed migration and reject invalid or rewritten publication provenance.

HTTP tests distinguish intentional duration limits and user stops from failed transport and empty recordings. A regression test caught and fixed the interaction between request read timeouts and planned recording completion. Migration tests cover v3-to-v4 success, conflict rollback and preservation of existing financial limits. Retention tests cover age, quota pressure, processing acknowledgments, active/protected media, competing reservations, stale publication and deletion accounting.

Deleting interrupted media closes its pending intent while retaining history and replay protection. The CLI exposes `record`, `dvr` and `radio`. Directory fixtures exercise real client/service requests, Unicode filters, exact replay, immutable source registration without stream contact, oversized replies and process-kill recovery. Storage tests cover atomic page rollback, invalid rows, duplicate identities and v4-to-v5 migration preserving financial and DVR policy. Documentation checks cover local links/anchors, C-01 through C-40, R-01 through R-57, writing rules and diff whitespace.

A live directory smoke check on September 21 exercised automatic SRV mirror discovery, bounded HTTPS metadata acquisition, durable publication and cached CLI search. The corrected `--name france --limit 5` request accepted five station records with no rejected rows. An earlier request exposed the effect of empty API filters; unused parameters are now omitted and a fixture checks the request shape. This validates one directory interaction, not complete-world coverage or audio compatibility.

A prior public-radio audio smoke attempt was blocked before execution by automatic approval review; no public audio recording was obtained. Local media fixtures do not establish broad codec qualification. Publisher-feed research fetched only bounded RSS metadata, without episode media or secondary assets. No inference, physical radio or paid service was exercised.

The available host is Windows x86_64, AMD Ryzen 7 7840U, approximately 64 GiB RAM. A reported host crash has no established cause. Builds remain limited to two jobs, ordinary tests to two threads per binary and native-media tests to one. Test subprocesses and fixture servers have finite deadlines. Other operating systems, small-host capacity and language quality remain unqualified.

During redirect verification, an existing directory crash fixture once timed out waiting five seconds for its local request. The original cause was not captured. Failure diagnostics now include the durable refresh status, with the deadline unchanged. The targeted test, three complete service-suite repeats and the final full verification passed. Investigate with those diagnostics if it recurs; this is not evidence of qualified long-running reliability.

## Current contracts and limits

Catalog schema is v6 and IPC is v6. Stop older controllers before replacing binaries. Existing schema versions migrate transactionally; automatic backups and power-loss recovery still require qualification. The single library owner retains the lock through workers and catalog operations. Read [controller](../decisions/0002-local-controller.md), [journal](../decisions/0003-capture-journal.md), [source authority](../decisions/0004-source-authority-and-http.md), [recording/retention](../decisions/0005-recording-and-retention.md), [directory refresh](../decisions/0006-radio-discovery.md) and [authorized redirects](../decisions/0007-authorized-redirects.md) decisions.

Directory refresh merges one requested page, at most 500 rows and 2 MiB, into a cache of at most 10,000 stations. It does not delete unseen stations or claim complete coverage. Refresh requires the service; search/show/registration can use the local cache offline. One active refresh, finite history, deadlines and shared network slots bound work. No periodic refresh or second provider exists yet. Directory metadata, health and languages are not detected content or verified stream compatibility.

The current recording profile accepts HTTP audio with an explicit deny/same-origin/public redirect policy and a three-hop ceiling. Existing sources migrate with redirects denied. Checks, concurrency and total deadlines apply across the route; pins cannot grant cross-origin access and HTTPS cannot downgrade. Playlists/HLS, interleaved ICY metadata, automatic retries and proxies remain unsupported. Two active attempts, 15 minutes and 256 MiB per attempt are conservative bounds, not measured capacity claims. Hickory provides asynchronous DNS with checked destinations; TLS retains offline verification and explicit roots. The external decoder receives local bytes only and is not bundled or covered by the Rust advisory audit.

Envelope v2 exports redirect policy and optional origin/peer/status observations published with successful media. Paths and queries are excluded, legacy/unavailable evidence is null, and failed-attempt routes are not retained yet. These observations are provenance, not new permissions.

Media quota includes retained bytes and full outstanding reservations. Kept/archived objects remain protected and count toward quota. Failed/interrupted bytes remain unverified and charged until deletion. Archive is not backup. Explicit processing acknowledgment only records a receipt; it does not run analysis. Metadata exports are snapshots and do not confer authority. Original checksums are recorded at publication; external file-change reconciliation remains open.

The free-space floor cannot reserve space against other programs. Catalog/WAL, models, exports and future derivative caches need separate storage policies. Native aggregate memory/CPU sandboxing, decoder fault corpora, Unix parent-death behavior, filesystem ACL qualification, physical power loss, migration backups and source-permission revocation remain release gates. No OS startup service, release, platform certification, TUI or provider dispatch exists.

## Next bounded work

Connect the implemented catalog and recordings to integrated playback and bounded playlist handling through the existing checked redirect path. Resolve click-reporting requirements separately from metadata reads and qualify public stream compatibility. Add favorites and the first rendered terminal explorer through existing operations; keep partial-cache coverage and stale observations visible.

Follow with bounded direct RSS subscriptions, episode identity/revisions, publisher transcript/chapters and enclosure acquisition. Use shared capture/storage controls and explicit backfill limits. Continuous segments and analysis workers must record actual timing, gaps, processing dependencies and cleanup receipts. The [broadcast analysis contract](../design/broadcast-analysis.md) now covers language shifts, candidate ads and independent song boundaries; no detector is implemented. Do not create a second scheduler, HTTP policy, catalog or accounting path.

Keep canonical decisions and evidence here and in the linked documents. Disposable diagnostics belong in ignored `.agents/`. Verify Git and remote state before claiming a clean or synchronized checkpoint. No release or deployment is implied by this record.
