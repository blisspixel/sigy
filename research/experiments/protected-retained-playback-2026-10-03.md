# Protected retained playback validation

Reviewed: 2026-10-03. Scope: the first DV-01A protected-reader increment on the current Windows workstation. [Decision0083](../../docs/decisions/0083-protected-retained-readers.md) defines the selected contract; [active work](../../docs/development/progress.md) records integrated verification. No stage exit, supported device/platform profile or language qualification is established.

## Selected path and evidence boundaries

The library-owning service admits one frozen recording interval and streams its encoded bytes over the existing private, peer-checked pipe. The client runs the configured decoder and owns audio output. Admission and original-file protection commit before dispatch. Only the actual joined file reader constructs the private completion proof used to end protection. Exact request replay starts no reader or decoder.

This keeps cooperative file reading separate from a new service-side native decoding purpose. Creation-time decoder identity, native containment, abrupt-parent-death behavior and aggregate resource admission remain open. An original file can be proved closed while buffered client audio or a native descendant still exists. Null-output observations establish decoder progress, without proving sound heard at a device.

Catalog schema advances from v48 to v49; local IPC advances from v49 to v50. No dependency, model, provider, receiver or translation target changes. English remains the implemented translation target and original scripts remain stored. External spend is USD 0 under the existing cumulative USD 20 ceiling.

## Focused checks and repairs

The initial worker filter passed ten cases and the broader processing regression filter passed 89. Actual files, bounded channels and joined readers check byte identity, first/second-pass checksum refusal, nonregular inputs, cancellation, failed delivery, deadlines and ownership lifetime. A cooperative deadline cannot interrupt a blocked filesystem call. Protection and the ownership handle remain held until the blocking closure actually returns.

Four actor cases passed actual byte transfer, exact replay without pipe reattachment, stale generations, cancellation-transaction failure followed by successful reading, wrong private completion identity and completion-transaction failure. Failed completion persistence produces a visible recovery hold and removes the already-finished in-memory worker, allowing the actor to report idle without releasing catalog protection.

The storage/history filter passed twelve cases before the additional refusal cases: nine storage cases and three integrated backup cases. Independent hash witnesses, actual UTF-16LE/BE populated migration/reopen, immutable identity, replay, capacity and query cleanup are covered. The backup cases exercise real backup and restore with protected sealed intervals of reserved recordings; a failed operation retains the original state. Open tails remain outside the canonical media projection.

Decoder tests initially passed 13 cases, and the later one-frame progress repair passed 14. Concurrent pumping and progress draining prevent one pipe from starving the other. One absolute deadline includes pumping, progress EOF and actual child exit, with a separately bounded stop/reap grace. A real waiting child demonstrates that closed progress output cannot bypass the execution deadline. Hostile progress checks cover conflicting/regressing times, oversized lines/frames, aggregate output and frame counts. The lifetime output budget is explicitly 8 MiB; the frame budget derives from the wall-work horizon at the declared cadence, separately from media-duration bounds.

The CLI listening filter passed ten cases after dispatcher extraction. Replay inspection precedes decoder resolution, enabling historical replay with a stopped service and removed decoder configuration. Fresh admission advances only from the canonical typed missing-receipt result. Malformed responses are cleaned up against the expected request/generation; uncertain admission errors name the inspection command. Waits compare the entire frozen specification. Decoder and reader outcomes remain separate.

Cross-boundary review repaired several integration defects before the native gate: retained MPEG-TS remains available without expanding direct-live formats; retained input no longer runs at real time while discarding a long prefix; gap/open-tail/outside-audio refusal messages retain their existing meaning; cancellation/completion floor a regressed clock without new authority; and backup uses the shared published-segment projection. The combined workspace, all-target Clippy pass denies warnings, with no lint allowance.

## Full-history work profile

A bounded layout fixture contains 4,096 independently hashed frozen reader identities over a legitimate published interval. Its terminal rows are synthetic layout data; separate joined-worker tests establish physical close proof.

Using the interactive 100 ms guard for full-history integrity inspection initially interrupted at 102.846 ms with repeated lineage queries and at 100.3206 ms with one indexed stream. Borrowed validation and fixed-stack canonical hashing reduced measured samples to 91.1215 and 92.1414 ms, but a reopen still interrupted. This was insufficient margin for a full-history audit.

Before coverage qualification, only `audit_retained_readers`, including owned library reopen, received a purpose-specific 1,000 ms wall / 4,000,000 VM-operation / 10 ms lock-wait profile. Every bounded row, digest, lineage and unresolved-capacity constraint remains checked. Show, list, admission, cancellation, completion and holds retain their interactive 100 ms profile. Query interruption refuses rather than returning incomplete history.

With the selected profile, the 4,096-row audit measured 112.2234 ms and a sixteen-row list measured 2.1535 ms in one debug sample. The independent fixed-stack digest witness is `7e4233e45cdf0b23e24ee88dd96238f7c66937bf9729c2edf96422193497e1f2`. These observations qualify neither portable latency nor a physical-reader capacity.

## Integrated gates

The first native run passed 15 of 18 fixtures in 108.00 seconds. Both new offset/replay/restart and long-prefix-seek cases passed. Three existing playback assertions found successful short WAV output reported as no advancement when only one final progress frame exists. A validated positive observation now counts as advancement from zero, without requiring a previous frame. Independent positive-one-frame, zero and regressing-time checks preserve raw values and refusal behavior. Existing native assertions remain unchanged. The repaired native ladder passed all 18 fixtures in 105.40 seconds.

Final recovery review found that a clock error after a finished worker could leave its in-memory entry blocking shutdown. Matching ID/generation bookkeeping now ends before fallible clock/catalog work, while durable protection remains. One observed clock is reused for completion and fallback hold. An injected clock-error fixture checks stale-generation refusal, unchanged protected history, an idle actor and subsequent restart hold. Final integrated verification includes this later repair.

The first ordinary gate then caught two fixture defects. The new gap matrix paired a gap with an incompatible `end_of_body` publication, correctly refused by the existing publisher; valid `stream_gap` inputs now exercise the intended refusal messages. The genuine v29 migration oracle needed the new deletion trigger in its latest-schema expectation. It still compares every original dependent definition verbatim, checks both new guard definitions and demonstrates actual deletion/release refusal after legal retained admission. Both focused cases and strict workspace Clippy pass. Production validation and old assertions were preserved.

The five-format recording ladder now additionally plays a protected retained range on each generated clip. One focused real-decoder run gave:

| Format | Stored decoded microseconds | File seek microseconds | Raw elapsed microseconds | Expected remainder microseconds |
| --- | --- | --- | --- | --- |
| WAV | 1,000,000 | 250,000 | 750,000 | 750,000 |
| MP3 | 1,152,000 | 250,000 | 902,000 | 902,000 |
| AAC/ADTS | 1,152,000 | 250,000 | 902,000 | 902,000 |
| FLAC | 1,000,000 | 250,000 | 750,000 | 750,000 |
| Ogg Vorbis | 1,000,000 | 250,000 | 750,000 | 750,000 |

The 100,000 microsecond tolerance was declared before these observations and did not change. Codec padding remains in the stored measured duration. This compares retained-range progress against capture's measured duration on one loopback-generated clip per format, without independent sample accuracy, speech quality or a supported codec matrix.

The final frozen tree passes `cargo verify`: 1,003 ordinary test executions, 19 ignored, formatting, warnings-denied Clippy, build, native-source hashes and the advisory audit of 313 dependencies against 1,290 advisories. Final `cargo verify-media` passes all 18 fixtures in 97.70 seconds. The ladder includes the five-format protected seeks above, retained MPEG-TS, scheduled late-start offsets, offline replay, stop/restart holds and a 60-second recording sought to its last 750 ms under the existing 20-second CLI fixture deadline.

Full `cargo verify-coverage` passes ordinary tests, the ignored measurement and all 18 native fixtures, including test sources and build scripts without exclusions:

| Workspace crate | Covered / executable lines | Line coverage |
| --- | --- | --- |
| `sigy` | 21,561 / 24,213 | 89.04% |
| `sigy-core` | 1,436 / 1,521 | 94.41% |
| `sigy-service` | 56,741 / 60,877 | 93.20% |
| `sigy-test-recognizer` | 257 / 283 | 90.81% |
| `sigy-xtask` | 1,130 / 1,215 | 93.00% |

The immutable local JSON receipt is `target/coverage-reports/workspace-20208-1791092189374970400.json`, SHA-256 `23d56485d0ea5996c9e3cd43142cbb182ac1ac49fcc238b07b4b27040e40c358`. Display percentages truncate to two decimal places; the gate uses exact integer counts. All 427 frozen runtime/configuration entries and 14 retained asset/legal hashes remain unchanged through verification.

Actual final-binary help, plain/JSON reader inspection and missing-ID guidance pass on a private empty library. Before/after doctor output is identical and no service starts. A leading-option reader canary is refused without opening another library. The first private inspection script incorrectly expected a full control snapshot from page-only CLI JSON; correcting the script required no product change. No TUI layout or audible-control increment ran; existing production-render tests pass in the full gate.

Private logs retain the failed ordinary/media runs and final gates. Final ordinary log SHA-256 is `d60683a7e84b0fece73b0a0d44ca6bd48e3ec69ddcbc41d8a57d8f708fcb6a9d`; final media log is `737502d273ad50e9cdf1b2670b938c9ec5c1ba3a7227071c574f45a0d9480f8e`. The initial failed ordinary log is `60f78dd920cb82ede7b0f50b3f6f259f4c87df7f4476ef5c0877f15326bdef90`; initial failed media log is `c8d792b97b1d193cef266fef6aa754136de089a31ecd60ea8d2860e8282270e3`. No diagnostic upload ran.

Full required commands are `cargo verify`, `cargo verify-media` and `cargo verify-coverage`, sequentially on the pinned Rust 1.98.1 toolchain. Ordinary tests use two threads, media tests one, and Cargo two jobs. Coverage retains every workspace crate, test source and build script with the existing 80% per-crate gate. Exact-commit hosted verification remains required before source-only publication.

## Remaining work

DV-01B adds exact cited start/end excerpts. DV-01C adds separately admitted segment handoffs, gap-aware navigation and actual audible player state. EX-02B links focused station context to immutable source and recording identities. These require their own normal, hostile, recovery and real-media evidence. No segment stitching, prefetch, forced hold clearance or interactive TUI playback is added here.

Primary references reviewed on 2026-10-03: [FFmpeg output-side seek, duration and progress](https://ffmpeg.org/ffmpeg.html), [Tokio 1.53.1 blocking-task cancellation](https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html), and [SQLite encoding and type conversions](https://www.sqlite.org/lang_expr.html#cast_expressions). Upstream semantics support the design; the measurements above establish only the observed local behavior.
