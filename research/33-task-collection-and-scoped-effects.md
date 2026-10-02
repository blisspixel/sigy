# Task collection and scoped effects

Reviewed: 2026-10-01. Status: primary-source research and implementation contract review. This extends [agentic analysis](15-agentic-analysis.md), [private diagnostics and recovery](32-private-diagnostics-and-recovery.md), and the [durable task workflow](../docs/design/task-workflows.md). No dependency, model, paid request or external inference was selected.

## Findings

[SQLite transaction control](https://www.sqlite.org/lang_transaction.html) documents one simultaneous writer and the immediate acquisition of a write transaction. An error may undo a statement without undoing every preceding statement. Application code must preserve the intended transaction boundary and handle unsuccessful completion.

**Application inference:** an accepted collection grant, its new schedule rules and their ownership bindings belong in one immediate transaction. The scheduled recording, conservative capture reservation and occurrence transition belong in the existing admission transaction. A grant alone is not a recording reservation or permission to connect outside the existing source grant. Inject failure at both statement and commit boundaries, close and reopen, and compare with an uninterrupted reference.

[SQLite foreign-key guidance](https://www.sqlite.org/foreignkeys.html) describes referential enforcement and deferred checks at commit. Valid references establish existence, not the meaning of a relationship.

**Application inference:** catalog audits must additionally verify immutable scope hashes, exact source revisions, planned intervals, original rule versions and recording provenance. Selecting every recording from the same source cannot establish task ownership. A generation fence must be checked in the same transaction that admits an effect; checking it only in a client or on an earlier tick leaves a stale-authority window.

[SQLite backup guidance](https://www.sqlite.org/backup.html) describes snapshot techniques, including the existing `VACUUM INTO` alternative. A catalog snapshot does not coordinate separately retained media.

**Application inference:** collection history must survive the canonical verified backup and restore path with its original reservations and identifiers. A restored task cannot gain another lifetime grant. Process restart, verified restore and physical power-loss durability are distinct checks. Existing offline ownership, media verification and manifest publication remain necessary.

## Alternatives and chosen increment

| Alternative | Consequence |
| --- | --- |
| Adopt a pre-existing schedule | Conflates independent authority with a task and makes cancellation ambiguous |
| New scheduler or workflow queue | Duplicates civil-time, admission, recovery and resource policy |
| Grant unlimited recurring collection | Prevents a finite workflow from demonstrating bounded effects |
| Bind new once schedules to one finite task grant | Reuses the scheduler and preserves exact ownership without changing independent work |

The first collection increment chooses one lifetime grant with one or two explicit authorized source revisions, finite UTC windows and byte ceilings, and zero paid allowance. It binds the inspected monitor version and complete action prefix. Existing monitor capture budgets remain shared and conservatively charged at admission. Cancellation fences future task collection; already admitted capture and independently authorized processing keep their existing authority.

Task processing requires a further explicit contract. A monitor can process a recording under its own saved policy; that does not make its recognition job task-owned. Before task cancellation can stop processing, canonical jobs need durable interests that distinguish task, monitor and direct authority. Input checksum, profile identity, transcript revision, effect receipt and usage charge must then be bound atomically. A general planner follows those execution guarantees and needs measured task competence.

## Required evidence and limits

The collection gate includes exact and conflicting replay, policy and action drift, cancellation before and after admission, stale generations, original-rule mutation refusal, UTC midnight allocation, missed windows, exhausted shared caps, unrelated same-source recordings, SQL rollback, corruption rejection and populated restore. A loopback native-media check should demonstrate the shared recording path after client exit. Those checks do not qualify speech quality, a planner, sustained capacity, clean-host recovery or a supported release.

The public contract and dated outcomes belong in [task-owned collection](../docs/decisions/0069-task-owned-collection.md) and its [validation record](experiments/task-collection-2026-10-01.md). Private logs and disposable libraries remain local.
