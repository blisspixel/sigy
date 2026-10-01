# Briefings

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC are v38. This is the local exit of roadmap operation 33: one generation over stored findings, with coverage stated first, one repetition group counted once, and conflict left unresolved. Classification stays off. Frozen coverage, a redacted export, and a classifier remain. No language is qualified. Stage 5 stays open because operations 23, 24, 25, and 28 are still partial. Operation 28 is not exited. Operations 34 and 35 remain. Amended 2026-09-30 by [frozen briefing coverage](0059-frozen-briefing-coverage.md): coverage on a stored briefing is the snapshot frozen at publish. `monitor coverage` still reads live. This briefing command does not by itself write the export. Catalog schema and local IPC are now v39. The subsequent [classification-off increment](0061-classification-off-and-monitor-inspection.md) supplies operation 35 local evidence.

## Decision

`monitor briefing MONITOR ID add` stores one generation for a coverage window. The window defaults to the last 24 hours. `--hours` accepts 1 to 744. `--from-ms` and `--to-ms` name an exact range. `monitor briefing MONITOR ID show` reads that generation and takes no window. `sigy mcp` has no tool that publishes a briefing. The existing monitor tools still only read or propose.

The service accepts a window whose start is at least zero, whose end is later, and whose length is at most 31 days (2,678,400,000 milliseconds). Any other window is `monitor window` and writes nothing, including when the same briefing id is repeated. A service clock below zero is `briefing clock` before the window check, so a repeated window with a negative clock still writes nothing.

The same monitor id, briefing id, and identical window returns the stored generation and writes nothing, even when a finding was stored later. The same id with a different window is `briefing-conflict` and writes nothing. A new projection uses a new briefing id. A raw insert whose generation is not the next integer aborts with `briefing generation`. That abort is a storage integrity error, because the service already assigns the next generation.

Each publish freezes finding membership inside one immediate transaction. Every stored finding on the monitor becomes a member, in finding-id order. The window selects the coverage that the page reports. It does not choose which findings are members. A finding stored after publish is absent from that generation. A later show still reports only the members stored with the generation.

Coverage is read again on show, through the same count as `monitor coverage` for the stored window. The rendered text places that coverage, including the daily audio cap, before the findings. The page says finding membership is frozen at the monitor version that was current when the generation was stored, and that coverage is the current read of this window. Amended 2026-09-30 by [frozen briefing coverage](0059-frozen-briefing-coverage.md): show reads the snapshot stored with the generation. `monitor coverage` stays a live read.

Repetition compares the original script after Unicode lowercase and whitespace folding. Runs of whitespace become one space, and the ends are dropped. No edit distance and no Unicode normalization form are applied. Near-identical paraphrases stay in separate groups. Sharing a retained interval does not merge findings: a correction can reuse cue times while the script changes, and merging those would hide the conflict. Interval-hash rebroadcast grouping remains.

Groups are numbered from zero in the order a folded script is first seen. Corroboration is the number of distinct groups. A repeated report does not raise that number. A script that appears once is its own group and still counts once. The rendered repetition-group count is that same number. A monitor with no findings stores corroboration zero and no members, so coverage can still be stated. The exit fixture has findings.

The text names stable ids. A group of two or more prints `Repetition group N: id, id. These copies count once.` and then one line per finding with its original script. A single finding prints only `Finding id: script`. The page also carries each finding's English text or untranslated reason, and whether a newer transcript or translation exists. Those stale flags are computed on read. The member row is not rewritten. The retained interval and recording stay on the finding. Then the text states that support is none, contradiction is unresolved, independence is unresolved, and classification is off. Wording remains uncertain. This is not human review.

Classification is stored as the literal `off`. There is no classifier and no command that marks support or contradiction. Publishing inserts no analysis job, translation job, provider attempt, request, or ledger event. Transcript text and a model proposal do not create a briefing.

Each monitor holds at most 1,024 briefings. The Rust check and a `BEFORE INSERT` trigger both refuse the next row. The trigger abort `briefing limit` is `briefing-limit`. The fixture set does not insert 1,024 rows. Generations run from 1 through 1,024, are unique per monitor, and stay contiguous. Update aborts with `briefings are immutable`. Delete aborts with `briefings are retained`. Member rows use the same messages. Opening audits classification, the cap, contiguous generations, and that corroboration equals the distinct group ordinals starting at zero. An empty table passes.

A generation stays in the catalog. It has no media file, so it does not count toward the 50 GB media quota, and temporary-recording retention does not delete it.

A missing monitor is not found. Show of a briefing id that was never stored is not found. Monitor ids and briefing ids use the same key rules as other monitor ids.

Migration 038 creates `monitor_briefings` and `monitor_briefing_members` and their triggers, and sets the catalog user version to 38. Local IPC is 38 because the monitor page gained a briefing variant. A catalog opened at the current schema and then rewound can reinstall the tables and triggers.

## Evidence

One local Spanish cue family. Findings `world` and `copy` cite transcript revision 1, original script "Una feria mundial", English "A world fair". A correction to "Una feria local" is transcript revision 2, translated as "A local fair", and finding `local` cites that revision. The briefing window covers the recording. Corroboration is 2 and the generation has 3 members. `copy` and `world` share group 0. `local` is group 1. `world` is stale because the correction is a newer transcript. `local` is current. Classification is `off`. Coverage uses that window. A model proposal of the cue script leaves the briefing count at zero. Publishing leaves analysis jobs, translation jobs, provider attempts, requests, and ledger events unchanged. A finding stored afterward is absent from the generation. Reopening the catalog still loads the three members.

Repeating the same window returns that generation and leaves the count at one. A different window is `briefing-conflict`. A negative clock is `briefing clock`. A missing monitor is not found. A raw insert with generation 5 aborts with `briefing generation`. Update and delete abort.

The rendered page places the daily audio cap before `Repetition group 0: copy, world.` It states three reports, two repetition groups, and corroboration 2, and it says repetition is not independent corroboration. Support is none. Contradiction and independence are unresolved. Classification is off. The text does not contain the word classifier. `briefing fair week add --from-ms 0 --to-ms 60000` parses as one publish for that window.

On 2026-09-29, `cargo verify` passed 530 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder command line and HTTP acquisition are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix. The fixture is local storage. It is not a public-station run.

## Limitations

- Amended 2026-09-30 by [frozen briefing coverage](0059-frozen-briefing-coverage.md): show reads the snapshot stored with the generation. `monitor coverage` stays a live read. The export is a separate read.
- Finding membership is every stored finding at publish time. The window does not filter findings by cue time.
- The briefing text shows finding ids and the original script. English, an untranslated reason, and stale flags are on the page. The retained interval stays on the finding.
- Repetition is exact lowercase and whitespace equality of the original script. It is not edit distance and it is not an interval hash. Near-identical paraphrases stay unresolved.
- Support, contradiction, and independence stay unresolved. This command has no way to mark them. Classification stays off. Operation 35 remains that requirement.
- Wording remains uncertain. A briefing is not human review and qualifies no language.
- The 1,024 cap is enforced by the trigger and the Rust pre-check. The fixture set does not insert 1,024 rows.
- Show of a briefing id that was never stored returns not found. The fixture covers a missing monitor on publish.
- `sigy mcp` cannot publish or export a briefing.
- The fixture is one local Spanish cue family. It is not a public-station run.
- A recognition phrase stored through a window end can still clip a word. This command does not reopen that recognition.
- Linux delegated-cgroup containment and the suspended-spawn assignment window remain deferred. The window stays open.
- Stage 5 is not exited. Operations 23, 24, 25, and 28 remain partial. Operation 28 is not exited. The subsequent [classification-off increment](0061-classification-off-and-monitor-inspection.md) supplies operation 35's local evidence; stage 6 remains open.
