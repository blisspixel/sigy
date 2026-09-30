# Frozen briefing coverage

Date: 2026-09-30. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC are v39. This is the local exit of roadmap operation 34: one coverage snapshot is frozen with the briefing generation, an earlier generation stays readable, a later correction marks the cited finding stale and writes no ledger event, and a redacted export is a snapshot. Classification stays off. `monitor coverage` stays a live read. No language is qualified. Stage 5 stays open because operations 23, 24, 25, and 28 are still partial. Operation 28 is not exited. Operation 35 remains.

## Decision

`monitor briefing MONITOR ID add` copies coverage in the same immediate transaction as the briefing header and its members. `show` reads that copy. `monitor coverage` still counts the sources the monitor follows now. The same window returns the stored generation and writes nothing, including no second coverage copy. A different window is `briefing-conflict` and writes nothing. A second briefing id is the next generation and leaves the first in place.

The stored coverage keeps the monitor version and the daily audio cap current at publish, one row per followed source, and one row per schedule of that version. Source order is the order of the sources followed then. Reason order is the order stored with that source. Ordinals are contiguous from zero. A source with more than 256 reasons, an empty reason, a reason longer than 1,024 bytes, or a reason count of zero is a storage integrity error. Update of those rows aborts with `briefings are immutable`. Delete aborts with `briefings are retained`.

A later monitor revision can raise the daily and total audio caps together. The live coverage read then reports the new cap and the new version. The stored generation keeps the earlier cap and version. A briefing published after that revision freezes the new cap. Reopening the catalog returns both generations unchanged.

A transcript correction after publish marks the cited member stale on the next read. The stored original script stays the cited revision. The correction adds no finding, analysis job, translation job, provider attempt, request, or ledger event. It records a zero-USD analysis decision with no request, which is the existing correction record. The coverage snapshot stays as stored.

An insert that is not committed leaves the briefing count unchanged. A committed briefing without a coverage header fails the next open as catalog integrity, so a partial generation cannot remain. Publish commits the header, the members, and the snapshot together. No trigger requires the coverage row at the moment the briefing row is inserted, because the snapshot is written after that row. The open audit is the backstop.

`monitor briefing MONITOR ID export` is a read. It writes nothing. The document name is `sigy.briefing`, the document version is 1, `catalog` is false, and `authority` is `none`. The note is: "This snapshot is not the catalog. It grants no permission, spend, or retention. Wording remains uncertain. This is not human review." The document carries the monitor id, briefing id, generation, window, monitor version, classification, corroboration, the frozen coverage, and the members, including English or an untranslated reason and the stale flags. It carries no filesystem paths, stream URLs, secret names, ledger balances, job ids, or provider routes. A source revision id is an evidence identity. Other commands still print the whole service view under `--json`. Export prints only this document, including when `--json` is set. `sigy mcp` has no tool that publishes or exports a briefing.

The rendered briefing text says coverage was frozen when the generation was stored, and it still places that coverage before the findings. Classification stays the literal `off`.

Migration 039 creates `monitor_briefing_coverage`, `monitor_briefing_sources`, `monitor_briefing_reasons`, and `monitor_briefing_schedules`, with their immutability triggers, then copies coverage for any briefing that has none. A fresh catalog has no briefings, so that copy writes nothing. A catalog upgraded from v38 freezes coverage at upgrade, and the briefing header keeps the monitor version stored at the original publish. Those two versions are not required to match. Local IPC is 39 because export is a monitor operation and the service view can carry the redacted document. Schema and IPC move together because the rows are stored.

## Evidence

One local Spanish cue. Finding `world` cites transcript revision 1, original script "Una feria mundial". The monitor's daily and total audio caps are 3600 seconds. Briefing `week` freezes daily audio seconds 3600 and monitor version 1. Revising both caps to 7200 makes the live coverage read report 7200 and version 2. `week` still reports 3600 and version 1, and its source list matches that live source list. Briefing `next` on the same window freezes 7200 at generation 2. `week` stays generation 1.

A correction of the cited cue to "Una feria distinta" appends transcript revision 2. The finding count stays the same. Analysis jobs, translation jobs, provider attempts, requests, and ledger events stay the same. The stored script stays "Una feria mundial" and the member is stale. The coverage snapshot is the one frozen before the correction. The export document has `catalog` false, document `sigy.briefing`, and authority `none`, and its daily audio cap is 3600. The JSON contains "not the catalog" and does not contain `http`, `ledger`, or `secret`. Building the export leaves the briefing count unchanged.

A transaction that inserts a later generation and ends without commit leaves the count unchanged, and that id is absent. Updating `monitor_briefing_coverage` fails. Reopening the catalog still returns `week` at 3600 and `next` at generation 2 and 7200.

A briefing row inserted without a coverage header fails the next open as catalog integrity.

The rendered page says coverage was frozen when the generation was stored. It does not say the coverage is the current read. The daily audio cap still precedes the repetition group. `briefing fair week export` parses as the redacted read. That JSON contains the note and does not contain `http`, `ledger`, or `secret`.

On 2026-09-30, `cargo verify` passed 532 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder command line and HTTP acquisition are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix. The fixture is local storage. It is not a public-station run.

## Limitations

- Repetition is exact lowercase and whitespace equality of the original script. It is not edit distance and it is not an interval hash. Near-identical paraphrases stay unresolved.
- The window does not filter findings by cue time. Finding membership is every stored finding at publish.
- The briefing text shows the original script. English, reasons, and stale flags are on the page and in the export. The retained interval stays on the finding.
- The fixture shows that the daily cap and the monitor version stay frozen, and that an update of the coverage row is refused. It does not insert a new capture, so source counts do not drift in this run.
- An upgraded v38 briefing freezes coverage at upgrade. The fixture creates briefings on the current schema, in the publish transaction, so it does not run that upgrade copy.
- The 1,024 cap is enforced and not fixture-tested with 1,024 rows.
- `sigy mcp` cannot publish or export a briefing.
- Wording remains uncertain. This is not human review. Classification stays off. Operation 35 remains that requirement.
- The fixture is one local Spanish cue family. It is not a public-station run.
- A recognition phrase stored through a window end can still clip a word. This command does not reopen that recognition.
- Linux delegated-cgroup containment and the suspended-spawn assignment window remain deferred. The window stays open.
- Stage 5 is not exited. Operations 23, 24, 25, and 28 remain partial. Operation 28 is not exited. Operation 35 has not started.
