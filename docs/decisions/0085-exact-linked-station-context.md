# 0085: Exact linked station context

Date: 2026-10-04. Status: implemented increment; verification evidence belongs in [active work](../development/progress.md). This is EX-02B, without a complete listening desk or stage exit.

## Decision

Preserve EX-02A's immediate local Enter inspection and exact Back restoration. Inside that frozen context, `i` explicitly reads immutable directory registrations and their exact source-bound recording metadata. `sigy radio linked UUID` uses the same read-only operation. Neither route registers a source, contacts a station, refreshes the directory, creates a recording, probes a media file or starts playback.

The service selects only `source_directory_links` with provider `radio_browser` and the complete canonical station UUID. Each source remains bound to its immutable revision and original directory metadata. A changed name, origin or endpoint cannot retarget historical recordings. Current cache presence is a separate observation, not proof that an older revision is the current endpoint. A missing cache entry does not erase a historical registration.

The projection returns at most four source revisions in revision-ID order and four exact recording rows per revision in recording-ID order. A fifth identity supplies a real more marker without decoding its full metadata. No library-wide source or recording scan, inferred name/URL join, full result count, new scheduler or second catalog is introduced. An empty complete scoped result can report no registrations or recordings for that exact identity. More markers mean that later results were not inspected; this slice provides no pagination.

Recording summaries expose immutable recording/source identity, capture state, storage state, retention class, published byte/duration metadata and indexed presence of sealed retained segments, release receipts and gaps. These are catalog observations. They establish neither continuous coverage nor filesystem availability, and they grant no playback authority. Released, deleted and unpublished media remain distinguishable through the stored fields. A missing external file is not detected by this read.

## Consistency and bounds

TUI requests include the frozen page's complete catalog namespace, revision and comparison identity. Mismatch refuses the read while retaining the selected cached observation. The fresh CLI route omits an expected catalog and receives the current catalog from the same read transaction. Every successful response echoes provider, station UUID and catalog. Independent monotonic context-request generations fence delayed replies after Back, reopen or replacement. A refused or failed read keeps any prior linked observation with an explicit failure label.

One deferred catalog transaction and the existing whole-operation guard cover baseline integrity/ledger inspection plus the projection: 100 ms cooperative deadline, 4 million checkpoint-counted VM operations and 10 ms lock wait. Production checks occur at 1,000-op SQLite statement checkpoints; short statements can execute without a callback, so this is not an exact cumulative opcode ceiling across many short statements. The fixed four-by-four projection separately bounds their count. These bounds establish no latency qualification. Raw source strings and the 8 KiB immutable directory JSON are checked before copying/parsing. Source configurations use the canonical decoder. Vectors have fixed four-row capacity; final page encoding uses the existing capped writer with a 128 KiB limit. The transport retains its separate whole-frame cap.

Schema 50 adds only `source_directory_links(provider,station_id,source_revision)` and `capture_jobs(source_revision,id)` lookup indexes. Historical rows, hashes and source grants remain unchanged. Local IPC advances to 51. There is no dependency change in this linked-context increment.

## Acceptance and limitations

Require real API paths for registration, refresh drift, duplicate station names, recording publication, gaps and whole-recording deletion. Check exact bindings, absent registration, four/five lookahead boundaries, independent query-plan assertions, multibyte corrupt metadata refusal, work exhaustion/guard cleanup, populated 49-to-50 migration/reopen and exact legacy definitions. A controlled cache-removal test is resilience evidence only: current refresh merges and has no public deletion route.

Render pending, loaded and failed states through production compact 80x24 and wide 132x40 layouts, preserving original scripts, Back/scroll instructions, more/unknown labels and exact query/selection restoration. Synthetic rendered observations do not qualify actual source availability, media continuity, font shaping or interaction latency. The current terminal event loop awaits the service read; request-generation fixtures establish adoption rules, not an asynchronous UI guarantee.

Verification receipts belong in active work after root gates complete. No release support, stage exit, language-quality or playback qualification follows from this read projection.
