# Stored findings

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC are v37. This is the local exit of roadmap operation 32 for one citation: a missing range is rejected. Claim grouping, relationships, briefings, and automatic reprocessing remain. No language is qualified. Stage 5 stays open because operations 23, 24, 25, and 28 are still partial. Operation 28 is not exited. Amended the same day by [briefings](0058-briefings.md): a briefing can include a finding. This command does not publish a briefing. Catalog schema and local IPC are now v38.

## Decision

`monitor finding MONITOR FINDING add --transcript ID --transcript-revision N --translation-revision M --ordinal U` stores one citation on an existing monitor. `--original` is `retained`, `expired`, or `missing`, and defaults to `retained`. `monitor finding MONITOR FINDING show` reads that citation. `sigy mcp` has no tool that publishes a finding. The existing monitor tools still only read or propose.

The service copies the cue's start and end. The caller does not supply a clock or a range. Times are stored only for a true `retained` citation: the recording storage state is `retained`, its checksum matches the transcript, the cue interval sits fully inside one published recording interval, and no recording gap overlaps the cue. The stored interval is the cue's own start and end.

`expired` stores no interval, and only when the recording is `deleting` or `deleted`. A retained recording with an expired statement is `finding-original` and writes nothing. A cue that overlaps a gap is missing, so an expired statement on that recording is also `finding-original`.

`missing` stores no interval, and only when the recording is neither `deleting` nor `deleted`, and the cue is outside every published interval or overlaps a gap. A fully covered retained cue with a missing statement is `finding-original`. Citing `retained` when that range is absent is `finding-range` and writes nothing.

A recording that is `retained` with a checksum mismatch, or `reserved`, while a published interval covers the cue and no gap overlaps it, matches none of the three statements. `retained` returns `finding-range`. `expired` and `missing` return `finding-original`. Nothing is inserted. There is no fourth stored state. This edge is implemented in Rust and in the insert trigger, and the fixture set does not cover it.

The transcript must have role `original`, outcome `text`, and kind `recognition` or `correction`. A legacy placeholder or a `no_text` revision is `finding-range`, checked before the cue and translation lookup. The translation is the row keyed by transcript id, transcript revision, and translation revision, and a translation cue must exist for the ordinal. An untranslated cue, with no English text and a reason set, may still be cited. The rendered page says it is untranslated. A missing monitor, transcript, cue ordinal, or translation is not found.

The row is immutable. The same monitor id, finding id, and identical citation returns the stored page and writes nothing. Stale flags are recomputed on that read. A service clock below zero is `finding clock` before that replay, so a matching citation with a negative clock still writes nothing. The same id with a different citation is `finding-conflict` and writes nothing, including when the new ordinal does not exist. That check happens before the media lookup.

A newer transcript text revision, a recognition or a correction whose outcome is `text` and whose revision is greater than the cited revision, is reported `stale_transcript`. A newer translation of the cited transcript revision is reported `stale_translation`. Each field is present only when true. The row is not updated, and nothing is queued. Correcting a cited transcript leaves the finding's citation, script, and English as stored.

Publishing a finding inserts no analysis job, translation job, provider attempt, or ledger event. Transcript text does not create a finding or a job. A monitor match is computed on read and does not insert a finding. A proposal whose request is the cue script does not insert one.

Each monitor holds at most 1,024 findings. The Rust check and a `BEFORE INSERT` trigger both refuse the next row. The trigger abort `finding limit` is `finding-limit`. The fixture set does not insert 1,024 rows. Update aborts with `findings are immutable`. Delete aborts with `findings are retained`. A raw insert whose times are not the cue's own times aborts with `finding citation`. That abort is a storage integrity error, because the service already refused a false citation before insert. Opening audits the citation predicate and the 1,024 cap. An empty table passes.

The transcript references its recording, so the recording row stays while the transcript exists. Missing describes a cue with no playable retained interval. It does not mean the recording row was removed. Deleting a recording after a retained finding is stored leaves that row as retained. A later finding id can state `expired`.

Wording remains uncertain. The rendered page says the cited revision stays readable, that wording remains uncertain, and that this is not human review.

Migration 037 creates `monitor_findings` and its triggers, and sets the catalog user version to 37. Local IPC is 37 because the monitor page gained a finding variant. A catalog opened at the current schema and then rewound can reinstall the table and triggers.

## Evidence

One local Spanish cue, original script "Una feria mundial" and English "A world fair", is cited from translation revision 1 of transcript revision 1. The retained interval renders as 00:00.000 to 00:01.000 on the recording. Reopening the catalog still loads the finding. A raw insert that changes the start aborts with `finding citation`. Update and delete abort. Proposing that cue script, then reading passage matches, leaves the finding count at zero and leaves analysis jobs, translation jobs, provider attempts, and ledger events unchanged. Publishing the finding leaves those counts unchanged.

A gap that overlaps the cue rejects `retained` as `finding-range` and stores `missing` with no interval. `expired` on that gapped recording is `finding-original`. A recording whose storage state is `deleting` stores `expired` with no interval, and `retained` or `missing` on that recording is refused.

A later correction to "Una feria local" marks the finding stale. The cited revision, the stored script, and the stored English stay. Job, attempt, request, and ledger counts stay equal to the counts from before that correction. An explicit later translation of the cited revision, "A local fair", marks the translation stale, leaves the stored English as "A world fair", and adds one translation job. It adds no ledger event.

A legacy placeholder is `finding-range`. The rendered expired and missing pages cite no interval. An untranslated cue renders its reason. Repeating the add command parses `--original` as `retained` when the flag is omitted, and show only reads.

On 2026-09-29, `cargo verify` passed 525 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder command line and HTTP acquisition are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix. The fixture is local storage. It is not a public-station run.

## Limitations

- One citation per finding id. A briefing can include this finding ([briefings](0058-briefings.md)). This command does not publish a briefing. Relationships beyond that repetition, and automatic reprocessing, remain.
- A newer revision is labeled stale on read. Automatic recomputation remains with operation 34. This command does not enqueue it.
- Wording remains uncertain. A finding is not human review and qualifies no language.
- The unavailable recording state, a retained checksum mismatch or a reserved recording while the interval covers and no gap overlaps, is implemented and not fixture-tested.
- Deleting a recording after a retained finding does not rewrite that row. A new finding id can state expired.
- A recognition phrase stored through a window end can still clip a word. This command does not reopen that recognition.
- The 1,024 cap is enforced by the trigger and the Rust pre-check. The fixture set does not insert 1,024 rows.
- `sigy mcp` cannot publish a finding.
- Linux delegated-cgroup containment and the suspended-spawn assignment window remain deferred.
- Stage 5 is not exited. Operations 23, 24, 25, and 28 remain partial. Operation 28 is not exited.
