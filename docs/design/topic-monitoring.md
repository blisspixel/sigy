# Topic monitoring

Status: proposed design, 2026-09-24. This turns roadmap operations 30 to 35 into a concrete first slice on the implemented recognition, language evidence and translation records. Nothing here is implemented yet. It follows [analysis and knowledge](../planning/10-analysis-and-knowledge.md) and the [scaling architecture](scaling-architecture.md).

## What a user does

"Follow reports about the Nile dam in Arabic, French and English news for a week, at most 6 hours of audio a day, locally." Sigy records the named sources on the saved schedules, transcribes and translates what it captures, and keeps a briefing: matching passages grouped into reports, each linked to the exact original words, their translation, and the retained audio. The user can see what was covered, what was missed, and what Sigy did and why.

## Records

| Record | Contents | Mutability |
| --- | --- | --- |
| Monitor | Stable ID, name, created time | Immutable identity |
| Monitor version | Goal text (the user's words), terms per language, source revisions, schedule references, daily and total audio caps, processing profiles, paid allowance scope (zero by default) | Append-only; a change is a new version |
| Monitor action | Proposed change, its origin (user, schedule, rule, model proposal), the policy version checked, the admission decision (applied, refused, deferred) with its reason, the outcome, and a reservation ID or an explicit zero cost | Append-only |
| Coverage snapshot | Per source and window: planned, captured, decoded, transcribed, translated and included durations; gaps with causes; missed windows | Append-only per generation |
| Passage match | One transcript cue range (and its translation cue) that matched a monitor term, with the matching method and the exact revisions | Immutable; stale when a dependent revision changes |
| Finding | A claim-level grouping of passage matches with wording, time bounds, and relationships | Revisioned |
| Relationship | Between findings or passages: repetition (mechanical), support or contradiction (user or measured classifier), or unresolved | Revisioned, with origin |
| Briefing generation | A projection over a named set of findings and coverage, with its own generation number | Append-only; earlier generations stay readable |

## Rules

- **Authority.** A monitor version is created only by the user through the CLI, TUI or an explicit agent tool call. Text in a transcript, translation, feed or model output can propose an action but cannot apply one, change a cap, add a source outside the version's list, or open a paid route. A refused proposal is kept with its reason.
- **Citations.** A finding is published only if every cited range exists: one translation revision, the original transcript revision it translates, and one retained interval, or an explicit statement that the original audio has expired or is missing.
- **Coverage.** Planned, captured, decoded, transcribed, translated and included time are separate numbers. Gaps and missed windows stay in the denominator. A briefing states coverage before it states findings.
- **Repetition is not corroboration.** Two passages with near-identical normalized text, or a rebroadcast of the same interval hash, form a repetition group that counts once. Independence between sources is unknown unless the user states it.
- **No classifier by default.** Support and contradiction are "unresolved" until a user marks them or a later measured classifier profile, with its own allowance, proposes them. Missing classification is shown as off or pending, never as agreement.
- **Corrections.** A new transcript or translation revision marks dependent passage matches and findings stale. Recomputation runs through the normal queue and policy; a correction grants no new source, spend or retention.
- **History.** A new briefing generation never deletes an older one. A crash while publishing a generation leaves either the whole generation or none.

## Matching without a model

Terms are stored per language as the user wrote them, in any script. Matching runs on the original transcript text and, separately, on the English translation, after Unicode NFC normalization, case folding where the script has case, and whitespace folding. A match records which text matched (original or English), the term, and the cue range. This is deliberately simple and explainable: it finds literal mentions, misses paraphrases, and says so. A later semantic matcher is a separate, measured profile.

## First increments

1. **Monitor versions and the action log (operation 30).** Storage for monitors, versions and actions, with the authority rules above; CLI `monitor create`, `monitor revise`, `monitor show`, `monitor actions`. Exit: out-of-policy proposals are refused and kept, rows survive restart, and nothing in a transcript can create a version or action.
2. **Coverage and scheduling (operation 31).** Monitor versions reference existing schedules; coverage snapshots count each stage separately, with missed windows. Exit: counters for a fixture day with a gap, a missed window and an untranslated language.
3. **Passage matches and cited findings (operation 32).** Term matching over published transcripts and translations; findings refuse missing ranges. Exit: a missing range is rejected, and a match in Arabic original text links to its English cue and its audio interval.
4. **Briefings without a classifier (operation 33).** Repetition groups, unresolved relationships, coverage-first briefing text, and Markdown export with stable IDs. Exit: a fixture with repeated and conflicting reports shows one repetition group and unresolved conflict.
5. **Projection history and correction propagation (operation 34), classification off (operation 35).**

## Open questions

- Whether monitors may add sources from a directory search automatically within a user-approved list, or only propose them. The first slice only proposes.
- How long briefing generations are kept, and whether they count toward the library quota.
