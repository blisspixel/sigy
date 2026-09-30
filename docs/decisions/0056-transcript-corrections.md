# Transcript corrections

Date: 2026-09-29. Status: implemented and tested on Windows x86_64. Catalog schema and local IPC are v36. This is the local exit of roadmap operation 29. One call appends one original-script cue edit. Names, language-span edits, interpretations, and automatic reprocessing remain. No language is qualified. Stage 5 stays open because operations 23, 24, 25, and 28 are still partial. Operation 28 is not exited. Amended the same day by [stored findings](0057-stored-findings.md): a stored finding can cite this correction or a recognition revision. This command does not publish a finding. Catalog schema and local IPC are now v37. Amended the same day by [briefings](0058-briefings.md): a briefing can include a finding that cites this correction. This command does not publish a briefing. Catalog schema and local IPC are now v38.

## Decision

`analysis correct ID --expect N --ordinal U --text SCRIPT` appends one transcript revision. The same command is `analysis_correct` through `sigy mcp`. One call replaces one cue. Every other cue is copied with the same ordinal, start, and end. The caller does not supply times.

The replacement is at most 4,096 bytes in the service. The agent tool's shared text slot remains 2,048 bytes, and its ordinal slot accepts 0 through 255. Empty text, a NUL, and any other control character, including a newline, are refused. An identical script is `correction-unchanged` and writes nothing.

`--expect` is the revision last read. It must be the newest revision, in 1 through 64. The stored revision is that number plus one. Its parent is the expected revision. Its kind is `correction`, its outcome is `text`, and its profile is `user-correction-v1`. The role stays `original`. The job id, job generation, and profile digest are null. Cue count and text bytes match the copied cues. The state is published. One zero-USD analysis decision is stored with no request id. The service clock must be at least zero and at least the parent revision's clock. No coverage row is stored. Wording stays `uncertain`. The rendered page says the previous revision stays readable and that wording remains uncertain. This is not human review.

A second edit that names the same expected revision is `revision-conflict` and writes nothing. A missing pin, or an expected revision with no row, is not found. Expecting revision 2 when only revision 1 exists is not found. A parent whose outcome is not text, including a legacy placeholder and a `no_text` recognition, is `correction-unavailable`. When the analysis input is not the current published revision, or the recording is not retained, the result is `input-expired` and the previous revision stays current. The command does not restore media, change retention, or enqueue work. Expecting 64 when revision 64 is already newest is `revision-limit`.

Dependents are not rewritten. A translation page includes `stale: true` only when that translation follows an older transcript revision. The field is absent when the translation follows the newest revision, and absence means current. The newest transcript page sets `stale_translation_of` when this revision is newest, this revision has no translation, and an older revision of this transcript has one. It sets `stale_language_of` by the same rule for language evidence bound to an older revision. Unbound language evidence, with a null transcript id, is not reported stale. Reading the older revision with `--revision` still returns its cues and its translation.

Monitor passage matches and translation counts follow the newest text revision: a recognition or a correction whose outcome is `text`. Recognition coverage microseconds stay on the recognition revision, so transcribed audio is unchanged by a cue edit. After a correction of a translated cue, translated cues are zero and cues without a translation equal the cue count, until a later translation of the new revision exists.

`analysis correct` does not call the dispatcher. It inserts no analysis job, translation job, provider attempt, or ledger event. A global limit of zero and a paid limit of zero leave those counts unchanged. An explicit later `analysis translate` of the new revision is allowed, because a correction with text is a text revision. That path is local and zero USD. The stale translation stays unresolved until that separate request.

A `BEFORE INSERT` trigger refuses a correction that is not the next revision of the current newest text parent, or whose input is not the current retained pin. The abort text is `transcript revision conflicts`, mapped to `revision-conflict`. The library lock serializes writers.

Migration 036 widens the transcript checks to admit `correction` and installs those triggers. A catalog whose stored definition already contains the widened kind check skips the table rebuild and still installs the triggers. Child foreign keys are deferred and checked before the migration commits. Opening audits a correction's profile, parent, job bindings, cue times, and the absence of coverage rows.

## Evidence

A two-cue recognition is corrected at ordinal 0. Revision 1 still reads the original scripts after the catalog is reopened. The unedited cue keeps its start and end. A second correction, expecting revision 2, copies the first correction and replaces ordinal 1 from the immediate parent. Another transcript is unchanged. Coverage rows stay on the recognition revision. The new row is `user-correction-v1`, wording `uncertain`, amount `0.000000`, with no job and no profile digest.

The same expected revision conflicts after the first append. A raw insert that reuses that revision aborts with `transcript revision conflicts`. An identical script, empty text, a newline, a NUL, another control character, a missing ordinal, a missing pin, a missing revision, and a clock before the parent write nothing. A legacy placeholder and a `no_text` revision are `correction-unavailable`. Deleting the retained recording, then correcting, is `input-expired`, and revision 1 remains newest.

With the global and paid limits at zero, job, translation-job, provider-attempt, request, and ledger counts stay unchanged. A monitor whose term matched the recognized cue matches only the term that survives the corrected script. Transcribed microseconds stay positive and unchanged. Translated cues drop to zero until an explicit local translation of revision 2, which clears the stale mark and still creates no provider attempt. Bound language evidence is stale only on the newest transcript page. Unbound evidence is not. The rendered correction says the previous revision stays readable. The rendered older translation says it is stale, and the previous revision stays readable.

On 2026-09-29, `cargo verify` passed 518 tests, with 15 native-media tests ignored, warnings-denied Clippy, a locked build, and `cargo audit` of 312 crate dependencies against 1,277 advisories. The decoder command line and HTTP acquisition are unchanged, so `cargo verify-media` was not rerun. This is one Windows host, not a platform matrix.

## Limitations

- One cue per call. Times are copied. The caller cannot move a cue.
- Wording stays uncertain. A correction is not human review and qualifies no language.
- Names, language-span edits, and interpretations are not this command.
- Nothing is reprocessed automatically. A stale translation or a stale language-evidence binding stays until a later explicit request.
- The command does not restore media or change retention.
- The command does not dispatch, so an exhausted allowance has no request to refuse beyond the refusal to write a paid row.
- The agent tool's text cap is 2,048 bytes, and its expected-revision slot stops at 63. The service accepts 4,096 bytes per cue. The revision-limit result applies when the newest revision is already 64. The fixture set does not insert 64 revisions.
- A recognition phrase stored through a window end can still clip a word. This command does not reopen that recognition.
- Linux delegated-cgroup containment and the suspended-spawn assignment window remain deferred.
- Stage 5 is not exited. Operations 23, 24, 25, and 28 remain partial. Operation 28 is not exited.
